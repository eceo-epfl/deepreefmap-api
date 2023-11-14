from sqlmodel import SQLModel, Field, Column, UniqueConstraint, Relationship
from geoalchemy2 import Geometry, WKBElement
from uuid import uuid4, UUID
from typing import Any
import shapely
from pydantic import validator
from typing import TYPE_CHECKING
from typing import List
from app.sensors.models import SensorRead

if TYPE_CHECKING:
    from app.sensors.models import Sensor


class AreaBase(SQLModel):
    name: str = Field(default=None, index=True)
    description: str


class Area(AreaBase, table=True):
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
    geom: Any = Field(sa_column=Column(Geometry("POLYGON", srid=4326)))

    sensors: list["Sensor"] = Relationship(
        back_populates="area", sa_relationship_kwargs={"lazy": "selectin"}
    )


class AreaRead(AreaBase):
    id: UUID  # We use the UUID as the return ID
    geom: Any
    sensors: List["SensorRead"]

    @validator("geom")
    def convert_wkb_to_json(cls, v: WKBElement) -> Any:
        """Convert the WKBElement to a shapely mapping"""
        if isinstance(v, WKBElement):
            return shapely.geometry.mapping(shapely.wkb.loads(str(v)))
        else:
            return v


class AreaCreate(AreaBase):
    geom: list[tuple[float, float]]

    @validator("geom")
    def convert_json_to_wkt(cls, v: list[float]) -> Any:
        """Convert the WKBElement to a shapely mapping"""
        if isinstance(v, list):
            polygon = shapely.geometry.Polygon(v)
            oriented_polygon = shapely.geometry.polygon.orient(
                polygon, sign=1.0
            )
            return oriented_polygon.wkt
        else:
            return v


class AreaUpdate(AreaBase):
    pass
