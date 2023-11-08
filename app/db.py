import os
from sqlmodel import SQLModel, create_engine
from sqlmodel.ext.asyncio.session import AsyncSession, AsyncEngine
from sqlalchemy.orm import sessionmaker
from geoalchemy2 import load_spatialite
from sqlalchemy.event import listen
from sqlalchemy import event
from app.areas.models import Area
from app.sensors.models import Sensor

# DATABASE_URL = os.environ.get("DATABASE_URL")
DATABASE_URL = "postgresql+asyncpg://postgres:psql@localhost:5432/postgres"

engine = AsyncEngine(create_engine(DATABASE_URL, echo=True, future=True))


async def init_db():
    async with engine.begin() as conn:
        # Drop all and start with a seeded DB whilst in development
        # Create database if it does not exist.
        # conn.execute("CREATE DATABASE IF NOT EXISTS {engine.url.database}}")

        # await conn.run_sync(SQLModel.metadata.drop_all)
        # await conn.run_sync(SQLModel.metadata.create_all)
        from app.areas.views import add_areas_to_db
        from app.sensors.views import add_sensors_to_db as add_sensors

        # await add_areas_to_db()
        await add_sensors()
        # pass


async def get_session() -> AsyncSession:
    async_session = sessionmaker(
        engine, class_=AsyncSession, expire_on_commit=False
    )
    async with async_session() as session:
        yield session
