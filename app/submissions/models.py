from sqlmodel import (
    SQLModel,
    Field,
    Relationship,
    JSON,
    Column,
    UniqueConstraint,
)
from uuid import uuid4, UUID
import datetime
from app.objects.models import (
    InputObject,
    InputObjectAssociations,
    InputObjectAssociationsRead,
    InputObjectAssociationsUpdate,
)
from typing import Any, TYPE_CHECKING
from typing_extensions import Self
from pydantic import model_validator
from geoalchemy2 import WKBElement
import shapely

if TYPE_CHECKING:
    from app.transects.models import Transect


class SubmissionBase(SQLModel):
    name: str | None = Field(default=None, index=True)
    owner: UUID = Field(default=None, nullable=True, index=True)
    description: str | None = Field(default=None)
    comment: str | None = Field(default=None)
    processing_has_started: bool = Field(default=False)
    processing_completed_successfully: bool = Field(default=False)
    fps: int | None = Field(default=None)
    time_seconds_start: int | None = Field(default=None)
    time_seconds_end: int | None = Field(default=None)
    percentage_covers: list[dict[str, Any]] = Field(
        default=[], sa_column=Column(JSON)
    )
    transect_id: UUID | None = Field(default=None, foreign_key="transect.id")

    @model_validator(mode="after")
    def validate_time_seconds(
        self: Self,
    ) -> Self:
        if self.time_seconds_start and self.time_seconds_end:
            if self.time_seconds_start > self.time_seconds_end:
                raise ValueError("Start time must be less than end time")
        if self.time_seconds_start and self.time_seconds_start < 0:
            raise ValueError("Start time must be >= 0")
        if self.time_seconds_end and self.time_seconds_end < 0:
            raise ValueError("End time must be >= 0")
        return self

    @model_validator(mode="after")
    def validate_fps(
        self: Self,
    ) -> Self:
        if self.fps:
            if self.fps <= 0:
                raise ValueError("fps must be greater than 0")
        return self


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
    time_added_utc: datetime.datetime = Field(
        default_factory=datetime.datetime.now,
        nullable=False,
        index=True,
    )

    inputs: list[InputObject] = Relationship(
        back_populates="submissions",
        sa_relationship_kwargs={"lazy": "selectin"},
        link_model=InputObjectAssociations,
    )

    input_associations: list[InputObjectAssociations] = Relationship(
        back_populates="submission",
        sa_relationship_kwargs={"lazy": "selectin"},
    )

    transect: "Transect" = Relationship(
        back_populates="submissions",
        sa_relationship_kwargs={"lazy": "selectin"},
    )


class SubmissionCreate(SubmissionBase):
    # A list of UUIDs corresponding to InputObject IDs
    input_associations: list[InputObjectAssociations] = []


class KubernetesExecutionStatus(SQLModel):
    # Information from RCP about the execution status of the submission
    submission_id: str
    status: str
    time_started: str | None = None


class SubmissionFileOutputs(SQLModel):
    # Information about the output files of a submission
    filename: str
    size_bytes: int
    last_modified: datetime.datetime
    url: str


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


class SubmissionRead(SubmissionBase):
    id: UUID
    time_added_utc: datetime.datetime
    run_status: list[KubernetesExecutionStatus] = []
    input_associations: list[InputObjectAssociationsRead] = []
    file_outputs: list[SubmissionFileOutputs] = []
    transect: TransectRead | None = None


class SubmissionUpdate(SubmissionBase):
    input_associations: list[InputObjectAssociationsUpdate]


class SubmissionJobLogRead(SQLModel):
    id: str
    message: str
