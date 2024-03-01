from fastapi import Depends, APIRouter, Query, Response, HTTPException, Body
from sqlmodel import select
from app.db import get_session, AsyncSession
from app.utils import decode_base64
from app.deepreef.submissions.models import (
    Submission,
    SubmissionRead,
    SubmissionUpdate,
    SubmissionCreate,
)
from uuid import UUID
from sqlalchemy import func
from datetime import timezone
import json
import csv
from typing import Any
import boto3
from app.s3 import get_s3
from app.config import config
from fastapi import File, UploadFile

router = APIRouter()


@router.get("/s3", response_model=Any)
async def list_s3_objects(
    s3: boto3.client = Depends(get_s3),
    prefix: str = Query(None),
) -> Any:
    """List all objects in the S3 bucket"""

    # Always prefix the S3 bucket with the S3_PREFIX plus extra query prefix,
    # this is typically going to be inputs or outputs
    prefix = f"{config.S3_PREFIX}/{prefix if prefix else ''}"

    objects = s3.list_objects(
        Bucket=config.S3_BUCKET_ID,
        Prefix=prefix if prefix is not None else "",
    ).get("Contents", None)

    return objects


@router.put("/s3", response_model=Any)
async def upload_output_object(
    s3: boto3.client = Depends(get_s3),
    files: list[UploadFile] = File(...),
    # job_id: UUID = Query(...),
) -> Any:
    """Upload a file to the S3 bucket"""

    # Always prefix the S3 bucket with the S3_PREFIX plus extra query prefix,
    # this is typically going to be input or output for example:
    # deepreefmap-dev/<job-uuid>/inputs
    prefix = f"{config.S3_PREFIX}/generic_upload/output"

    for file_obj in files:
        print(file_obj.filename, file_obj.size)
        content = await file_obj.read()
        print(content)

        # Write bytes to S3 bucket
        response = s3.put_object(
            Bucket=config.S3_BUCKET_ID,
            Key=f"{prefix}/{file_obj.filename}",
            Body=content,
        )

        print("Response", response)


@router.get("/{submission_id}", response_model=SubmissionRead)
async def get_submission(
    session: AsyncSession = Depends(get_session),
    *,
    submission_id: UUID,
) -> SubmissionRead:
    """Get an submission by id"""

    query = select(Submission).where(Submission.id == submission_id)
    res = await session.execute(query)
    submission = res.one_or_none()
    submission_dict = submission[0].dict() if submission else {}

    res = await session.execute(query)

    return SubmissionRead(**submission_dict)


@router.get("", response_model=list[SubmissionRead])
async def get_submissions(
    response: Response,
    session: AsyncSession = Depends(get_session),
    *,
    filter: str = Query(None),
    sort: str = Query(None),
    range: str = Query(None),
) -> list[SubmissionRead]:
    """Get all submissions (mock data until we define requirements)"""

    sort = json.loads(sort) if sort else []
    range = json.loads(range) if range else []
    filter = json.loads(filter) if filter else {}

    # Make a fake list of 6 x SubmissionRead with realistic values
    data = [
        SubmissionRead(
            id=UUID("00000000-0000-0000-0000-000000000000"),
            geom={
                "type": "Point",
                "coordinates": [0.0, 0.0],
            },
            name="Submission 1",
            description="Rhône River",
            processing_finished=True,
            processing_successful=True,
            data_size_mb=2150.88,
            duration_seconds=180,
            submitted_at_utc="2024-01-04T08:33:21+00:00",
            submitted_by="ejthomas",
        ),
        SubmissionRead(
            id=UUID("00000000-0000-0000-0000-000000000001"),
            geom={
                "type": "Point",
                "coordinates": [0.0, 0.0],
            },
            name="Submission 2",
            description="Vispa River",
            processing_finished=True,
            processing_successful=False,
            data_size_mb=3800.40,
            duration_seconds=308,
            submitted_at_utc="2024-01-06T16:12:09+00:00",
            submitted_by="ejthomas",
        ),
    ]

    total_count = len(data)

    # Order by sort field params ie. ["name","ASC"]
    if len(sort) == 2:
        sort_field, sort_order = sort
        if sort_order == "ASC":
            data = sorted(data, key=lambda k: getattr(k, sort_field))
        else:
            data = sorted(
                data, key=lambda k: getattr(k, sort_field), reverse=True
            )

    # Filter by filter field params ie. {"name":"bar"}

    if len(range) == 2:
        start, end = range
        data = data[start:end]
    else:
        start, end = [0, total_count]  # For content-range header

    response.headers["Content-Range"] = (
        f"submissions {start}-{end}/{total_count}"
    )

    return data


@router.post("", response_model=SubmissionRead)
async def create_submission(
    # payload: SubmissionCreate = Body(...),
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

    for file_obj in files:
        content = await file_obj.read()

        # Write bytes to S3 bucket
        response = s3.put_object(
            Bucket=config.S3_BUCKET_ID,
            Key=f"{prefix}/{file_obj.filename}",
            Body=content,
        )

        # Validate that response is 200: OK
        if response["ResponseMetadata"]["HTTPStatusCode"] != 200:
            # Delete the submission if the S3 upload fails
            await session.delete(submission)
            await session.commit()
            # Delete any S3 objects with the prefix
            s3.delete_objects(
                Bucket=config.S3_BUCKET_ID,
                Delete={"Objects": [{"Key": f"{prefix}/{file_obj.filename}"}]},
            )
            raise HTTPException(
                status_code=500,
                detail="Failed to upload file to S3",
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
    submission_data = submission_update.dict(exclude_unset=True)
    if not submission_db:
        raise HTTPException(status_code=404, detail="Submission not found")

    # Update the fields from the request
    for field, value in submission_data.items():
        if field in ["latitude", "longitude"]:
            # Don't process lat/lon, it's converted to geom in model validator
            continue
        if field == "video":
            # Convert base64 to bytes, input should be csv, read and add rows
            # to submission_data table with submission_id
            rawdata, dtype = decode_base64(value)

            if dtype != "csv":
                raise HTTPException(
                    status_code=400,
                    detail="Only CSV files are supported",
                )
            # Treat the rawdata as a CSV file, read in the rows
            decoded = []
            for row in csv.reader(rawdata.decode("utf-8").splitlines()):
                decoded.append(row)

        print(f"Updating: {field}, {value}")
        setattr(submission_db, field, value)

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
