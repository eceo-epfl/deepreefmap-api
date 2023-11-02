from fastapi import Depends, FastAPI, APIRouter
from sqlmodel import select, Session
from main import app
from db import get_session, AsyncSession
from areas.models import Area, AreaCreate, AreaRead

router = APIRouter()


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
async def add_area(
    area: AreaCreate,
    session: AsyncSession = Depends(get_session),
) -> AreaRead:
    """Adds an area to the database"""

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
