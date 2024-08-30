import botocore.exceptions
from app.config import config
from app.submissions.models import Submission
from app.db import AsyncSession
from uuid import UUID
from aioboto3 import Session as S3Session
import botocore
from botocore.exceptions import ClientError
from sqlmodel import select, update
import json


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
