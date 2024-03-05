from fastapi import Depends, APIRouter, Query, Response, HTTPException, Body
from sqlmodel import select
from app.db import get_session, AsyncSession
from app.submissions.models import (
    Submission,
    SubmissionRead,
    SubmissionUpdate,
)
from uuid import UUID
from sqlalchemy import func
import json
from typing import Any
import boto3
from app.object_store.service import get_s3, S3Connection
from app.config import config
from fastapi import File, UploadFile
from kubernetes import client, config as k8s_config

router = APIRouter()


@router.get("/kubernetes/jobs")
async def get_jobs(
    response: Response,
    *,
    filter: str = Query(None),
    sort: str = Query(None),
    range: str = Query(None),
) -> Any:
    """Get all kubernetes jobs in the namespace"""

    k8s_config.load_kube_config()

    v1 = client.CoreV1Api()
    # print("Listing pods with their IPs:")
    ret = v1.list_namespaced_pod(config.NAMESPACE)

    return ret.items


@router.get("/{submission_id}", response_model=SubmissionRead)
async def get_submission(
    session: AsyncSession = Depends(get_session),
    s3: S3Connection = Depends(get_s3),
    *,
    submission_id: UUID,
) -> SubmissionRead:
    """Get an submission by id"""

    query = select(Submission).where(Submission.id == submission_id)
    res = await session.exec(query)
    submission = res.one_or_none()
    submission_dict = submission.dict() if submission else {}

    s3_objects = s3.get_s3_submission_inputs(submission_dict.get("id"))
    submission_dict["inputs"] = s3_objects
    print(submission_dict)
    res = await session.exec(query)
    return SubmissionRead(**submission_dict)


@router.get("", response_model=list[SubmissionRead])
async def get_submissions(
    response: Response,
    session: AsyncSession = Depends(get_session),
    s3: S3Connection = Depends(get_s3),
    *,
    include_inputs: bool = Query(True),
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

    submission_objs = [SubmissionRead.model_validate(x) for x in submissions]

    if include_inputs:
        # Reluctantly doing this for usability in the list, but it has
        # potential to be slow if the list is large. Want to avoid adding S3
        # metadata to the DB if possible to avoid sync issues.
        for submission in submission_objs:
            s3_objects = s3.get_s3_submission_inputs(submission.id)
            submission.inputs = s3_objects

    response.headers["Content-Range"] = (
        f"submissions {start}-{end}/{total_count}"
    )

    return submission_objs


@router.post("", response_model=SubmissionRead)
async def create_submission(
    files: list[UploadFile] = File(...),
    session: AsyncSession = Depends(get_session),
    s3: boto3.client = Depends(get_s3),
) -> None:
    """Creates a submission record from one or more video files"""

    if len(files) == 0:
        raise HTTPException(
            status_code=400,
            detail="At least one file must be provided",
        )
    # Check that there are distinct filenames in the list of files
    filenames = [file.filename for file in files]
    if len(filenames) != len(set(filenames)):
        raise HTTPException(
            status_code=400,
            detail="All files must have distinct filenames",
        )

    # Create new submission DB object with no fields
    submission = Submission()

    session.add(submission)
    await session.commit()
    await session.refresh(submission)

    # Use the generated DB submission ID to create a prefix for the S3 bucket
    prefix = f"{config.S3_PREFIX}/{submission.id}/inputs"
    try:
        for file_obj in files:
            content = await file_obj.read()

            # Write bytes to S3 bucket
            response = s3.session.put_object(
                Bucket=config.S3_BUCKET_ID,
                Key=f"{prefix}/{file_obj.filename}",
                Body=content,
            )

            # Validate that response is 200: OK
            if response["ResponseMetadata"]["HTTPStatusCode"] != 200:
                # Error in uploading, raise exception
                raise Exception(
                    "Failed to upload file to S3: "
                    f"{response['ResponseMetadata']}"
                )

    except Exception as e:
        await session.delete(submission)
        await session.commit()
        s3.session.delete_objects(
            Bucket=config.S3_BUCKET_ID,
            Delete={"Objects": [{"Key": f"{prefix}/{file_obj.filename}"}]},
        )
        raise HTTPException(
            status_code=500,
            detail=f"Failed to upload file to S3: {e}",
        )

    return submission


@router.put("/{submission_id}", response_model=SubmissionRead)
async def update_submission(
    submission_id: UUID,
    submission_update: SubmissionUpdate,
    session: AsyncSession = Depends(get_session),
) -> SubmissionRead:
    res = await session.execute(
        select(Submission).where(Submission.id == submission_id)
    )
    submission_db = res.scalars().one()
    submission_data = submission_update.model_dump(exclude_unset=True)
    if not submission_db:
        raise HTTPException(status_code=404, detail="Submission not found")

    session.add(submission_db)
    await session.commit()
    await session.refresh(submission_db)

    return submission_db


@router.delete("/{submission_id}")
async def delete_submission(
    submission_id: UUID,
    session: AsyncSession = Depends(get_session),
    filter: dict[str, str] | None = None,
) -> None:
    """Delete an submission by id"""
    res = await session.execute(
        select(Submission).where(Submission.id == submission_id)
    )
    submission = res.scalars().one_or_none()

    if submission:
        await session.delete(submission)
        await session.commit()
