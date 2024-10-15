import pytest
import pytest_asyncio
from httpx import ASGITransport, AsyncClient
from typing import AsyncGenerator
from sqlalchemy.ext.asyncio import AsyncSession
from sqlmodel import SQLModel
from app.db import get_session, engine, async_session
from app.main import app
from app.submissions.k8s import get_k8s_v1
from app.objects.service import get_s3
from app.users.models import User
from app.auth.services import get_user_info
import copy


@pytest_asyncio.fixture
async def modified_async_session() -> AsyncGenerator[AsyncSession, None]:

    async def get_session() -> AsyncGenerator[AsyncSession, None]:
        async with async_session() as session:
            yield session

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


@pytest.fixture
def test_user_one():
    return User(
        id="541f0b39-89b1-4216-a8b5-30888282f4e1",
        username="test_user",
        email="test_user@example.org",
        first_name="Test",
        last_name="User One",
        realm_roles=["user"],
        client_roles=["user"],
    )


@pytest_asyncio.fixture
async def client_one_user(
    modified_async_session: AsyncSession,
    test_user_one: User,
) -> AsyncGenerator[AsyncClient, None]:
    def override_get_session():
        yield modified_async_session

    def override_get_user_info():
        return test_user_one

    client_one = copy.copy(app)
    client_one.dependency_overrides[get_session] = override_get_session
    client_one.dependency_overrides[get_k8s_v1] = override_get_k8s_v1
    client_one.dependency_overrides[get_s3] = override_get_s3
    client_one.dependency_overrides[get_user_info] = override_get_user_info
    async with AsyncClient(
        transport=ASGITransport(app=client_one), base_url="http://test"
    ) as client:
        yield client

    client_one.dependency_overrides.clear()


@pytest.fixture
def test_user_two():
    return User(
        id="7d225d0c-a48f-460c-a921-93abfa1e6ed3",
        username="test_user",
        email="test_two_user@example.com",
        first_name="Test",
        last_name="User Two",
        realm_roles=["user"],
        client_roles=["user"],
    )


@pytest_asyncio.fixture
async def client_two_user(
    modified_async_session: AsyncSession,
    test_user_two: User,
) -> AsyncGenerator[AsyncClient, None]:
    def override_get_session():
        yield modified_async_session

    def override_get_user_info():
        return test_user_two

    client_two = copy.copy(app)
    client_two.dependency_overrides[get_session] = override_get_session
    client_two.dependency_overrides[get_k8s_v1] = override_get_k8s_v1
    client_two.dependency_overrides[get_s3] = override_get_s3
    client_two.dependency_overrides[get_user_info] = override_get_user_info
    async with AsyncClient(
        transport=ASGITransport(app=client_two), base_url="http://test"
    ) as client:
        yield client

    client_two.dependency_overrides.clear()


@pytest.fixture
def test_admin_user_three():
    return User(
        id="eab1fac6-4669-4d9e-ad69-5aa4a2bbeeff",
        username="test_admin",
        email="test_admin_user@example.org",
        first_name="Test",
        last_name="Admin",
        realm_roles=["admin"],
        client_roles=["admin"],
    )


@pytest_asyncio.fixture
async def client_three_admin(
    modified_async_session: AsyncSession,
    test_admin_user_three: User,
) -> AsyncGenerator[AsyncClient, None]:
    def override_get_session():
        yield modified_async_session

    def override_get_user_info():

        return test_admin_user_three

    client_three = copy.copy(app)
    client_three.dependency_overrides[get_session] = override_get_session
    client_three.dependency_overrides[get_k8s_v1] = override_get_k8s_v1
    client_three.dependency_overrides[get_s3] = override_get_s3
    client_three.dependency_overrides[get_user_info] = override_get_user_info
    async with AsyncClient(
        transport=ASGITransport(app=client_three), base_url="http://test"
    ) as client:
        yield client

    client_three.dependency_overrides.clear()
