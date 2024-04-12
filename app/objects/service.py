from app.config import config
import aioboto3
from typing import Any, AsyncGenerator
from app.objects.models import S3Object


async def get_s3() -> AsyncGenerator[aioboto3.Session, None]:
    session = aioboto3.Session()
    async with session.client(
        "s3",
        aws_access_key_id=config.S3_ACCESS_KEY,
        aws_secret_access_key=config.S3_SECRET_KEY,
        endpoint_url=f"https://{config.S3_URL}",
    ) as client:
        yield client
