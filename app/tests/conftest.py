from fastapi.testclient import TestClient
from httpx import AsyncClient
from typing import Generator
from app.db import init_db, get_session
from app.main import app
from app.config import config
import pytest
import pytest_asyncio
from sqlmodel import SQLModel, create_engine
from sqlmodel.ext.asyncio.session import (
    AsyncSession,
    AsyncEngine,
)
from sqlalchemy.orm import sessionmaker


DATABASE_URL = "sqlite+aiosqlite:///"

engine = AsyncEngine(create_engine(DATABASE_URL, echo=True, future=True))


@pytest_asyncio.fixture(scope="function")
async def async_session() -> AsyncSession:
    session = sessionmaker(engine, class_=AsyncSession, expire_on_commit=False)

    async with session() as s:
        async with engine.begin() as conn:
            await conn.run_sync(SQLModel.metadata.create_all)

        yield s

    async with engine.begin() as conn:
        await conn.run_sync(SQLModel.metadata.drop_all)

    await engine.dispose()


@pytest_asyncio.fixture
async def async_client():
    async with AsyncClient(
        app=app,
        base_url=f"http://localhost:8000{config.API_V1_PREFIX}",
    ) as client:
        yield client
