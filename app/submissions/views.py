from fastapi import Depends, APIRouter, Query, Response, HTTPException, Body
from sqlmodel import select
from app.db import get_session, AsyncSession
from app.submissions.models import (
    Submission,
    SubmissionRead,
    SubmissionUpdate,
    SubmissionCreate,
    KubernetesExecutionStatus,
)
from app.objects.models import InputObject
from uuid import UUID
from sqlalchemy import func
import json
from typing import Any
import boto3
from app.objects.service import get_s3, S3Connection
from app.config import config
from kubernetes.client import CoreV1Api, ApiClient
from app.submissions.k8s import get_k8s_v1, get_k8s_custom_objects
import random

router = APIRouter()


@router.get("/kubernetes/jobs")
async def get_jobs(
    k8s: CoreV1Api = Depends(get_k8s_v1),
) -> Any:
    """Get all kubernetes jobs in the namespace"""

    ret = k8s.list_namespaced_pod(config.NAMESPACE)

    api = ApiClient()
    return api.sanitize_for_serialization(ret.items)


@router.get("/{submission_id}", response_model=SubmissionRead)
async def get_submission(
    session: AsyncSession = Depends(get_session),
    s3: S3Connection = Depends(get_s3),
    k8s: CoreV1Api = Depends(get_k8s_v1),
    *,
    submission_id: UUID,
) -> SubmissionRead:
    """Get an submission by id"""

    query = select(Submission).where(Submission.id == submission_id)
    res = await session.exec(query)
    submission = res.one_or_none()

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
                submission_id=job_data["metadata"]["name"],
                status=job_data["status"]["phase"],
                time_started=job_data["status"]["startTime"],
            )
        )
    model_obj = SubmissionRead.model_validate(submission)
    model_obj.run_status = job_status

    return model_obj


@router.post("/{submission_id}/execute", response_model=Any)
async def execute_submission(
    submission_id: UUID,
    session: AsyncSession = Depends(get_session),
    s3: S3Connection = Depends(get_s3),
    k8s: CoreV1Api = Depends(get_k8s_custom_objects),
) -> Any:

    # Set name to be submission_id + random number five digits long
    name = f"{submission_id}-{str(random.randint(10000, 99999))}"

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
                    "labels": {"user": "ejthomas", "release": name},
                },
                "spec": {
                    "schedulerName": "runai-scheduler",
                    "restartPolicy": "Never",
                    "securityContext": {
                        "runAsUser": 266488,
                        "runAsGroup": 12500,
                        "fsGroup": 12500,
                    },
                    "containers": [
                        {
                            "name": "rcp-test-ejthomas",
                            "image": "registry.rcp.epfl.ch/rcp-test-ejthomas/rcp-test:latest",
                            "workingDir": "/app",
                            "resources": {"limits": {"nvidia.com/gpu": 1}},
                        }
                    ],
                },
            },
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
    print("Job issued")
    print(api_response)
    return api_response


@router.get("", response_model=list[SubmissionRead])
async def get_submissions(
    response: Response,
    session: AsyncSession = Depends(get_session),
    s3: S3Connection = Depends(get_s3),
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

    response.headers["Content-Range"] = (
        f"submissions {start}-{end}/{total_count}"
    )

    return submissions


@router.post("", response_model=SubmissionRead)
async def create_submission(
    submission: SubmissionCreate,
    session: AsyncSession = Depends(get_session),
    s3: boto3.client = Depends(get_s3),
) -> None:
    """Creates a submission record from one or more video files"""

    if len(submission.inputs) == 0:
        raise HTTPException(
            status_code=400,
            detail="At least one file must be provided",
        )
    inputs = []
    # For each file in submission.inputs, query for the object in objects
    # table with the same ID.
    for input_file in submission.inputs:
        res = await session.exec(
            select(InputObject).where(InputObject.id == input_file)
        )
        object_db = res.one_or_none()
        if not object_db:
            raise HTTPException(
                status_code=404,
                detail="Input object not found",
            )
        inputs.append(object_db)

    # Create new submission DB object with no fields
    submission = Submission(inputs=inputs)

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

    res = await session.exec(
        select(Submission).where(Submission.id == submission_id)
    )
    obj = res.one_or_none()
    if not obj:
        raise HTTPException(status_code=404, detail="Submission not found")

    submission_data = submission_update.model_dump(exclude_unset=True)

    # Update the fields from the request
    for field, value in submission_data.items():
        print(f"Updating: {field}, {value}")
        setattr(obj, field, value)

    session.add(obj)
    await session.commit()
    await session.refresh(obj)

    return obj


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
