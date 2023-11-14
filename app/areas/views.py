from fastapi import Depends, APIRouter, Query, Response, Body
from sqlmodel import select
from app.db import get_session, AsyncSession
from app.areas.models import Area, AreaCreate, AreaRead, AreaUpdate
from uuid import UUID
from sqlalchemy import func
import json

router = APIRouter()


@router.get("/{area_id}", response_model=AreaRead)
async def get_area(
    session: AsyncSession = Depends(get_session),
    *,
    area_id: UUID,
) -> AreaRead:
    """Get an area by id"""
    res = await session.execute(select(Area).where(Area.id == area_id))
    area = res.scalars().one_or_none()

    return area


@router.get("", response_model=list[AreaRead])
async def get_areas(
    response: Response,
    filter: str = Query(None),
    sort: str = Query(None),
    range: str = Query(None),
    session: AsyncSession = Depends(get_session),
):
    """Get all areas"""

    sort = json.loads(sort) if sort else []
    range = json.loads(range) if range else []
    filter = json.loads(filter) if filter else {}

    query = select(Area)

    # Do a query to satisfy total count for "Content-Range" header
    count_query = select(func.count(Area.iterator))
    if len(filter):  # Have to filter twice for some reason? SQLModel state?
        for field, value in filter.items():
            for qry in [query, count_query]:  # Apply filter to both queries
                if isinstance(value, list):
                    qry = qry.where(getattr(Area, field).in_(value))
                elif field == "id" or field == "area_id":
                    qry = qry.where(getattr(Area, field) == value)
                else:
                    qry = qry.where(getattr(Area, field).like(f"%{value}%"))

    # Execute total count query (including filter)
    total_count_query = await session.execute(count_query)
    total_count = total_count_query.scalar_one()

    # Order by sort field params ie. ["name","ASC"]
    if len(sort) == 2:
        sort_field, sort_order = sort
        if sort_order == "ASC":
            query = query.order_by(getattr(Area, sort_field))
        else:
            query = query.order_by(getattr(Area, sort_field).desc())

    # Filter by filter field params ie. {"name":"bar"}
    if len(filter):
        for field, value in filter.items():
            if isinstance(value, list):
                query = query.where(getattr(Area, field).in_(value))
            elif field == "id" or field == "area_id":
                query = query.where(getattr(Area, field) == value)
            else:
                query = query.where(getattr(Area, field).like(f"%{value}%"))

    if len(range) == 2:
        start, end = range
        query = query.offset(start).limit(end - start + 1)
    else:
        start, end = [0, total_count]  # For content-range header

    # Execute query
    results = await session.execute(query)
    areas = results.scalars().all()

    response.headers["Content-Range"] = f"areas {start}-{end}/{total_count}"

    return areas


@router.post("", response_model=AreaRead)
async def create_area(
    area: AreaCreate = Body(...),
    session: AsyncSession = Depends(get_session),
) -> AreaRead:
    """Creates an area"""
    print(area)
    area = Area.from_orm(area)
    session.add(area)
    await session.commit()
    await session.refresh(area)

    return area


@router.put("/{area_id}", response_model=AreaRead)
async def update_area(
    area_id: UUID,
    area_update: AreaUpdate,
    session: AsyncSession = Depends(get_session),
) -> AreaRead:
    res = await session.execute(select(Area).where(Area.id == area_id))
    area = res.one()

    # Update the fields from the request
    for field, value in area.items():
        if field in area_update.json:
            setattr(area, field, area_update.json[field])

    await session.add(area)
    await session.commit()
    await session.refresh(area)

    return area


@router.delete("/{area_id}")
async def delete_area(
    area_id: UUID,
    session: AsyncSession = Depends(get_session),
    filter: dict[str, str] | None = None,
) -> None:
    """Delete an area by id"""
    res = await session.execute(select(Area).where(Area.id == area_id))
    area = res.scalars().one_or_none()

    if area:
        await session.delete(area)
        await session.commit()
