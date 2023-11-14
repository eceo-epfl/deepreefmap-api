from fastapi import Depends, APIRouter, Query, Response
from sqlmodel import select
from app.db import get_session, AsyncSession
from app.sensors.models import (
    Sensor,
    SensorRead,
    SensorReadWithData,
)
from uuid import UUID
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
    count_query = select(func.count(Sensor.iterator))
    if len(filter):  # Have to filter twice for some reason? SQLModel state?
        for field, value in filter.items():
            if field == "id" or field == "area_id":
                count_query = count_query.filter(
                    getattr(Sensor, field) == value
                )
            else:
                count_query = count_query.filter(
                    getattr(Sensor, field).like(f"%{str(value)}%")
                )
    total_count = await session.execute(count_query)
    total_count = total_count.scalar_one()

    query = select(Sensor)

    # Order by sort field params ie. ["name","ASC"]
    if len(sort) == 2:
        sort_field, sort_order = sort
        if sort_order == "ASC":
            query = query.order_by(getattr(Sensor, sort_field))
        else:
            query = query.order_by(getattr(Sensor, sort_field).desc())

    # Filter by filter field params ie. {"name":"bar"}
    if len(filter):
        for field, value in filter.items():
            if field == "id" or field == "area_id":
                query = query.filter(getattr(Sensor, field) == value)
            else:
                query = query.filter(
                    getattr(Sensor, field).like(f"%{str(value)}%")
                )

    if len(range) == 2:
        start, end = range
        query = query.offset(start).limit(end - start + 1)
    else:
        start, end = [0, total_count]  # For content-range header

    # Execute query
    results = await session.execute(query)
    sensors = results.scalars().all()

    response.headers["Content-Range"] = f"sensors {start}-{end}/{total_count}"

    return sensors
