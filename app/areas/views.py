from fastapi import Depends, FastAPI, APIRouter
from sqlmodel import select, Session
from main import app
from db import get_session, AsyncSession
from areas.models import Area, AreaCreate, AreaRead

router = APIRouter()


@router.get("/", response_model=list[AreaRead])
async def get_areas(session: AsyncSession = Depends(get_session)):
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
