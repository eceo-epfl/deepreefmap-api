from pydantic import BaseModel, field_validator
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
from typing import TYPE_CHECKING, Any
from app.objects.models.links import (
    InputObjectAssociations,
    InputObjectAssociationsRead,
)

if TYPE_CHECKING:
    from app.submissions.models import Submission


class InputObjectBase(SQLModel):
    filename: str | None = Field(default=None, index=True)
    size_bytes: int | None = Field(
        default=None, sa_column=Column(BigInteger())
    )
    hash_md5sum: str | None = Field(default=None)
    notes: str | None = Field(default=None)
    parts: list[dict[str, Any]] = Field(default=[], sa_column=Column(JSON))
    upload_id: str | None = Field(default=None)


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

    class Config:
        arbitrary_types_allowed = True


class InputObjectRead(InputObjectBase):
    id: UUID
    time_added_utc: datetime.datetime
    parts: Any


class InputObjectUpdate(InputObjectBase):
    pass
