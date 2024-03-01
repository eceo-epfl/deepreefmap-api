from app.config import config
import boto3

s3 = boto3.client(
    "s3",
    aws_access_key_id=config.S3_ACCESS_KEY,
    aws_secret_access_key=config.S3_SECRET_KEY,
    endpoint_url=f"https://{config.S3_URL}",
)


def get_s3() -> boto3.client:
    yield s3
