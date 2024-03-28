from sqlmodel import SQLModel, Field, Column, Relationship, UniqueConstraint
from uuid import uuid4, UUID
from typing import Any, TYPE_CHECKING
import datetime
from fastapi import UploadFile, File
from app.objects.models import InputObjectAssociations

# if TYPE_CHECKING:
from app.objects.models import InputObject

# from app.objects.models import S3Object


class SubmissionBase(SQLModel):
    name: str | None = Field(default=None, index=True)
    description: str | None = Field(default=None)
    comment: str | None = Field(default=None)
    processing_has_started: bool = Field(default=False)
    processing_completed_successfully: bool = Field(default=False)


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
        link_model=InputObjectAssociations,
        sa_relationship_kwargs={"lazy": "selectin"},
    )


class SubmissionCreate(SubmissionBase):
    inputs: list[UUID] = []  # A list of UUIDs corresponding to InputObject IDs


class KubernetesExecutionStatus(SQLModel):
    # Information from RCP about the execution status of the submission
    submission_id: str
    status: str
    time_started: datetime.datetime


class SubmissionRead(SubmissionBase):
    id: UUID
    time_added_utc: datetime.datetime
    inputs: list[InputObject] = []
    run_status: list[KubernetesExecutionStatus] = []


class SubmissionUpdate(SubmissionBase):
    pass
