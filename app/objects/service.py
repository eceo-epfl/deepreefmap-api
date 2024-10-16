from app.config import config
import aioboto3
from typing import Any, AsyncGenerator
from app.objects.models import S3Object
from app.status.models import StatusRead, S3Status
from cashews import cache


async def get_s3() -> AsyncGenerator[aioboto3.Session, None]:
    session = aioboto3.Session()
    async with session.client(
        "s3",
        aws_access_key_id=config.S3_ACCESS_KEY,
        aws_secret_access_key=config.S3_SECRET_KEY,
        endpoint_url=f"https://{config.S3_URL}",
    ) as client:
        yield client


@cache.early(ttl="30s", early_ttl="10s", key="s3:status")
async def get_s3_status():
    s3_status = False
    s3_local = S3Status()

    session = aioboto3.Session()
    async with session.client(
        "s3",
        aws_access_key_id=config.S3_ACCESS_KEY,
        aws_secret_access_key=config.S3_SECRET_KEY,
        endpoint_url=f"https://{config.S3_URL}",
    ) as s3:
        print("Fetching S3 status...")
        # Get total usage (items, size) for inputs, outputs
        s3_local = S3Status()

        try:
            # Get input usage (items, size) for all input objects
            response = await s3.list_objects_v2(
                Bucket=config.S3_BUCKET_ID,
                Prefix=f"{config.S3_PREFIX}/inputs/",
            )

            if response.get("Contents"):
                s3_local.input_object_count = len(response.get("Contents"))
                for obj in response.get("Contents"):
                    s3_local.input_size += obj.get("Size")

            # Get total usage (items, size) for outputs
            response = await s3.list_objects_v2(
                Bucket=config.S3_BUCKET_ID,
                Prefix=f"{config.S3_PREFIX}/outputs/",
            )
            if response.get("Contents"):
                s3_local.output_object_count = len(response.get("Contents"))
                for obj in response.get("Contents"):
                    s3_local.output_size += obj.get("Size")

            # Get total usage (items, size) for all objects
            response = await s3.list_objects_v2(
                Bucket=config.S3_BUCKET_ID,
                Prefix=f"{config.S3_PREFIX}/",
            )
            if response.get("Contents"):
                s3_local.total_object_count = len(response.get("Contents"))
                for obj in response.get("Contents"):
                    s3_local.total_size += obj.get("Size")
            s3_status = True

        except Exception:
            s3_local = None

    return s3_local, s3_status
