from pydantic import BaseModel, field_validator
import datetime
from sqlmodel import SQLModel, Field, UniqueConstraint, Relationship
from uuid import uuid4, UUID
from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from app.submissions.models import Submission
    from app.objects.models import InputObject


class InputObjectAssociations(SQLModel, table=True):
    __table_args__ = (
        UniqueConstraint(
            "input_object_id",
            "submission_id",
            name="no_same_link_constraint",
        ),
    )

    iterator: int = Field(
        default=None,
        nullable=False,
        primary_key=True,
        index=True,
    )
    input_object_id: UUID = Field(
        foreign_key="inputobject.id",
        index=True,
        nullable=False,
    )
    submission_id: UUID = Field(
        foreign_key="submission.id",
        index=True,
        nullable=False,
    )

    # input: "InputObject" = Relationship(
    #     back_populates="submission_links",
    #     sa_relationship_kwargs={"lazy": "selectin"},
    # )
    # submission: "Submission" = Relationship(
    #     back_populates="input_links",
    #     sa_relationship_kwargs={"lazy": "selectin"},
    # )
