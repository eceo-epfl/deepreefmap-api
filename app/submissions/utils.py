from app.config import config
from app.submissions.models import Submission
from app.submissions.status.models import RunStatus
from app.db import AsyncSession
from uuid import UUID
from aioboto3 import Session as S3Session
from botocore.exceptions import ClientError
from sqlmodel import select, update
from app.submissions.k8s import get_cached_submission_jobs
import json
import datetime
import asyncio


async def populate_percentage_covers(
    submission_id: UUID,
    session: AsyncSession,
    s3: S3Session,
) -> Submission:
    """Populate the percentage_covers field of a Submission object with the
    data from the S3 bucket. This function is called after the processing
    of the submission is completed.
    """

    # Get Submission object
    query = select(Submission).where(Submission.id == submission_id)
    submission = await session.exec(query)
    submission = submission.one_or_none()

    try:
        if not submission:
            raise ValueError(f"Submission with ID {submission_id} not found.")
        # Get the two files from S3
        response = await s3.get_object(
            Bucket=config.S3_BUCKET_ID,
            Key=f"{config.S3_PREFIX}/outputs/{str(submission_id)}/"
            "percentage_covers.json",
        )
        # Get response body
        percentage_covers = await response["Body"].read()
        percentage_covers = json.loads(percentage_covers)

        # Do the same for the class_to_color.json file
        response = await s3.get_object(
            Bucket=config.S3_BUCKET_ID,
            Key=f"{config.S3_PREFIX}/outputs/{str(submission_id)}/"
            "class_to_color.json",
        )
        class_to_color = await response["Body"].read()
        class_to_color = json.loads(class_to_color)

        joined_covers = []
        for key, value in percentage_covers.items():
            joined_covers.append(
                {
                    "class": key,
                    "percentage_cover": value,
                    "color": class_to_color.get(key),
                }
            )
        # Add to DB
        update_query = (
            update(
                Submission,
            )
            .where(Submission.id == submission_id)
            .values(percentage_covers=joined_covers)
        )
        await session.exec(update_query)
        await session.commit()
    except ClientError:
        return submission

    except ValueError:
        return submission

    await session.refresh(submission)

    return submission


async def iteratively_check_status(
    session: AsyncSession,
    submission_id: UUID,
    job_id: str,
    timeout: int = config.SUBMISSION_JOB_CHECK_TIMEOUT,
) -> None:
    """Used as a background task to poll for the status of the submission run

    Initiated when a job submission is created. The function will continue to
    check the status of the submission run until it is "Completed" or "Failed".

    If the submission is "Running", the function will sleep for 5 seconds
    before checking the status again. It will also update the log.
    """

    print(
        f"Spooling up submission status check for {submission_id} - {job_id}"
    )
    job = await get_submission_job_by_name(submission_id, job_id)
    time_started = datetime.datetime.now()

    while job is None:
        print(f"Job {job_id} is not ready yet")
        await asyncio.sleep(config.SUBMISSION_JOB_CHECK_POLLING_INTERVAL)
        job = await get_submission_job_by_name(submission_id, job_id)
        if (datetime.datetime.now() - time_started).seconds > timeout:
            print(
                f"Submission {submission_id} has timed out waiting for update."
                f" Cancelling job {job_id}"
            )
            return

    # Create Runstatus object and add to DB
    run_status = RunStatus(
        kubernetes_pod_name=job_id,
        submission_id=submission_id,
        status="Pending",
        is_still_kubernetes_resource=True,  # This gets updated on status reqs
    )
    session.add(run_status)
    await session.commit()

    # Hold this for the final update in case it's deleted during the run
    last_status = None

    while job and job.status in ["Pending", "Running"]:
        print("In loop")
        print(f"Job: {job}")

        await session.exec(
            update(RunStatus)
            .where(RunStatus.kubernetes_pod_name == job_id)
            .values(
                status=job.status,
                is_running=True,
                is_successful=False,
                time_started=job.time_started,
                last_updated=datetime.datetime.now(),
            )
        )
        await session.commit()
        last_status = job.status

        print(f"Submission ID: {submission_id}: Job {job_id} is running")

        # Sleep before checking again
        await asyncio.sleep(config.SUBMISSION_JOB_CHECK_POLLING_INTERVAL)

        # Get the updated list of running jobs
        job = await get_submission_job_by_name(submission_id, job_id)

    print("Loop finished")

    # Do a final update of all the job
    is_k8s_resource = True if job is not None else False

    final_status = False
    if job and is_k8s_resource and job.status == "Succeeded":
        final_status = True

    await session.exec(
        update(RunStatus)
        .where(RunStatus.kubernetes_pod_name == job_id)
        .values(
            is_running=False,
            is_successful=final_status,
            is_still_kubernetes_resource=is_k8s_resource,
            last_updated=datetime.datetime.now(),
            status=job.status if is_k8s_resource else last_status,
        )
    )
    await session.commit()

    print(f"Submission ID: {submission_id} has finished running.")


def extract_submission_id_from_job_name(job_name: str) -> UUID:
    """Extract the submission ID from the job name

    The UUID after the first - is the UUID of the submission
    eg.2a17fbc5-27b9-4e3a-b7f2-0ae381c22fd7 in
       deepreef-2a17fbc5-27b9-4e3a-b7f2-0ae381c22fd7-82658-0-0
    """

    job_split = job_name.split("-")
    submission_uuid = "-".join(job_split[1:5])

    return UUID(submission_uuid)


async def get_submission_job_by_name(
    submission_id: UUID,
    job_id: str,
):
    jobs = await get_cached_submission_jobs(submission_id)
    for job in jobs:
        if job_id in job.job_id:  # The end may have -0-0, -0-1, etc, use "in"
            # Return the job
            return job

    return None
