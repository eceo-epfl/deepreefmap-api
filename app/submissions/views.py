from fastapi import Depends, APIRouter, Query, Response, HTTPException, Header
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
from app.submissions.utils import populate_percentage_covers
from fastapi.responses import StreamingResponse
from app.objects.models import InputObject, InputObjectAssociations
from uuid import UUID
from sqlalchemy import func
import json
from typing import Any, Annotated
from app.objects.service import get_s3
from aioboto3 import Session as S3Session
from app.config import config
from kubernetes.client import CoreV1Api, ApiClient
from app.submissions.k8s import (
    get_k8s_v1,
    get_k8s_custom_objects,
    delete_job,
)
import random


router = APIRouter()


@router.get("/logs/{job_id}", response_model=SubmissionJobLogRead)
async def get_job_log(
    job_id: str,
    k8s: CoreV1Api = Depends(get_k8s_v1),
) -> SubmissionJobLogRead:
    """Get the log for the given submission job

    Clean out any carriage return lines and only return the last result of
    each line.
    """

    # Get the log from the job
    ret = k8s.read_namespaced_pod_log(
        name=str(job_id),
        namespace=config.NAMESPACE,
    )

    lines = ret.split("\n")
    cleaned_lines = []
    for line in lines:
        progress_lines = line.split("\r")  # Remove carriage returns
        cleaned_lines.append(progress_lines[-1])  # Only keep the last line

    return SubmissionJobLogRead(id=job_id, message="\n".join(cleaned_lines))


@router.get("/kubernetes/jobs")
async def get_jobs(
    k8s: CoreV1Api = Depends(get_k8s_v1),
) -> Any:
    """Get all kubernetes jobs in the namespace"""

    ret = k8s.list_namespaced_pod(config.NAMESPACE)

    api = ApiClient()
    return api.sanitize_for_serialization(ret.items)


@router.delete("/kubernetes/jobs/{job_id}")
async def delete_runai_job(
    job_id: str,
) -> Any:

    # Remove the two digits at the end of the job ID to get the submission ID
    # For example deepreef-463230f2-82c7-439c-831b-ef8c0b201ee1-56966-0-0
    # should be deepreef-463230f2-82c7-439c-831b-ef8c0b201ee1-56966
    job_id = "-".join(job_id.split("-")[:-2])
    print(job_id)

    delete_job(job_id)


@router.get("/{submission_id}", response_model=SubmissionRead)
async def get_submission(
    session: AsyncSession = Depends(get_session),
    s3: S3Session = Depends(get_s3),
    k8s: CoreV1Api = Depends(get_k8s_v1),
    user_id: UUID = Header(...),
    user_is_admin: bool = Header(...),
    *,
    submission_id: UUID,
) -> SubmissionRead:
    """Get an submission by id"""

    query = select(Submission).where(Submission.id == submission_id)
    if not user_is_admin:
        query = query.where(Submission.owner == user_id)

    res = await session.exec(query)
    submission = res.one_or_none()

    if not submission:
        raise HTTPException(status_code=404, detail="Submission not found")

    # Get all jobs from k8s then filter out the ones that belong to the
    # submission_id

    jobs = k8s.list_namespaced_pod(config.NAMESPACE)
    jobs = jobs.items
    jobs = [job for job in jobs if str(submission_id) in job.metadata.name]
    job_status = []
    for job in jobs:
        api = ApiClient()
        job_data = api.sanitize_for_serialization(job)
        job_status.append(
            KubernetesExecutionStatus(
                submission_id=job_data["metadata"].get("name"),
                status=job_data["status"].get("phase"),
                time_started=job_data["status"].get("startTime"),
            )
        )

    # Get the file outputs in the S3 bucket
    response = await s3.list_objects_v2(
        Bucket=config.S3_BUCKET_ID,
        Prefix=f"{config.S3_PREFIX}/outputs/{str(submission_id)}/",
    )
    outputs = response.get("Contents", [])

    output_files = []
    for output in outputs:
        output_files.append(
            SubmissionFileOutputs(
                filename=output["Key"].split("/")[-1],
                size_bytes=output["Size"],
                last_modified=output["LastModified"],
                url=(
                    f"/api/submissions/{submission_id}/"
                    f"{output['Key'].split('/')[-1]}"
                ),
            )
        )

    # Sort the job_status by time_started, put any None or empty strings to
    # beginning of list
    job_status = sorted(
        job_status,
        key=lambda x: (
            x.time_started if x.time_started else "9999-00-00T00:00:00Z"
        ),
        reverse=True,
    )

    model_obj = SubmissionRead.model_validate(submission)
    model_obj.run_status = job_status
    model_obj.file_outputs = output_files

    return model_obj


@router.get("/{submission_id}/{filename}", response_class=StreamingResponse)
async def get_submission_output_file(
    session: AsyncSession = Depends(get_session),
    s3: S3Session = Depends(get_s3),
    *,
    submission_id: UUID,
    filename: str,
) -> StreamingResponse:
    """With the given submission ID and filename, returns the file from S3"""

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
    return StreamingResponse(content=response["Body"].iter_chunks())


@router.post("/{submission_id}/execute", response_model=Any)
async def execute_submission(
    submission_id: UUID,
    user_id: UUID = Header(...),
    user_is_admin: bool = Header(...),
    session: AsyncSession = Depends(get_session),
    s3: S3Session = Depends(get_s3),
    k8s: CoreV1Api = Depends(get_k8s_custom_objects),
) -> Any:

    # Set name to be submission_id + random number five digits long
    name = f"deepreef-{submission_id}-{str(random.randint(10000, 99999))}"

    query = select(Submission).where(Submission.id == submission_id)
    if not user_is_admin:
        query = query.where(Submission.owner == user_id)
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
        str(association.input_object_id) for association in input_associations
    ]  # In order due to query above

    # Timestamp variable should be the combination of time_seconds_start and
    # time_seconds_end. If there is only 1 file (input_object_ids), then use
    # the format is `f"{time_seconds_start}-{time_seconds_end}"`. If there are
    # multiple files, then the format is
    # `f"{time_seconds_start}-end,begin-{time_seconds_end}"``
    if len(input_object_ids) == 1:
        timestamp = (
            f"{submission.time_seconds_start}-{submission.time_seconds_end}"
        )
    else:
        timestamp = (
            f"{submission.time_seconds_start}-end,"
            f"begin-{submission.time_seconds_end}"
        )

    job = {
        "apiVersion": "run.ai/v1",
        "kind": "RunaiJob",
        "metadata": {
            "name": name,
            "namespace": config.NAMESPACE,
            "labels": {"user": "ejthomas", "release": name},
        },
        "spec": {
            "template": {
                "metadata": {
                    "labels": {
                        "user": "ejthomas",
                        "release": name,
                    }
                },
                "spec": {
                    "schedulerName": "runai-scheduler",
                    "restartPolicy": "Never",
                    "securityContext": {
                        "runAsUser": 1000,
                        "runAsGroup": 1000,
                        "fsGroup": 1000,
                    },
                    "containers": [
                        {
                            "name": "rcp-test-ejthomas",
                            "image": (
                                f"{config.DEEPREEFMAP_IMAGE}:"
                                f"{config.DEEPREEFMAP_IMAGE_TAG}"
                            ),  # noqa
                            "env": [
                                {
                                    "name": "S3_URL",
                                    "value": config.S3_URL,
                                },
                                {
                                    "name": "S3_BUCKET_ID",
                                    "value": config.S3_BUCKET_ID,
                                },
                                {
                                    "name": "S3_ACCESS_KEY",
                                    "value": config.S3_ACCESS_KEY,
                                },
                                {
                                    "name": "S3_SECRET_KEY",
                                    "value": config.S3_SECRET_KEY,
                                },
                                {
                                    "name": "S3_PREFIX",
                                    "value": config.S3_PREFIX,
                                },
                                {  # This is an env var, so list dumped as str
                                    "name": "INPUT_OBJECT_IDS",
                                    "value": json.dumps(input_object_ids),
                                },
                                {
                                    "name": "SUBMISSION_ID",
                                    "value": str(submission_id),
                                },
                                {
                                    "name": "TIMESTAMP",
                                    "value": timestamp,
                                },
                                {
                                    "name": "FPS",
                                    "value": str(submission.fps),
                                },
                            ],
                            "resources": {"limits": {"nvidia.com/gpu": 1}},
                        }
                    ],
                },
            }
        },
    }

    # Create the job
    api_response = k8s.create_namespaced_custom_object(
        namespace=config.NAMESPACE,
        plural="runaijobs",
        body=job,
        version="v1",
        group="run.ai",
    )

    return api_response


@router.get("", response_model=list[SubmissionRead])
async def get_submissions(
    response: Response,
    user_id: UUID = Header(...),
    user_is_admin: bool = Header(...),
    session: AsyncSession = Depends(get_session),
    s3: S3Session = Depends(get_s3),
    k8s: CoreV1Api = Depends(get_k8s_v1),
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
    if not user_is_admin:
        count_query = count_query.where(Submission.owner == user_id.hex)

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
    if not user_is_admin:
        query = query.where(Submission.owner == user_id.hex)

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
    # Get all jobs from k8s then filter out the ones that belong to the
    # submission_id

    jobs = k8s.list_namespaced_pod(config.NAMESPACE)
    jobs = jobs.items
    for submission in submissions:
        submission_jobs = [
            job for job in jobs if str(submission.id) in job.metadata.name
        ]
        job_status = []
        for job in submission_jobs:
            api = ApiClient()
            job_data = api.sanitize_for_serialization(job)
            job_status.append(
                KubernetesExecutionStatus(
                    submission_id=job_data["metadata"].get("name"),
                    status=job_data["status"].get("phase"),
                    time_started=job_data["status"].get("startTime"),
                )
            )
        job_status = sorted(
            job_status,
            key=lambda x: (
                x.time_started if x.time_started else "9999-00-00T00:00:00Z"
            ),
            reverse=True,
        )
        submission.run_status = job_status

        # If the latest job status is "Succeeded", and there is no data for
        # percentage_covers in the DB, then populate the percentage_covers
        # field with the data from the S3 bucket. This is done in case
        # an export is req'd in the frontend before the processing is
        # completed (which is mostly done when a single submission is viewed).
        if job_status and (job_status[0].status == "Succeeded"):
            submission = await populate_percentage_covers(
                submission_id=submission.id,
                session=session,
                s3=s3,
            )

    response.headers["Content-Range"] = (
        f"submissions {start}-{end}/{total_count}"
    )

    return submissions


@router.post("", response_model=SubmissionRead)
async def create_submission(
    submission: SubmissionCreate,
    user_id: UUID = Header(...),
    user_is_admin: bool = Header(...),
    session: AsyncSession = Depends(get_session),
) -> SubmissionRead:
    """Creates a submission record from one or more video files"""

    if len(submission.input_associations) == 0:
        raise HTTPException(
            status_code=400,
            detail="At least one file must be provided",
        )

    # Create submission object to get submission_id to link to input objects
    obj = Submission(
        name=submission.name,
        description=submission.description,
        transect_id=submission.transect_id,
        fps=config.DEFAULT_SUBMISSION_FPS,
        owner=user_id,
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
        if not user_is_admin:
            query = query.where(InputObject.owner == user_id)

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
    user_id: UUID = Header(...),
    user_is_admin: bool = Header(...),
    session: AsyncSession = Depends(get_session),
) -> SubmissionRead:
    """Update an submission by id"""

    query = select(Submission).where(Submission.id == submission_id)
    if not user_is_admin:
        query = query.where(Submission.owner == user_id)

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
                        print(
                            f"Updating Field: inputs.{input_field}"
                            f" Value: {input_value}"
                        )

                session.add(input_obj)
                await session.commit()
        else:
            print(f"Updating: {field}, {value}")
            setattr(obj, field, value)

    session.add(obj)
    await session.commit()
    await session.refresh(obj)

    return obj


@router.delete("/{submission_id}")
async def delete_submission(
    submission_id: UUID,
    user_id: UUID = Header(...),
    user_is_admin: bool = Header(...),
    session: AsyncSession = Depends(get_session),
    filter: dict[str, str] | None = None,
) -> None:
    """Delete an submission by id"""

    query = select(Submission).where(Submission.id == submission_id)
    if not user_is_admin:
        query = query.where(Submission.owner == user_id)

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
