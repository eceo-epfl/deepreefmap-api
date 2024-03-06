from app.config import config
import boto3
from typing import Any, Generator
from app.objects.models import S3Object


class S3Connection:
    def __init__(
        self,
        aws_access_key_id=config.S3_ACCESS_KEY,
        aws_secret_access_key=config.S3_SECRET_KEY,
        endpoint_url=f"https://{config.S3_URL}",
    ) -> None:
        """Initialise connection"""
        self.session = boto3.client(
            "s3",
            aws_access_key_id=aws_access_key_id,
            aws_secret_access_key=aws_secret_access_key,
            endpoint_url=endpoint_url,
        )

    def get_s3_submission_inputs(
        self,
        submission_id,
    ) -> Any:
        """List all objects in the S3 bucket"""
        prefix = f"{config.S3_PREFIX}/{submission_id}/inputs"

        responses = self.session.list_objects(
            Bucket=config.S3_BUCKET_ID,
            Prefix=prefix,
        ).get("Contents", None)

        if not responses:
            # Don't continue if there are no objects in the bucket
            return []

        # Restructure responses into S3Objects
        objs = [
            S3Object(
                filename=response["Key"],
                last_updated=response["LastModified"],
                size_bytes=response["Size"],
                hash_md5sum=response["ETag"],
            )
            for response in responses
        ]

        return objs

    def session(
        self,
    ) -> boto3.client:
        yield self.session


def get_s3() -> Generator[S3Connection, None, None]:
    yield S3Connection()
