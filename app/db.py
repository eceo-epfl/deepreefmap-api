import os
from sqlmodel import SQLModel, create_engine
from sqlmodel.ext.asyncio.session import AsyncSession, AsyncEngine
from sqlalchemy.orm import sessionmaker
from geoalchemy2 import load_spatialite
from sqlalchemy.event import listen
from sqlalchemy import event
from app.areas.models import Area
from app.sensors.models import Sensor, SensorData
from app.config import config
from datetime import datetime


engine = AsyncEngine(create_engine(config.DB_URL, echo=True, future=True))


async def get_session() -> AsyncSession:
    async_session = sessionmaker(
        engine, class_=AsyncSession, expire_on_commit=False
    )
    async with async_session() as session:
        yield session
