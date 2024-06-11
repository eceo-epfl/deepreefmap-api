import pytest
import pytest_asyncio
from fastapi.testclient import TestClient
from httpx import AsyncClient
from typing import AsyncGenerator
from sqlalchemy.ext.asyncio import create_async_engine, AsyncSession
from sqlalchemy.orm import sessionmaker
from sqlmodel import SQLModel
from app.db import get_session, engine, async_session
from app.main import app
from app.config import config
from app.submissions.k8s import get_k8s_v1
from app.objects.service import get_s3


@pytest_asyncio.fixture
async def modified_async_session() -> AsyncGenerator[AsyncSession, None]:
    # Make sure to delete all data before and after each test

    async with async_session() as session:
        async with engine.begin() as conn:
            await conn.run_sync(SQLModel.metadata.drop_all)
            await conn.run_sync(SQLModel.metadata.create_all)

        yield session

    await engine.dispose()


def override_get_k8s_v1():
    # Return a class that mocks the kubernetes CoreV1Api

    class MockPod:
        def __init__(self, name):
            self.metadata = MockMetadata(name)

    class MockMetadata:
        def __init__(self, name):
            self.name = name

    class MockCoreV1Api:
        def list_namespaced_pod(self, namespace):
            return MockPodList()

    class MockPodList:
        def __init__(self):
            self.items = [MockPod("test-job")]

    return MockCoreV1Api()


# async def get_s3() -> AsyncGenerator[aioboto3.Session, None]:
#     session = aioboto3.Session()
#     async with session.client(
#         "s3",
#         aws_access_key_id=config.S3_ACCESS_KEY,
#         aws_secret_access_key=config.S3_SECRET_KEY,
#         endpoint_url=f"https://{config.S3_URL}",
#     ) as client:
#         yield client


def override_get_s3():
    class MockListObjectsV2:
        async def __call__(self, Bucket, Prefix):
            return {
                "Contents": [
                    {
                        "Key": "test-key",
                        "Size": 12345,
                        "LastModified": "2021-10-01T00:00:00Z",
                    }
                ]
            }

    class MockS3:
        def __init__(self):
            self.list_objects_v2 = MockListObjectsV2()

        async def get_object(self, Bucket, Key):
            return {"Body": {"read": lambda: b'{"test": "data"}'}}

    return MockS3()


@pytest_asyncio.fixture
async def client(
    modified_async_session: AsyncSession,
) -> AsyncGenerator[AsyncClient, None]:
    def override_get_session():
        yield modified_async_session

    app.dependency_overrides[get_session] = override_get_session
    app.dependency_overrides[get_k8s_v1] = override_get_k8s_v1
    app.dependency_overrides[get_s3] = override_get_s3
    async with AsyncClient(app=app, base_url="http://test") as client:
        yield client

    app.dependency_overrides.clear()
