from sqlmodel import (
    SQLModel,
    Field,
    UniqueConstraint,
    Relationship,
    Column,
)
from pydantic import model_validator
from typing import Any, TYPE_CHECKING
from uuid import uuid4, UUID
import datetime
from sqlalchemy.sql import func
from geoalchemy2 import Geometry, WKBElement
import shapely

if TYPE_CHECKING:
    from app.objects.models.inputs import InputObject
    from app.submissions.models import Submission


class TransectBase(SQLModel):
    name: str
    owner: UUID | None = Field(default=None, nullable=True)
    description: str | None = None
    length: float | None = None
    depth: float | None = None
    created_on: datetime.datetime = Field(
        default_factory=datetime.datetime.now,
        title="Created On",
        description="Date and time when the record was created",
        sa_column_kwargs={"default": func.now()},
    )

    last_updated: datetime.datetime = Field(
        default_factory=datetime.datetime.now,
        title="Last Updated",
        description="Date and time when the record was last updated",
        sa_column_kwargs={
            "onupdate": func.now(),
            "server_default": func.now(),
        },
    )


class Transect(TransectBase, table=True):
    __table_args__ = (
        UniqueConstraint("id"),
        UniqueConstraint("name"),
    )
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
    geom: Any = Field(
        default=None, sa_column=Column(Geometry("LINESTRING", srid=4326))
    )
    inputs: list["InputObject"] = Relationship(
        back_populates="transect", sa_relationship_kwargs={"lazy": "selectin"}
    )
    submissions: list["Submission"] = Relationship(
        back_populates="transect", sa_relationship_kwargs={"lazy": "selectin"}
    )


class TransectCreate(TransectBase):
    latitude_start: float
    longitude_start: float
    latitude_end: float
    longitude_end: float

    geom: Any | None = None

    @model_validator(mode="after")
    def convert_lat_lon_to_wkt(cls, values: Any) -> Any:
        """Convert the lat/lon coordinates to a WKT geometry"""

        # Encode the SRID into the WKT
        values.geom = shapely.wkt.dumps(
            shapely.geometry.LineString(
                [
                    (values.longitude_start, values.latitude_start),
                    (values.longitude_end, values.latitude_end),
                ]
            )
        )

        return values


class TransectRead(TransectBase):
    id: UUID
    geom: Any | None = None

    latitude_start: float | None = None
    longitude_start: float | None = None

    latitude_end: float | None = None
    longitude_end: float | None = None

    inputs: list[Any] = []
    submissions: list[Any] = []

    @model_validator(mode="after")
    def convert_wkb_to_lat_long(
        cls,
        values: "TransectRead",
    ) -> dict:
        """Form the lat/lon geom from the start and end of the line string"""

        if isinstance(values.geom, WKBElement):
            if values.geom is not None:
                shapely_obj = shapely.wkb.loads(str(values.geom))
                if shapely_obj is not None:
                    mapping = shapely.geometry.mapping(shapely_obj)
                    values.latitude_start = mapping["coordinates"][0][1]
                    values.longitude_start = mapping["coordinates"][0][0]
                    values.latitude_end = mapping["coordinates"][-1][1]
                    values.longitude_end = mapping["coordinates"][-1][0]
                    values.geom = mapping

        elif isinstance(values.geom, dict):
            if values.geom is not None:
                values.latitude_start = values.geom["coordinates"][0][1]
                values.longitude_start = values.geom["coordinates"][0][0]
                values.latitude_end = values.geom["coordinates"][-1][1]
                values.longitude_end = values.geom["coordinates"][-1][0]

                values.geom = values.geom

        else:
            values.latitude_start = None
            values.longitude_start = None
            values.latitude_end = None
            values.longitude_end = None

        return values


class TransectUpdate(TransectCreate):
    pass
