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
import gpxpy
import gpxpy.gpx
import csv

router = APIRouter()


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

    response.headers[
        "Content-Range"
    ] = f"submissions {start}-{end}/{total_count}"

    return data


# @router.get("", response_model=list[SubmissionReadWithDataSummary])
# async def get_submissions(
#     response: Response,
#     session: AsyncSession = Depends(get_session),
#     *,
#     filter: str = Query(None),
#     sort: str = Query(None),
#     range: str = Query(None),
# ):
#     """Get all submissions"""
#     sort = json.loads(sort) if sort else []
#     range = json.loads(range) if range else []
#     filter = json.loads(filter) if filter else {}

#     # Do a query to satisfy total count for "Content-Range" header
#     count_query = select(func.count(Submission.iterator))
#     if len(filter):  # Have to filter twice for some reason? SQLModel state?
#         for field, value in filter.items():
#             if field == "id" or field == "area_id":
#                 count_query = count_query.filter(
#                     getattr(Submission, field) == value
#                 )
#             else:
#                 count_query = count_query.filter(
#                     getattr(Submission, field).like(f"%{str(value)}%")
#                 )
#     total_count = await session.execute(count_query)
#     total_count = total_count.scalar_one()

#     # Query for the quantity of records in SubmissionData that match the submission as
#     # well as the min and max of the time column
#     query = (
#         select(
#             Submission,
#             func.count(SubmissionData.id).label("qty_records"),
#             func.min(SubmissionData.time).label("start_date"),
#             func.max(SubmissionData.time).label("end_date"),
#         )
#         .outerjoin(SubmissionData, Submission.id == SubmissionData.submission_id)
#         .group_by(
#             Submission.id,
#             Submission.geom,
#             Submission.name,
#             Submission.description,
#             Submission.iterator,
#         )
#     )

#     # Order by sort field params ie. ["name","ASC"]
#     if len(sort) == 2:
#         sort_field, sort_order = sort
#         if sort_order == "ASC":
#             query = query.order_by(getattr(Submission, sort_field))
#         else:
#             query = query.order_by(getattr(Submission, sort_field).desc())

#     # Filter by filter field params ie. {"name":"bar"}
#     if len(filter):
#         for field, value in filter.items():
#             if field == "id" or field == "area_id":
#                 query = query.filter(getattr(Submission, field) == value)
#             else:
#                 query = query.filter(
#                     getattr(Submission, field).like(f"%{str(value)}%")
#                 )

#     if len(range) == 2:
#         start, end = range
#         query = query.offset(start).limit(end - start + 1)
#     else:
#         start, end = [0, total_count]  # For content-range header

#     # Execute query
#     results = await session.execute(query)
#     submissions = results.all()
#     # print(submissions)

#     response.headers["Content-Range"] = f"submissions {start}-{end}/{total_count}"

#     # Add the summary information for the data (instead of the full data)
#     submissions_with_data = []
#     for row in submissions:
#         submissions_with_data.append(
#             SubmissionReadWithDataSummary(
#                 **row[0].dict(),
#                 data=SubmissionDataSummary(
#                     qty_records=row[1],
#                     start_date=row[2],
#                     end_date=row[3],
#                 ),
#             )
#         )

#     return submissions_with_data


@router.post("", response_model=SubmissionRead)
async def create_submission_from_gpx(
    payload: SubmissionCreate = Body(...),
    session: AsyncSession = Depends(get_session),
) -> None:
    """Creates a submission from a video file"""

    submission = Submission.from_orm(payload)

    session.add(submission)
    await session.commit()
    await session.refresh(submission)

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
