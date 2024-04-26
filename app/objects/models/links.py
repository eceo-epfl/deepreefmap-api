from sqlmodel import SQLModel, Field, UniqueConstraint, Relationship
from uuid import UUID
from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from app.submissions.models import Submission
    from app.objects.models import InputObject


class InputObjectAssociationsBase(SQLModel):
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
    processing_order: int = Field(
        default=0,
        nullable=False,
    )


class InputObjectAssociations(InputObjectAssociationsBase, table=True):
    __table_args__ = (
        UniqueConstraint(
            "input_object_id",
            "submission_id",
            name="no_same_link_constraint",
        ),
        UniqueConstraint(
            "processing_order",
            "submission_id",
            name="no_same_order_constraint",
        ),
    )

    iterator: int = Field(
        default=None,
        nullable=False,
        primary_key=True,
        index=True,
    )

    submission: "Submission" = Relationship(
        back_populates="input_associations",
        sa_relationship_kwargs={"lazy": "selectin"},
    )

    input_object: "InputObject" = Relationship(
        back_populates="input_associations",
        sa_relationship_kwargs={"lazy": "selectin"},
    )


class InputObjectAssociationsRead(InputObjectAssociationsBase):
    input_object: "InputObject"


class InputObjectAssociationsUpdate(InputObjectAssociationsBase):
    pass
