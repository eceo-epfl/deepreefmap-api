from sqlmodel import SQLModel, Field, Column, Relationship, UniqueConstraint
from geoalchemy2 import Geometry, WKBElement
from uuid import uuid4, UUID
from typing import Any
from pydantic import validator, root_validator
import shapely
from typing import TYPE_CHECKING
import datetime


class SubmissionBase(SQLModel):
    name: str = Field(default=None, index=True)
    description: str | None = Field(default=None)
    comment: str | None = Field(default=None)
    time_added_utc: datetime.datetime = Field(
        default=None,
        nullable=True,
        index=True,
    )


class Submission(SubmissionBase, table=True):
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
        back_populates="submissions",
        sa_relationship_kwargs={"lazy": "selectin"},
    )
    data: list["SubmissionData"] = Relationship(
        back_populates="sensor",
        sa_relationship_kwargs={"lazy": "selectin"},
    )


class SubmissionDataBase(SQLModel):
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


class SubmissionData(SubmissionDataBase, table=True):
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

    sensor: Submission = Relationship(
        back_populates="data",
        sa_relationship_kwargs={"lazy": "selectin"},
    )


class SubmissionDataRead(SubmissionDataBase):
    id: UUID
    sensor_id: UUID


class SubmissionRead(SubmissionBase):
    id: UUID
    geom: Any
    data_size_mb: float | None = Field(default=None)
    processing_finished: bool | None = Field(default=None)
    processing_successful: bool | None = Field(default=None)
    duration_seconds: int | None = Field(default=None)
    submitted_at_utc: datetime.datetime | None = Field(default=None)
    submitted_by: str | None = Field(default=None)


class SubmissionCreate(SubmissionBase):
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


class SubmissionUpdate(SubmissionCreate):
    instrumentdata: str | None = None
