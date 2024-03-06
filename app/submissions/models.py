from sqlmodel import SQLModel, Field, Column, Relationship, UniqueConstraint
from uuid import uuid4, UUID
from typing import Any
import datetime
from fastapi import UploadFile, File

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


class SubmissionRead(SubmissionBase):
    id: UUID
    time_added_utc: datetime.datetime
    # inputs: list[S3Object] = []


class SubmissionUpdate(SubmissionBase):
    pass
