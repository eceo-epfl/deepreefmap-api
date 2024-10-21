from fastapi import (
    Depends,
    APIRouter,
    Query,
    Response,
    HTTPException,
    BackgroundTasks,
)
from sqlmodel import select
from app.db import get_session, AsyncSession
from app.submissions.models import (
    Submission,
    SubmissionRead,
    SubmissionUpdate,
    SubmissionCreate,
    SubmissionJobLogRead,
    KubernetesExecutionStatus,
    SubmissionFileOutputs,
)
from app.submissions.utils import (
    populate_percentage_covers,
    iteratively_check_status,
)
from fastapi.responses import StreamingResponse
from app.objects.models import InputObject, InputObjectAssociations
from app.submissions.status.models import RunStatus
from uuid import UUID
from sqlalchemy import func
import json
from typing import Any
from app.objects.service import get_s3
from aioboto3 import Session as S3Session
from app.config import config
from kubernetes.client import CoreV1Api, ApiClient, CustomObjectsApi
from app.submissions.k8s import (
    get_k8s_v1,
    get_k8s_custom_objects,
    delete_job,
    fetch_cached_jobs,
    submit_job,
)
import random
from app.users.models import User
from app.auth.services import get_user_info
import datetime
import jwt
from app.auth.models import DownloadToken

job_log_router = APIRouter()
router = APIRouter()


@job_log_router.get("/{job_id}", response_model=SubmissionJobLogRead)
async def get_job_log(
    job_id: str,
    user: User = Depends(get_user_info),
    session: AsyncSession = Depends(get_session),
) -> SubmissionJobLogRead:
    """Get the log for the given submission job"""

    # log_content = await get_cached_job_log(f"{job_id}-0-0")
    query = select(RunStatus).where(RunStatus.kubernetes_pod_name == job_id)
    res = await session.exec(query)
    job = res.one_or_none()

    if not job:
        raise HTTPException(status_code=404, detail="Job not found")

    return SubmissionJobLogRead(id=job_id, message=job.logs)


@router.delete("/jobs/{job_id}")
async def delete_job_from_k8s(
    job_id: str,
    k8s: CustomObjectsApi | None = Depends(get_k8s_custom_objects),
    user: User = Depends(get_user_info),
) -> Any:
    """Deletes a job from the k8s cluster"""
    if k8s:
        try:
            response = delete_job(k8s, job_id)
        except Exception:
            response = False

        if response:
            return {"message": "Job deleted"}
        else:
            raise HTTPException(
                detail="Error deleting job",
                status_code=500,
            )
    else:
        raise HTTPException(
            status_code=500,
            detail="Kubernetes client initialization failed",
        )


@router.get("/{submission_id}", response_model=SubmissionRead)
async def get_submission(
    session: AsyncSession = Depends(get_session),
    s3: S3Session = Depends(get_s3),
    k8s: CoreV1Api | None = Depends(get_k8s_v1),
    user: User = Depends(get_user_info),
    *,
    submission_id: UUID,
) -> SubmissionRead:
    """Get a submission by id"""

    # Fetch submission from database
    query = select(Submission).where(Submission.id == submission_id)
    if not user.is_admin:
        query = query.where(Submission.owner == user.id)

    res = await session.exec(query)
    submission = res.one_or_none()

    if not submission:
        raise HTTPException(status_code=404, detail="Submission not found")

    # Sort the job_status by time_started
    job_status = sorted(
        # job_status,
        submission.run_status,
        key=lambda x: (
            x.time_started if x.time_started else "9999-00-00T00:00:00Z"
        ),
        reverse=True,
    )
    # Fetch the file outputs in the S3 bucket
    response = await s3.list_objects_v2(
        Bucket=config.S3_BUCKET_ID,
        Prefix=f"{config.S3_PREFIX}/outputs/{str(submission_id)}/",
    )
    outputs = response.get("Contents", [])

    output_files = [
        SubmissionFileOutputs(
            filename=output["Key"].split("/")[-1],
            size_bytes=output["Size"],
            last_modified=output["LastModified"],
            url=(
                "/api/submissions/"
                f"{submission_id}/{output['Key'].split('/')[-1]}"
            ),
        )
        for output in outputs
    ]

    model_obj = SubmissionRead.model_validate(submission)
    model_obj.run_status = job_status
    model_obj.file_outputs = output_files

    return model_obj


@router.get("/download/{token}", response_class=StreamingResponse)
async def get_submission_output_file(
    token: str,
    session: AsyncSession = Depends(get_session),
    s3: S3Session = Depends(get_s3),
) -> StreamingResponse:
    """With the given submission ID and filename, returns the file from S3"""

    # Decode the token from the user
    decoded = jwt.decode(
        token, config.SERIALIZER_SECRET_KEY, algorithms=["HS256"]
    )

    submission_id, filename, exp = decoded.values()

    if datetime.datetime.fromtimestamp(
        exp, tz=datetime.timezone.utc
    ) < datetime.datetime.now(datetime.UTC):
        raise HTTPException(
            status_code=401,
            detail="Token has expired",
        )

    # Make sure the user has access to the submission
    query = select(Submission).where(Submission.id == submission_id)
    res = await session.exec(query)
    submission = res.one_or_none()

    if not submission:
        raise HTTPException(status_code=404, detail="Submission not found")

    # Get the file from the S3 bucket
    response = await s3.get_object(
        Bucket=config.S3_BUCKET_ID,
        Key=f"{config.S3_PREFIX}/outputs/{str(submission_id)}/{filename}",
    )

    return StreamingResponse(
        content=response["Body"].iter_chunks(),
        media_type="application/octet-stream",
        headers={"Content-Disposition": f'attachment; filename="{filename}"'},
    )


@router.get("/{submission_id}/{filename}", response_model=DownloadToken)
async def get_submission_output_file_token(
    submission_id: UUID,
    user: User = Depends(get_user_info),
    session: AsyncSession = Depends(get_session),
    *,
    filename: str,
) -> DownloadToken:
    """With the given ID and filename, returns a token to download the file

    Necessary endpoint to allow the user to download the file but also
    providing some security by forcing authentication first.

    Token expires at a set time defined by config.SERIALIZER_EXPIRY_HOURS
    """

    query = select(Submission).where(Submission.id == submission_id)
    if not user.is_admin:
        query = query.where(Submission.owner == user.id)
    res = await session.exec(query)
    submission = res.one_or_none()

    if submission is None:
        raise HTTPException(status_code=404, detail="Submission not found")

    payload = {
        "submission_id": str(submission_id),
        "filename": filename,
        "exp": datetime.datetime.now(datetime.UTC)
        + datetime.timedelta(hours=config.SERIALIZER_EXPIRY_HOURS),
    }
    token = jwt.encode(
        payload, config.SERIALIZER_SECRET_KEY, algorithm="HS256"
    )

    return DownloadToken(token=token)


@router.post("/{submission_id}/execute", response_model=Any)
async def execute_submission(
    submission_id: UUID,
    user: User = Depends(get_user_info),
    session: AsyncSession = Depends(get_session),
    k8s: CustomObjectsApi | None = Depends(get_k8s_custom_objects),
    *,
    background_task: BackgroundTasks,
    s3: S3Session = Depends(get_s3),
) -> Any:

    # Set name to be submission_id + random number five digits long
    name = f"deepreef-{submission_id}-{str(random.randint(10000, 99999))}"

    query = select(Submission).where(Submission.id == submission_id)
    if not user.is_admin:
        query = query.where(Submission.owner == user.id)
    submission_res = await session.exec(query)
    submission = submission_res.one_or_none()

    if not submission:
        raise HTTPException(status_code=404, detail="Submission not found")

    # Get the input object IDs from the submission
    res = await session.exec(
        select(InputObjectAssociations)
        .where(InputObjectAssociations.submission_id == submission_id)
        .order_by(InputObjectAssociations.processing_order)  # Important!
    )

    input_associations = res.all()
    input_object_ids = [
        association.input_object_id for association in input_associations
    ]

    # Timestamp logic remains the same
    if len(input_object_ids) == 1:
        timestamp = (
            f"{submission.time_seconds_start}-{submission.time_seconds_end}"
        )
    else:
        timestamp = (
            f"{submission.time_seconds_start}-end,"
            f"begin-{submission.time_seconds_end}"
        )

    if k8s:
        api_response = submit_job(
            k8s,
            name,
            submission.fps,
            timestamp,
            submission_id,
            input_object_ids,
        )
    else:
        raise HTTPException(
            status_code=500,
            detail="GPU unavailable",
        )

    background_task.add_task(
        iteratively_check_status,
        session,
        s3,
        submission_id,
        job_id=name,
    )
    return api_response


@router.get("", response_model=list[SubmissionRead])
async def get_submissions(
    response: Response,
    user: User = Depends(get_user_info),
    session: AsyncSession = Depends(get_session),
    s3: S3Session = Depends(get_s3),
    k8s: CoreV1Api | None = Depends(get_k8s_v1),
    *,
    filter: str = Query(None),
    sort: str = Query(None),
    range: str = Query(None),
) -> list[SubmissionRead]:
    """Get all submissions"""

    sort = json.loads(sort) if sort else []
    range = json.loads(range) if range else []
    filter = json.loads(filter) if filter else {}

    # Do a query to satisfy total count for "Content-Range" header
    count_query = select(func.count(Submission.iterator))
    if not user.is_admin:
        count_query = count_query.where(Submission.owner == user.id)

    if len(filter):  # Have to filter twice for some reason? SQLModel state?
        for field, value in filter.items():
            if field == "id" or field == "submission_id":
                if isinstance(value, list):
                    for v in value:
                        count_query = count_query.filter(
                            getattr(Submission, field) == v
                        )
                else:
                    count_query = count_query.filter(
                        getattr(Submission, field) == value
                    )
            else:
                count_query = count_query.filter(
                    getattr(Submission, field).like(f"%{str(value)}%")
                )
    total_count = await session.exec(count_query)
    total_count = total_count.one()

    # Query for the quantity of records in SensorInventoryData that match the
    # sensor as well as the min and max of the time column
    query = select(Submission)
    if not user.is_admin:
        query = query.where(Submission.owner == user.id)

    # Order by sort field params ie. ["name","ASC"]
    if len(sort) == 2:
        sort_field, sort_order = sort
        if sort_order == "ASC":
            query = query.order_by(getattr(Submission, sort_field))
        else:
            query = query.order_by(getattr(Submission, sort_field).desc())

    # Filter by filter field params ie. {"name":"bar"}
    if len(filter):
        for field, value in filter.items():
            if field == "id" or field == "submission_id":
                if isinstance(value, list):
                    for v in value:
                        count_query = count_query.filter(
                            getattr(Submission, field) == v
                        )
                else:
                    count_query = count_query.filter(
                        getattr(Submission, field) == value
                    )
            else:
                query = query.filter(
                    getattr(Submission, field).like(f"%{str(value)}%")
                )

    if len(range) == 2:
        start, end = range
        query = query.offset(start).limit(end - start + 1)
    else:
        start, end = [0, total_count]  # For content-range header

    # Execute query
    results = await session.exec(query)
    submissions = results.all()

    submissions = [
        SubmissionRead.model_validate(submission) for submission in submissions
    ]

    for submission in submissions:
        job_status = sorted(
            submission.run_status,
            key=lambda x: (
                x.time_started
                if x.time_started
                else "9999-00-00T00:00:00Z"  # noqa
            ),
            reverse=True,
        )
        submission.run_status = job_status

    response.headers["Content-Range"] = (
        f"submissions {start}-{end}/{total_count}"
    )

    return submissions


@router.post("", response_model=SubmissionRead)
async def create_submission(
    submission: SubmissionCreate,
    user: User = Depends(get_user_info),
    session: AsyncSession = Depends(get_session),
) -> SubmissionRead:
    """Creates a submission record from one or more video files"""

    if len(submission.input_associations) == 0:
        raise HTTPException(
            status_code=400,
            detail="At least one file must be provided",
        )

    # Check that the input associations belong to the transect
    # that the submission to be created is associated with
    for input_file in submission.input_associations:
        query = select(InputObject).where(
            InputObject.id == input_file.input_object_id
        )
        res = await session.exec(query)
        input_object_obj = res.one_or_none()
        if not input_object_obj:
            raise HTTPException(
                status_code=404,
                detail=f"Object not found ({input_file.input_object_id})",
            )
        if input_object_obj.transect_id != submission.transect_id:
            raise HTTPException(
                status_code=400,
                detail=(
                    "Input object does not belong to the submission's transect"
                ),
            )

    # Create submission object to get submission_id to link to input objects
    obj = Submission(
        name=submission.name,
        description=submission.description,
        transect_id=submission.transect_id,
        fps=(
            submission.fps if submission.fps else config.DEFAULT_SUBMISSION_FPS
        ),
        time_seconds_start=submission.time_seconds_start,
        time_seconds_end=submission.time_seconds_end,
        owner=user.id,
    )

    session.add(obj)
    await session.commit()
    await session.refresh(obj)

    # For each file in submission.inputs, query for the object in objects
    # table with the same ID.
    for input_file in submission.input_associations:
        # Gather all inputs first, to double check they exist before creation
        query = select(InputObject).where(
            InputObject.id == input_file.input_object_id
        )

        # Don't allow non-admins to create submissions with other users objects
        if not user.is_admin:
            query = query.where(InputObject.owner == user.id)

        res = await session.exec(query)

        input_object_obj = res.one_or_none()
        if not input_object_obj:
            raise HTTPException(
                status_code=404,
                detail=f"Object not found ({input_file.input_object_id})",
            )

        input_association_obj = InputObjectAssociations(
            input_object_id=input_object_obj.id,
            submission_id=obj.id,
            processing_order=input_file.processing_order,
        )
        session.add(input_association_obj)

        await session.commit()
        await session.refresh(input_association_obj)

    return obj


@router.put("/{submission_id}", response_model=Any)
async def update_submission(
    submission_id: UUID,
    submission_update: SubmissionUpdate,
    user: User = Depends(get_user_info),
    session: AsyncSession = Depends(get_session),
) -> SubmissionRead:
    """Update an submission by id"""

    query = select(Submission).where(Submission.id == submission_id)
    if not user.is_admin:
        query = query.where(Submission.owner == user.id)

    res = await session.exec(query)
    obj = res.one_or_none()

    if not obj:
        raise HTTPException(status_code=404, detail="Submission not found")

    # res.sqlmodel_update(obj, submission_update)
    submission_data = submission_update.model_dump(exclude_unset=True)

    # Update the fields from the request
    for field, value in submission_data.items():
        # Handle nested objects in inputs
        if field == "input_associations":
            for input_association in value:
                res_input = await session.exec(
                    select(InputObjectAssociations)
                    .where(
                        InputObjectAssociations.input_object_id
                        == input_association["input_object_id"]
                    )
                    .where(
                        InputObjectAssociations.submission_id == submission_id
                    )
                )
                input_obj = res_input.one_or_none()

                if not input_obj:
                    raise HTTPException(
                        status_code=404, detail="Input Object not found"
                    )

                for input_field, input_value in input_association.items():
                    # Only allow processing_order to be updated
                    if input_field == "processing_order":
                        setattr(input_obj, input_field, input_value)

                session.add(input_obj)
                await session.commit()
        else:
            setattr(obj, field, value)

    session.add(obj)
    await session.commit()
    await session.refresh(obj)

    return obj


@router.delete("/{submission_id}")
async def delete_submission(
    submission_id: UUID,
    user: User = Depends(get_user_info),
    session: AsyncSession = Depends(get_session),
    filter: dict[str, str] | None = None,
) -> None:
    """Delete an submission by id"""

    query = select(Submission).where(Submission.id == submission_id)
    if not user.is_admin:
        query = query.where(Submission.owner == user.id)

    res = await session.exec(query)
    submission = res.one_or_none()

    if not submission:
        raise HTTPException(status_code=404, detail="Submission not found")

    # Delete associations
    for association in submission.input_associations:
        await session.delete(association)
        await session.commit()
    if submission:
        await session.delete(submission)
        await session.commit()
