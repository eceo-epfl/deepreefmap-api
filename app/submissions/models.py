from sqlmodel import (
    SQLModel,
    Field,
    Relationship,
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


class SubmissionBase(SQLModel):
    name: str | None = Field(default=None, index=True)
    description: str | None = Field(default=None)
    comment: str | None = Field(default=None)
    processing_has_started: bool = Field(default=False)
    processing_completed_successfully: bool = Field(default=False)
    fps: int | None = Field(default=None)
    time_seconds_start: int | None = Field(default=None)
    time_seconds_end: int | None = Field(default=None)


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


# class InputObjectLinks(SQLModel):
#     input_object_id: UUID
#     processing_order: int


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


class SubmissionRead(SubmissionBase):
    id: UUID
    time_added_utc: datetime.datetime
    run_status: list[KubernetesExecutionStatus] = []
    input_associations: list[InputObjectAssociationsRead] = []
    file_outputs: list[SubmissionFileOutputs] = []


class SubmissionUpdate(SubmissionBase):
    input_associations: list[InputObjectAssociationsUpdate]


class SubmissionJobLogRead(SQLModel):
    id: str
    message: str
