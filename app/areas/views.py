from fastapi import Depends, FastAPI, APIRouter, Query
from sqlmodel import select, Session
from app.db import get_session, AsyncSession
from app.areas.models import Area, AreaCreate, AreaRead
from uuid import UUID

router = APIRouter()


@router.get("/{area_id}", response_model=AreaRead)
async def get_area(
    session: AsyncSession = Depends(get_session),
    sort: list[str] | None = None,
    range: list[int] | None = None,
    filter: dict[str, str] | None = None,
) -> AreaRead:
    pass


@router.get("/", response_model=list[AreaRead])
async def get_areas(
    session: AsyncSession = Depends(get_session),
):
    """Get all areas"""

    result = await session.execute(select(Area))
    areas = result.scalars().all()

    return [
        AreaRead(
            id=area.uuid,
            name=area.name,
            description=area.description,
        )
        for area in areas
    ]


@router.post("/", response_model=AreaRead)
async def create_area(
    area: AreaCreate,
    session: AsyncSession = Depends(get_session),
) -> AreaRead:
    """Creates an area"""

    area = Area(name=area.name, description=area.description)
    session.add(area)
    await session.commit()
    await session.refresh(area)

    return AreaRead(
        name=area.name,
        description=area.description,
        id=area.uuid,
    )


@router.put("/{area_id}", response_model=AreaRead)
async def update_area(
    area_id: UUID,
    session: AsyncSession = Depends(get_session),
) -> AreaRead:
    res = await session.execute(select(Area).where(Area.uuid == area_id))
    area = res.one()

    # Update area
    area.name = area.name
    area.description = area.description

    await session.add(area)
    await session.commit()
    await session.refresh(area)

    return AreaRead(
        name=area.name,
        description=area.description,
        id=area.uuid,
    )


@router.put("/", response_model=list[AreaRead])
async def update_areas(
    session: AsyncSession = Depends(get_session),
    filter: dict[str, str] | None = None,
) -> list[AreaRead]:
    pass


@router.delete("/")
async def delete_areas(
    session: AsyncSession = Depends(get_session),
) -> None:
    pass


@router.delete("/{area_id}", response_model=AreaRead)
async def delete_area(
    session: AsyncSession = Depends(get_session),
    filter: dict[str, str] | None = None,
) -> None:
    pass
