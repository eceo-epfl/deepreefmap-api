import botocore.exceptions
from app.config import config
from app.submissions.models import Submission
from app.submissions.status.models import RunStatus
from app.db import AsyncSession
from uuid import UUID
from aioboto3 import Session as S3Session
import botocore
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
) -> None:
    """Used as a background task to poll for the status of the submission run

    Initiated when a job submission is created. The function will continue to
    check the status of the submission run until it is "Completed" or "Failed".

    If the submission is "Running", the function will sleep for 5 seconds
    before checking the status again. It will also update the log.
    """

    print(f"Spooling up submission status check for {submission_id}")

    # Get Submission object
    res = await session.exec(
        select(Submission).where(Submission.id == submission_id)
    )

    submission = res.one_or_none()
    if not submission:
        print(f"Submission with ID {submission_id} not found.")
        return

    await asyncio.sleep(10)  # Sleep for 10 seconds to allow the job to start

    # Manage a deduped list of running jobs so we can update once they are done
    running_jobs_id = set()

    jobs = await get_cached_submission_jobs(submission.id)
    running_jobs = [
        job for job in jobs if job.status in ["Pending", "Running"]
    ]
    [running_jobs_id.add(job.job_id) for job in running_jobs]

    while running_jobs:
        print("Jobs:", jobs)
        print("Running Jobs:", running_jobs)
        print("Running Jobs ID:", running_jobs_id)
        # Log and update the status of still-running jobs
        for job in jobs:
            if job.job_id in running_jobs_id:
                query = await session.exec(
                    select(RunStatus).where(
                        RunStatus.kubernetes_pod_name == job.job_id
                    )
                )
                run_status = query.one_or_none()

                if not run_status:
                    run_status = RunStatus(
                        kubernetes_pod_name=job.job_id,
                        submission_id=submission.id,
                    )
                    session.add(run_status)
                    await session.commit()

                await session.exec(
                    update(RunStatus)
                    .where(RunStatus.kubernetes_pod_name == job.job_id)
                    .values(
                        status=job.status,
                        is_running=True,
                        is_successful=False,
                        is_still_kubernetes_resource=True,
                        time_started=job.time_started,
                        last_updated=datetime.datetime.now(),
                        logs=[],  # Avoiding this for now
                    )
                )
                await session.commit()

                print(
                    f"Submission ID: {submission_id}: Job {job.job_id} is running"
                )
        # Sleep before checking again
        await asyncio.sleep(config.SUBMISSION_JOB_CHECK_POLLING_INTERVAL)

        # Get the updated list of running jobs
        jobs = await get_cached_submission_jobs(submission.id)
        running_jobs = [job for job in jobs if job.status == "Running"]
        [running_jobs_id.add(job.job_id) for job in running_jobs]

    print("Loop finished")
    print("Jobs:", jobs)
    print("Running Jobs:", running_jobs)
    print("Running Jobs ID:", running_jobs_id)
    # Do a final update of all the jobs recorded in this task
    for job in jobs:
        await session.exec(
            update(RunStatus)
            .where(RunStatus.kubernetes_pod_name == job.job_id)
            .values(
                is_running=False,
                is_successful=job.status == "Succeeded",
                last_updated=datetime.datetime.now(),
                status=job.status,
            )
        )
        await session.commit()

    # Check the list of submission.run_status. If there are any there that
    # are are not in the jobs list, update them to
    # is_still_kubernetes_resource=False
    await session.refresh(submission)

    for run_status in submission.run_status:
        if run_status.kubernetes_pod_name not in running_jobs_id:
            await session.exec(
                update(RunStatus)
                .where(
                    RunStatus.kubernetes_pod_name
                    == run_status.kubernetes_pod_name
                )
                .values(
                    is_still_kubernetes_resource=False,
                    last_updated=datetime.datetime.now(),
                    status="Deleted",
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
