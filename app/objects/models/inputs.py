import datetime
from sqlmodel import (
    SQLModel,
    Field,
    UniqueConstraint,
    Relationship,
    Column,
    JSON,
    BigInteger,
)
from uuid import uuid4, UUID
from typing import Any, TYPE_CHECKING
from app.objects.models.links import InputObjectAssociations
from pydantic import model_validator
from geoalchemy2 import WKBElement
import shapely

if TYPE_CHECKING:
    from app.submissions.models import Submission
    from app.transects.models import Transect


class InputObjectBase(SQLModel):
    filename: str | None = Field(default=None, index=True)
    owner: UUID | None = Field(default=None, nullable=True)
    size_bytes: int | None = Field(
        default=None, sa_column=Column(BigInteger())
    )
    hash_md5sum: str | None = Field(default=None)
    notes: str | None = Field(default=None)
    upload_id: str | None = Field(default=None)
    fps: float | None = Field(default=None)
    time_seconds: float | None = Field(default=None)
    frame_count: int | None = Field(default=None)
    processing_has_started: bool = Field(default=False)
    processing_completed_successfully: bool = Field(default=False)
    processing_message: str | None = Field(default=None)
    last_part_received_utc: datetime.datetime | None = Field(default=None)
    all_parts_received: bool = Field(default=False)
    transect_id: UUID | None = Field(default=None, foreign_key="transect.id")


class InputObject(InputObjectBase, table=True):
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
    time_added_utc: datetime.datetime = Field(
        default_factory=datetime.datetime.now,
        nullable=False,
        index=True,
    )

    input_associations: list[InputObjectAssociations] = Relationship(
        back_populates="input_object",
        sa_relationship_kwargs={"lazy": "selectin"},
    )

    submissions: list["Submission"] = Relationship(
        back_populates="inputs",
        sa_relationship_kwargs={"lazy": "selectin"},
        link_model=InputObjectAssociations,
    )
    transect: "Transect" = Relationship(
        back_populates="inputs", sa_relationship_kwargs={"lazy": "selectin"}
    )

    parts: list[dict[str, Any]] = Field(default=[], sa_column=Column(JSON))

    class Config:
        arbitrary_types_allowed = True


class TransectRead(SQLModel):
    id: UUID
    name: str
    description: str | None = None
    geom: Any | None = None

    latitude_start: float | None = None
    longitude_start: float | None = None

    latitude_end: float | None = None
    longitude_end: float | None = None

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


class InputObjectRead(InputObjectBase):
    id: UUID
    time_added_utc: datetime.datetime
    input_associations: list[InputObjectAssociations | None] = []
    transect: TransectRead | None = None


class InputObjectUpdate(InputObjectBase):
    pass
