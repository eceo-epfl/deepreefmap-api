from app.config import config
import boto3
from typing import Any

s3 = boto3.client(
    "s3",
    aws_access_key_id=config.S3_ACCESS_KEY,
    aws_secret_access_key=config.S3_SECRET_KEY,
    endpoint_url=f"https://{config.S3_URL}",
)


def get_s3() -> boto3.client:
    yield s3


def get_s3_submission_inputs(submission_id) -> Any:
    """List all objects in the S3 bucket"""
    prefix = f"{config.S3_PREFIX}/{submission_id}/inputs"

    objects = s3.list_objects(
        Bucket=config.S3_BUCKET_ID,
        Prefix=prefix,
    ).get("Contents", None)

    return objects
