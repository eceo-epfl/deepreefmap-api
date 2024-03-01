from sqlmodel import SQLModel, Field, Column, Relationship, UniqueConstraint
from uuid import uuid4, UUID
from typing import Any
import datetime


class SubmissionBase(SQLModel):
    name: str | None = Field(default=None, index=True)
    description: str | None = Field(default=None)
    comment: str | None = Field(default=None)


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


class SubmissionRead(SubmissionBase):
    id: UUID
    data_size_mb: float | None = Field(default=None)
    processing_finished: bool | None = Field(default=None)
    processing_successful: bool | None = Field(default=None)
    duration_seconds: int | None = Field(default=None)
    submitted_at_utc: datetime.datetime | None = Field(default=None)
    submitted_by: str | None = Field(default=None)


class SubmissionCreate(SubmissionBase):
    area_id: UUID


class SubmissionUpdate(SubmissionCreate):
    instrumentdata: str | None = None
