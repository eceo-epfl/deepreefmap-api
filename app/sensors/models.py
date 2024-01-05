from sqlmodel import SQLModel, Field, Column, Relationship, UniqueConstraint
from geoalchemy2 import Geometry, WKBElement
from uuid import uuid4, UUID
from typing import Any
from pydantic import validator, root_validator
import shapely
from typing import TYPE_CHECKING
import datetime

if TYPE_CHECKING:
    from app.areas.models import Area


class SensorBase(SQLModel):
    name: str = Field(default=None, index=True)
    description: str | None = Field(default=None)
    comment: str | None = Field(default=None)
    elevation: float | None = Field(default=None)
    time_recorded_at_utc: datetime.datetime = Field(
        default=None,
        nullable=True,
        index=True,
    )


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
    time_ingested_at_utc: datetime.datetime = Field(
        default_factory=datetime.datetime.now,
        nullable=False,
        index=True,
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


class SensorDataBase(SQLModel):
    instrument_seq: int = Field(  # The iterator integer in the instrument
        index=True,
        nullable=False,
    )
    time: datetime.datetime = Field(
        index=True,
        nullable=False,
    )
    time_zone: int | None = Field(
        index=False,
        nullable=True,
    )
    temperature_1: float | None = Field(
        index=True,
        nullable=True,
    )
    temperature_2: float | None = Field(
        index=True,
        nullable=True,
    )
    temperature_3: float | None = Field(
        index=True,
        nullable=True,
    )
    river_moisture_count: float | None = Field(
        index=True,
        nullable=True,
    )
    shake: int | None = Field(
        index=False,
        nullable=True,
    )
    error_flat: int | None = Field(
        index=False,
        nullable=True,
    )


class SensorData(SensorDataBase, table=True):
    __table_args__ = (UniqueConstraint("id"),)
    iterator: int = Field(
        nullable=False,
        primary_key=True,
        index=True,
    )
    id: UUID = Field(
        default_factory=uuid4,
        index=True,
        nullable=False,
    )

    sensor_id: UUID = Field(
        default=None, foreign_key="sensor.id", nullable=False, index=True
    )

    sensor: Sensor = Relationship(
        back_populates="data",
        sa_relationship_kwargs={"lazy": "selectin"},
    )


class SensorDataRead(SensorDataBase):
    id: UUID
    sensor_id: UUID


class SensorRead(SensorBase):
    id: UUID
    geom: Any
    area_id: UUID
    latitude: Any
    longitude: Any

    @root_validator
    def convert_wkb_to_lat_lon(cls, values: dict) -> dict:
        """Form the geometry from the latitude and longitude"""
        if isinstance(values["geom"], WKBElement):
            if values["geom"] is not None:
                shapely_obj = shapely.wkb.loads(str(values["geom"]))
                if shapely_obj is not None:
                    mapping = shapely.geometry.mapping(shapely_obj)

                    values["latitude"] = mapping["coordinates"][0]
                    values["longitude"] = mapping["coordinates"][1]
                    values["geom"] = mapping
        elif isinstance(values["geom"], dict):
            if values["geom"] is not None:
                values["latitude"] = values["geom"]["coordinates"][0]
                values["longitude"] = values["geom"]["coordinates"][1]
                values["geom"] = values["geom"]
        else:
            values["latitude"] = None
            values["longitude"] = None

        return values


class SensorDataSummary(SQLModel):
    start_date: datetime.datetime | None = None
    end_date: datetime.datetime | None = None
    qty_records: int | None = None


class SensorReadWithDataSummary(SensorRead):
    data: SensorDataSummary


class SensorReadWithDataSummaryAndPlot(SensorRead):
    data: SensorDataSummary | None
    temperature_plot: list[SensorDataRead] | None = None


class SensorCreate(SensorBase):
    area_id: UUID
    latitude: float
    longitude: float

    geom: Any | None = None

    @root_validator(pre=True)
    def convert_lat_lon_to_wkt(cls, values: dict) -> dict:
        """Form the geometry from the latitude and longitude"""

        if "latitude" in values and "longitude" in values:
            values[
                "geom"
            ] = f"POINT({values['latitude']} {values['longitude']})"

        return values


class SensorUpdate(SensorCreate):
    instrumentdata: str | None = None


class SensorCreateFromGPX(SQLModel):
    # Model to accept the data from the GPSX file. Data stored in Base64 of gpx
    area_id: UUID
    gpsx_files: list[Any] | None = None
