from sqlmodel import SQLModel, Field

from uuid import uuid4, UUID


class AreaBase(SQLModel):
    name: str = Field(default=None, index=True)
    description: str
    # parent_id: int = Field(default=None, foreign_key="area.id")
    # children: list["Area"] = Field(default=None, sa_relation="*")


class Area(SQLModel, table=True):
    id: int = Field(default=None, primary_key=True)
    uuid: UUID = Field(
        primary_key=True,
        default_factory=uuid4,
        index=True,
        nullable=False,
    )


class AreaRead(AreaBase):
    pass


class AreaCreate(AreaBase):
    pass
