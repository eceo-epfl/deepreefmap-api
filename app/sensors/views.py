from fastapi import Depends, APIRouter, Query, Response, HTTPException, Body
from sqlmodel import select
from app.db import get_session, AsyncSession
from app.sensors.models import (
    Sensor,
    SensorRead,
    SensorReadWithData,
    SensorUpdate,
    SensorCreate,
)
from uuid import UUID
from sqlalchemy import func
import json
import base64

router = APIRouter()


@router.get("/{sensor_id}", response_model=SensorReadWithData)
async def get_sensor(
    session: AsyncSession = Depends(get_session),
    *,
    sensor_id: UUID,
) -> SensorRead:
    """Get an sensor by id"""
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
    """Get all sensors"""
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


@router.post("", response_model=SensorRead)
async def create_sensor(
    sensor: SensorCreate = Body(...),
    session: AsyncSession = Depends(get_session),
) -> SensorRead:
    """Creates an sensor"""
    print(sensor)
    sensor = Sensor.from_orm(sensor)
    session.add(sensor)
    await session.commit()
    await session.refresh(sensor)

    return sensor


def decode_base64_to_csv(value: str) -> bytes:
    """Decode base64 string to csv bytes"""
    # Split the string using the comma as a delimiter
    data_parts = value.split(",")

    # Extract the data type and base64-encoded content
    if "text/csv" not in data_parts[0]:
        raise HTTPException(
            status_code=400,
            detail="Data type not supported, must be text/csv",
        )
    base64_content = data_parts[1]
    rawdata = base64.b64decode(base64_content)
    import csv

    # Treat the rawdata as a CSV file, read in the rows
    decoded = []
    for row in csv.reader(rawdata.decode("utf-8").splitlines()):
        decoded.append(row)

    return decoded


@router.put("/{sensor_id}", response_model=SensorRead)
async def update_sensor(
    sensor_id: UUID,
    sensor_update: SensorUpdate,
    session: AsyncSession = Depends(get_session),
) -> SensorRead:
    res = await session.execute(select(Sensor).where(Sensor.id == sensor_id))
    sensor_db = res.scalars().one()
    sensor_data = sensor_update.dict(exclude_unset=True)
    if not sensor_db:
        raise HTTPException(status_code=404, detail="Sensor not found")

    # Update the fields from the request
    for field, value in sensor_data.items():
        if field in ["latitude", "longitude"]:
            # Don't process lat/lon, it's converted to geom in model validator
            continue
        if field == "instrumentdata":
            # Convert base64 to bytes, input should be csv, read and add rows
            # to sensor_data table with sensor_id
            rows = decode_base64_to_csv(value)
            print(rows)

        print(f"Updating: {field}, {value}")
        setattr(sensor_db, field, value)

    session.add(sensor_db)
    await session.commit()
    await session.refresh(sensor_db)

    return sensor_db


@router.delete("/{sensor_id}")
async def delete_sensor(
    sensor_id: UUID,
    session: AsyncSession = Depends(get_session),
    filter: dict[str, str] | None = None,
) -> None:
    """Delete an sensor by id"""
    res = await session.execute(select(Sensor).where(Sensor.id == sensor_id))
    sensor = res.scalars().one_or_none()

    if sensor:
        await session.delete(sensor)
        await session.commit()
