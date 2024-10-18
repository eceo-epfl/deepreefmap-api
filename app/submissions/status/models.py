from sqlmodel import (
    SQLModel,
    Field,
    Relationship,
    JSON,
    Column,
)
from uuid import uuid4, UUID
from typing import Any, TYPE_CHECKING
from sqlalchemy.sql import func
import datetime

if TYPE_CHECKING:
    from app.submissions.models import Submission


class RunStatusBase(SQLModel):
    submission_id: UUID = Field(foreign_key="submission.id", index=True)
    kubernetes_pod_name: str = Field(default=None, index=True)
    status: str | None = Field(default=None, index=True)
    is_running: bool = Field(default=False, index=True)
    is_successful: bool = Field(default=False, index=True)
    is_still_kubernetes_resource: bool = Field(default=False, index=True)
    time_started: str | None = Field(default=None, index=True)
    logs: list[str] = Field(default=[], sa_column=Column(JSON))
    time_added_utc: datetime.datetime = Field(
        default_factory=datetime.datetime.now,
        nullable=False,
        index=True,
    )
    last_updated: datetime.datetime = Field(
        default_factory=datetime.datetime.now,
        title="Last Updated",
        description="Date and time when the record was last updated",
        sa_column_kwargs={
            "onupdate": func.now(),
            "server_default": func.now(),
        },
    )


class RunStatus(RunStatusBase, table=True):
    id: UUID = Field(
        default_factory=uuid4,
        index=True,
        nullable=False,
        primary_key=True,
    )

    submission: "Submission" = Relationship(back_populates="run_status")


class RunStatusCreate(RunStatusBase):
    pass


class RunStatusRead(RunStatusBase):
    id: UUID
    submission: Any


class RunStatusUpdate(RunStatusBase):
    pass


class RunStatusLogRead(SQLModel):
    id: str
    message: str
