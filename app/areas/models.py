from sqlmodel import SQLModel, Field, Column
from geoalchemy2 import Geometry
from uuid import uuid4, UUID
from typing import Any


class AreaBase(SQLModel):
    name: str = Field(default=None, index=True)
    description: str
    # boundary: Any = Field(sa_column=Column(Geometry("POLYGON", srid=4326)))
    # parent_id: int = Field(default=None, foreign_key="area.id")
    # children: list["Area"] = Field(default=None, sa_relation="*")


class Area(AreaBase, table=True):
    id: int = Field(
        default=None,
        nullable=False,
        primary_key=True,
        index=True,
    )
    uuid: UUID = Field(
        default_factory=uuid4,
        index=True,
        nullable=False,
    )


class AreaRead(AreaBase):
    id: UUID  # We use the UUID as the return ID
    centroid: Any
    location: Any


class AreaCreate(AreaBase):
    pass
