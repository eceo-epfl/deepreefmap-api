from sqlmodel import SQLModel, Field, Column
from geoalchemy2 import Geometry
from uuid import uuid4, UUID
from typing import Any


class SensorBase(SQLModel):
    name: str = Field(default=None, index=True)
    description: str


class Sensor(SensorBase, table=True):
    id: int = Field(
        default=None,
        nullable=False,
        primary_key=True,
        index=True,
    )
    uuid: UUID = Field(
        default_factory=uuid4,
        index=True,
        nullable=False,
    )


class SensorRead(SensorBase):
    id: UUID
    location: list[float]
    area_id: UUID


class SensorCreate(SensorBase):
    pass
