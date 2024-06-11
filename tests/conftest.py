from fastapi.testclient import TestClient
from typing import Generator, AsyncGenerator
from app.db import get_session
from app.main import app
import pytest
import pytest_asyncio
from sqlmodel import SQLModel
from sqlalchemy.ext.asyncio import create_async_engine
from sqlalchemy.orm import sessionmaker
from sqlmodel.ext.asyncio.session import AsyncSession
from app.config import config
from sqlalchemy.sql import text

# DATABASE_URL = "postgresql+asyncpg://postgres:psql@localhost:5444/postgres"


# Asynchronous engine for the rest of the operations

engine = create_async_engine(
    config.DB_URL, echo=False, future=True, pool_pre_ping=True
)


@pytest_asyncio.fixture(scope="function")
async def async_session() -> AsyncGenerator[AsyncSession, None]:
    async_session_factory = sessionmaker(
        engine, class_=AsyncSession, expire_on_commit=False
    )

    async with async_session_factory() as session:
        # Create postgis extension
        # await session.execute(text("CREATE EXTENSION IF NOT EXISTS postgis"))

        async with engine.begin() as conn:
            await conn.run_sync(SQLModel.metadata.create_all)

        yield session

        async with engine.begin() as conn:
            await conn.run_sync(SQLModel.metadata.drop_all)

    await engine.dispose()


@pytest.fixture
def client(
    async_session: AsyncSession,
) -> Generator[TestClient, None, None]:
    def override_get_session():
        yield async_session

    app.dependency_overrides[get_session] = override_get_session

    with TestClient(app) as client:
        yield client
    app.dependency_overrides.clear()
