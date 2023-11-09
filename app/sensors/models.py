from sqlmodel import SQLModel, Field, Column, Relationship, UniqueConstraint
from geoalchemy2 import Geometry, WKBElement
from uuid import uuid4, UUID
from typing import Any
from pydantic import validator
import shapely
from typing import TYPE_CHECKING
import datetime

if TYPE_CHECKING:
    from app.areas.models import AreaRead
    from app.areas.models import Area


class SensorBase(SQLModel):
    name: str = Field(default=None, index=True)
    description: str


class Sensor(SensorBase, table=True):
    __table_args__ = (UniqueConstraint("id"),)
    iterator: int = Field(
        default=None,
        nullable=False,
        primary_key=True,
        index=True,
    )
    id: UUID = Field(
        default_factory=uuid4,
        index=True,
        nullable=False,
    )
    geom: Any = Field(sa_column=Column(Geometry("POINT", srid=4326)))

    area_id: UUID = Field(default=None, foreign_key="area.id")
    area: "Area" = Relationship(
        back_populates="sensors", sa_relationship_kwargs={"lazy": "selectin"}
    )
    data: list["SensorData"] = Relationship(
        back_populates="sensor",
        sa_relationship_kwargs={"lazy": "selectin"},
    )


class SensorData(SQLModel, table=True):
    __table_args__ = (UniqueConstraint("id"),)
    iterator: int = Field(
        default=None,
        nullable=False,
        primary_key=True,
        index=True,
    )
    id: UUID = Field(
        default_factory=uuid4,
        index=True,
        nullable=False,
    )
    time: datetime.datetime = Field(
        index=True,
        nullable=False,
    )
    instrument_seq: int = Field(  # The iterator integer in the instrument
        index=True,
        nullable=False,
    )

    temperature_1: float = Field(
        index=True,
        nullable=True,
    )
    temperature_2: float = Field(
        index=True,
        nullable=True,
    )
    temperature_3: float = Field(
        index=True,
        nullable=True,
    )
    humidity: float = Field(
        index=True,
        nullable=True,
    )
    sensor_id: UUID = Field(default=None, foreign_key="sensor.id")

    sensor: Sensor = Relationship(
        back_populates="data",
        sa_relationship_kwargs={"lazy": "selectin"},
    )


class SensorRead(SensorBase):
    id: UUID
    geom: Any
    area_id: UUID
    data: list[SensorData]

    @validator("geom")
    def convert_wkb_to_json(cls, v: WKBElement) -> Any:
        """Convert the WKBElement to a shapely mapping"""
        if isinstance(v, WKBElement):
            return shapely.geometry.mapping(shapely.wkb.loads(str(v)))
        else:
            return v


class SensorCreate(SensorBase):
    pass
