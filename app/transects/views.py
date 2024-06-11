from app.transects.models import (
    TransectRead,
    Transect,
    TransectCreate,
    TransectUpdate,
)
from app.db import get_session, AsyncSession
from fastapi import (
    Depends,
    APIRouter,
    Query,
    Response,
    HTTPException,
    Request,
    Header,
)
from uuid import UUID
from app.crud import CRUD
from typing import Annotated

router = APIRouter()
crud = CRUD(Transect, TransectRead, TransectCreate, TransectUpdate)


async def get_count(
    response: Response,
    filter: str = Query(None),
    range: str = Query(None),
    sort: str = Query(None),
    user_id: UUID = Header(...),
    user_is_admin: bool = Header(...),
    session: AsyncSession = Depends(get_session),
):
    count = await crud.get_total_count(
        response=response,
        sort=sort,
        range=range,
        filter=filter,
        session=session,
        user_id=user_id,
        user_is_admin=user_is_admin,
    )

    return count


async def get_data(
    filter: str = Query(None),
    sort: str = Query(None),
    range: str = Query(None),
    user_id: UUID = Header(...),
    user_is_admin: bool = Header(...),
    session: AsyncSession = Depends(get_session),
):

    res = await crud.get_model_data(
        sort=sort,
        range=range,
        filter=filter,
        session=session,
        user_id=user_id,
        user_is_admin=user_is_admin,
    )

    return res


async def get_one(
    transect_id: UUID,
    user_id: UUID = Header(...),
    user_is_admin: bool = Header(...),
    session: AsyncSession = Depends(get_session),
):
    res = await crud.get_model_by_id(
        model_id=transect_id,
        session=session,
        user_id=user_id,
        user_is_admin=user_is_admin,
    )

    if not res:
        raise HTTPException(
            status_code=404, detail=f"ID: {transect_id} not found"
        )
    return res


@router.get("/{transect_id}", response_model=TransectRead)
async def get_transect(
    obj: CRUD = Depends(get_one),
) -> TransectRead:
    """Get a transect by id"""

    return obj


@router.get("", response_model=list[TransectRead])
async def get_all_transects(
    request: Request,
    response: Response,
    transects: CRUD = Depends(get_data),
    total_count: int = Depends(get_count),
) -> list[TransectRead]:
    """Get all transect data"""

    return transects


@router.post("", response_model=TransectRead)
async def create_transect(
    response: Response,
    transect: TransectCreate,
    user_id: UUID = Header(...),
    session: AsyncSession = Depends(get_session),
) -> TransectRead:
    """Creates a transect data record"""

    obj = Transect.model_validate(transect)
    obj.owner = user_id
    session.add(obj)

    await session.commit()
    await session.refresh(obj)

    return obj


@router.put("/{transect_id}", response_model=TransectRead)
async def update_transect(
    transect_update: TransectUpdate,
    *,
    transect: TransectRead = Depends(get_one),
    session: AsyncSession = Depends(get_session),
) -> TransectRead:
    """Update a transect by id"""

    update_data = transect_update.model_dump(exclude_unset=True)
    transect.sqlmodel_update(update_data)

    session.add(transect)
    await session.commit()
    await session.refresh(transect)

    return transect


@router.delete("/{transect_id}")
async def delete_transect(
    transect: TransectRead = Depends(get_one),
    session: AsyncSession = Depends(get_session),
) -> None:
    """Delete a transect by id"""

    await session.delete(transect)
    await session.commit()

    return {"ok": True}
