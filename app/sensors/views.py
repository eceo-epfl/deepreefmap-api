from fastapi import Depends, FastAPI, APIRouter, Query, Response
from sqlmodel import select, Session
from app.db import get_session, AsyncSession
from app.sensors.models import (
    Sensor,
    SensorCreate,
    SensorRead,
    SensorReadWithData,
)
from app.areas.models import AreaRead
from uuid import UUID, uuid4
from sqlalchemy import func
import json

router = APIRouter()


@router.get("/{sensor_id}", response_model=SensorReadWithData)
async def get_sensor(
    session: AsyncSession = Depends(get_session),
    *,
    sensor_id: UUID,
) -> SensorRead:
    """Get an area by id"""
    res = await session.execute(select(Sensor).where(Sensor.id == sensor_id))
    sensor = res.scalars().one_or_none()

    return sensor


@router.get("", response_model=list[SensorRead])
async def get_sensors(
    response: Response,
    session: AsyncSession = Depends(get_session),
    *,
    filter: str = Query(None),
    sort: str = Query(None),
    range: str = Query(None),
):
    """Get all areas"""
    sort = json.loads(sort) if sort else []
    range = json.loads(range) if range else []
    filter = json.loads(filter) if filter else {}

    # Do a query to satisfy total count for "Content-Range" header
    count_query = await session.execute(select(func.count(Sensor.iterator)))
    total_count = count_query.scalar_one()

    query = select(Sensor)

    # Order by sort field params ie. ["name","ASC"]
    if len(sort) == 2:
        sort_field, sort_order = sort
        if sort_order == "ASC":
            query = query.order_by(getattr(Sensor, sort_field))
        else:
            query = query.order_by(getattr(Sensor, sort_field).desc())

    if len(range) == 2:
        start, end = range
        query = query.offset(start).limit(end - start + 1)
    else:
        start, end = [0, total_count]  # For content-range header

    # Filter by filter field params ie. {"name":"bar"}
    if len(filter):
        for field, value in filter.items():
            if field == "id" or field == "area_id":
                query = query.where(getattr(Sensor, field) == value)
            else:
                query = query.where(getattr(Sensor, field).like(f"%{value}%"))

    # Execute query
    results = await session.execute(query)
    sensors = results.scalars().all()

    response.headers["Content-Range"] = f"sensors {start}-{end}/{total_count}"

    return sensors
