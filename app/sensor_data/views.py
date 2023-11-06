from fastapi import Depends, FastAPI, APIRouter, Query
from sqlmodel import select, Session
from main import app
from db import get_session, AsyncSession
from areas.models import Area, AreaCreate, AreaRead

router = APIRouter()


@router.get("/{area_id}", response_model=AreaRead)
async def get_area(
    session: AsyncSession = Depends(get_session),
    sort: list[str] | None = Query(None),
    range: list[int] | None = Query(None),
    filter: dict[str, str] | None = Query(None),
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
            id=area.id,
            uuid=area.uuid,
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
        uuid=area.uuid,
        id=area.id,
    )


@router.put("/{area_id}", response_model=AreaRead)
async def update_area(
    session: AsyncSession = Depends(get_session),
) -> AreaRead:
    pass


@router.put("/", response_model=list[AreaRead])
async def update_areas(
    session: AsyncSession = Depends(get_session),
    filter: dict[str, str] | None = Query(None),
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
    filter: dict[str, str] | None = Query(None),
) -> None:
    pass
