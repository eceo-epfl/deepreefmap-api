from sqlmodel import SQLModel, Field, Column, Relationship
from geoalchemy2 import Geometry, WKBElement
from uuid import uuid4, UUID
from typing import Any
from pydantic import validator
import shapely
from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from app.areas.models import Area


class SensorBase(SQLModel):
    name: str = Field(default=None, index=True)
    description: str


class Sensor(SensorBase, table=True):
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
    geom: Any = Field(sa_column=Column(Geometry("POINT", srid=4326)))

    area_id: UUID = Field(default=None, foreign_key="area.uuid")
    area: "Area" = Relationship(back_populates="sensors")


class SensorRead(SensorBase):
    id: UUID
    geom: Any
    area_id: UUID

    @validator("geom")
    def convert_wkb_to_json(cls, v: WKBElement) -> Any:
        """Convert the WKBElement to a shapely mapping"""
        print("EVAN", str(v))
        if isinstance(v, WKBElement):
            return shapely.geometry.mapping(shapely.wkb.loads(str(v)))
        else:
            return v


class SensorCreate(SensorBase):
    pass
