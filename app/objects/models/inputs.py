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

if TYPE_CHECKING:
    from app.submissions.models import Submission


class InputObjectBase(SQLModel):
    filename: str | None = Field(default=None, index=True)
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
    parts: list[dict[str, Any]] = Field(default=[], sa_column=Column(JSON))

    class Config:
        arbitrary_types_allowed = True


class InputObjectRead(InputObjectBase):
    id: UUID
    time_added_utc: datetime.datetime
    input_associations: list[InputObjectAssociations | None] = []


class InputObjectUpdate(InputObjectBase):
    pass
