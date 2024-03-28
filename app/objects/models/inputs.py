from pydantic import BaseModel, field_validator
import datetime
from sqlmodel import SQLModel, Field, UniqueConstraint, Relationship
from uuid import uuid4, UUID
from typing import TYPE_CHECKING
from app.objects.models.links import InputObjectAssociations

if TYPE_CHECKING:
    from app.submissions.models import Submission


class InputObjectBase(SQLModel):
    filename: str | None = Field(default=None, index=True)
    size_bytes: int | None = Field(default=None)
    hash_md5sum: str | None = Field(default=None)


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
    submissions: list["Submission"] = Relationship(
        back_populates="inputs",
        link_model=InputObjectAssociations,
        sa_relationship_kwargs={"lazy": "selectin"},
    )


class InputObjectRead(InputObjectBase):
    id: UUID
    time_added_utc: datetime.datetime


class InputObjectUpdate(InputObjectBase):
    pass
