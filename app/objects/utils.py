from uuid import UUID
import cv2
import aioboto3
from app.config import config
import os
from app.db import AsyncSession
from sqlmodel import update, select, delete
from app.objects.models import InputObject
import datetime
import hashlib
import asyncio
import math


async def generate_video_statistics(
    input_object_id: UUID,
    s3: aioboto3.Session,
    db: AsyncSession,
) -> float:
    # Background task to generate statistics on uploaded video files

    # Get video file from S3 with the given input_object_id
    filename = f"{input_object_id}.mp4"
    try:
        update_query = (
            update(InputObject)
            .where(InputObject.id == input_object_id)
            .values(
                processing_has_started=True,
                processing_message=(
                    f"Started processing at {datetime.datetime.now()} (UTC)"
                ),
            )
        )
        await db.exec(update_query)
        await db.commit()

        response = await s3.get_object(
            Bucket=config.S3_BUCKET_ID,
            Key=f"{config.S3_PREFIX}/inputs/{input_object_id}",
        )
        video_file = await response["Body"].read()

        # Save to temporary file

        with open(filename, "wb") as f:
            f.write(video_file)

        # Generate FPS, duration
        cap = cv2.VideoCapture(filename)
        fps = round(cap.get(cv2.CAP_PROP_FPS), 2)
        frame_count = int(cap.get(cv2.CAP_PROP_FRAME_COUNT))
        duration_seconds = round(frame_count / fps, 2)
        cap.release()

        # Generate MD5 hash
        hash_md5sum = hashlib.md5(video_file).hexdigest()

        os.remove(filename)

        update_query = (
            update(InputObject)
            .where(InputObject.id == input_object_id)
            .values(
                fps=fps,
                time_seconds=duration_seconds,
                processing_completed_successfully=True,
                hash_md5sum=hash_md5sum,
                frame_count=frame_count,
                all_parts_received=True,
                processing_message=(
                    "Video statistics generated successfully at "
                    f"{datetime.datetime.now()} (UTC)"
                ),
            )
        )
        await db.exec(update_query)
        await db.commit()
    except Exception as e:
        update_query = (
            update(InputObject)
            .where(InputObject.id == input_object_id)
            .values(
                processing_completed_successfully=False,
                processing_message=(
                    f"Error generating video statistics: {str(e)}"
                ),
            )
        )
        await db.exec(update_query)
        await db.commit()

    return


async def delete_incomplete_object(
    input_object_id: UUID,
    s3: aioboto3.Session,
    db: AsyncSession,
) -> float:
    # Monitor the time of the last updated part and delete if under threshold

    query = select(InputObject).where(InputObject.id == input_object_id)
    res = await db.exec(query)
    obj = res.one_or_none()

    time_since_last_part = (
        datetime.datetime.now() - obj.last_part_received_utc
    ).total_seconds()

    while time_since_last_part < config.INCOMPLETE_OBJECT_TIMEOUT_SECONDS:
        if obj.all_parts_received:
            return

        update_query = (
            update(InputObject)
            .where(InputObject.id == input_object_id)
            .values(
                processing_message=(
                    "Waiting for parts..."
                    "Last part transferred "
                    f"{math.ceil(time_since_last_part)} seconds ago. "
                ),
            )
        )
        await db.exec(update_query)
        await db.commit()

        # Recheck the object
        await asyncio.sleep(config.INCOMPLETE_OBJECT_CHECK_INTERVAL)
        await db.refresh(obj)

        time_since_last_part = (
            datetime.datetime.now() - obj.last_part_received_utc
        ).total_seconds()

    # Delete the object from S3
    await s3.delete_object(
        Bucket=config.S3_BUCKET_ID,
        Key=f"{config.S3_PREFIX}/inputs/{input_object_id}",
    )

    # Delete chunked object from S3
    await s3.abort_multipart_upload(
        Bucket=config.S3_BUCKET_ID,
        Key=f"{config.S3_PREFIX}/inputs/{obj.id}",
        UploadId=obj.upload_id,
    )

    # Delete the object from the database
    delete_query = delete(InputObject).where(InputObject.id == input_object_id)
    await db.exec(delete_query)
    await db.commit()
