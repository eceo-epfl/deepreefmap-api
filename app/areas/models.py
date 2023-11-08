from sqlmodel import SQLModel, Field, Column, UniqueConstraint, Relationship
from geoalchemy2 import Geometry, WKBElement
from geoalchemy2.shape import to_shape
from uuid import uuid4, UUID
from typing import Any
import shapely
from pydantic import validator
from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from app.sensors.models import Sensor


class AreaBase(SQLModel):
    name: str = Field(default=None, index=True)
    description: str


class Area(AreaBase, table=True):
    __table_args__ = (UniqueConstraint("uuid"),)
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
    geom: Any = Field(sa_column=Column(Geometry("POLYGON", srid=4326)))

    sensors: list["Sensor"] = Relationship(back_populates="area")


class AreaRead(AreaBase):
    id: UUID  # We use the UUID as the return ID
    centroid: Any
    geom: Any

    @validator("geom")
    def convert_wkb_to_json(cls, v: WKBElement) -> Any:
        """Convert the WKBElement to a shapely mapping"""
        obj = shapely.geometry.mapping(shapely.wkb.loads(str(v)))

        return obj


class AreaCreate(AreaBase):
    pass
