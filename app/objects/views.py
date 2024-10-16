from fastapi import (
    Depends,
    APIRouter,
    Request,
    Query,
    BackgroundTasks,
    Header,
    Response,
    HTTPException,
)
from fastapi.responses import JSONResponse
from app.db import get_session, AsyncSession
from app.objects.models import InputObject, InputObjectRead, InputObjectUpdate
from app.objects.service import get_s3
from app.config import config
from uuid import UUID
from sqlmodel import select, update
from sqlalchemy import func
from typing import Any
from aioboto3 import Session as S3Session
from app.objects.utils import (
    generate_video_statistics,
    delete_incomplete_object,
)
import datetime
import json
from app.users.models import User
from app.auth.services import get_user_info, get_payload
from app.objects import hooks

router = APIRouter()


@router.post("/hooks")
async def handle_file_upload(
    request: Request,
    session: AsyncSession = Depends(get_session),
    s3: S3Session = Depends(get_s3),
    *,
    background: BackgroundTasks,
):
    # Read the JSON payload from the request
    payload = await request.json()
    try:
        # Extracting the JWT auth token
        auth_token = payload["Event"]["HTTPRequest"]["Header"][
            "Authorization"
        ][0].split("Bearer ")[1]
        user = get_user_info(get_payload(auth_token))
    except Exception:
        raise HTTPException(
            detail="Unauthorized",
            status_code=401,
        )

    if payload["Type"] == "pre-create":
        return await hooks.pre_create(session, user, payload)

    if payload["Type"] == "post-receive":
        return await hooks.post_receive(session, user, payload)

    if payload["Type"] == "post-create":
        return await hooks.post_create(session, user, payload, s3, background)

    if payload["Type"] == "pre-finish":
        return await hooks.pre_finish(session, user, payload)

    if payload["Type"] == "post-finish":
        return await hooks.post_finish(session, user, payload, s3, background)


@router.get("/{object_id}", response_model=InputObjectRead)
async def get_object(
    user: User = Depends(get_user_info),
    session: AsyncSession = Depends(get_session),
    s3: S3Session = Depends(get_s3),
    *,
    object_id: UUID,
) -> InputObjectRead:
    """Get an object by id"""

    query = select(InputObject).where(InputObject.id == object_id)
    if not user.is_admin:
        query = query.where(InputObject.owner == user.id)
    res = await session.exec(query)
    obj = res.one_or_none()

    if not obj:
        raise HTTPException(status_code=404, detail="Object not found")

    return obj


@router.post("/{object_id}", response_model=Any)
async def regenerate_statistics(
    user: User = Depends(get_user_info),
    session: AsyncSession = Depends(get_session),
    s3: S3Session = Depends(get_s3),
    *,
    object_id: UUID,
    background_tasks: BackgroundTasks,
) -> InputObjectRead:
    """Get an object by id"""

    query = select(InputObject).where(InputObject.id == str(object_id))
    if not user.is_admin:
        query = query.where(InputObject.owner == user.id)
    res = await session.exec(query)
    obj = res.one_or_none()

    # Remove fields from vid statistics and set completed to false

    update_query = (
        update(InputObject)
        .where(InputObject.id == object_id)
        .values(
            fps=None,
            time_seconds=None,
            frame_count=None,
            processing_completed_successfully=False,
        )
    )
    await session.exec(update_query)
    await session.commit()
    if not obj:
        raise HTTPException(status_code=404, detail="Object not found")

    # Create background task to generate video statistics
    background_tasks.add_task(generate_video_statistics, obj.id, s3, session)

    return True


@router.get("", response_model=list[InputObjectRead])
async def get_objects(
    response: Response,
    user: User = Depends(get_user_info),
    session: AsyncSession = Depends(get_session),
    *,
    filter: str = Query(None),
    sort: str = Query(None),
    range: str = Query(None),
) -> list[InputObjectRead]:
    """Get all objects"""

    sort = json.loads(sort) if sort else []
    range = json.loads(range) if range else []
    filter = json.loads(filter) if filter else {}

    # Do a query to satisfy total count for "Content-Range" header
    count_query = select(func.count(InputObject.iterator))
    if not user.is_admin:
        count_query = count_query.where(InputObject.owner == user.id)

    if len(filter):  # Have to filter twice for some reason? SQLModel state?
        for field, value in filter.items():
            if field == "id" or field == "object_id":
                if isinstance(value, list):
                    for v in value:
                        count_query = count_query.filter(
                            getattr(InputObject, field) == v
                        )
                else:
                    count_query = count_query.filter(
                        getattr(InputObject, field) == value
                    )
            elif isinstance(value, bool):
                count_query = count_query.filter(
                    getattr(InputObject, field) == value
                )
            else:
                count_query = count_query.filter(
                    getattr(InputObject, field).like(f"%{str(value)}%")
                )
    total_count = await session.exec(count_query)
    total_count = total_count.one()

    # Query for the quantity of records in SensorInventoryData that match the
    # sensor as well as the min and max of the time column
    query = select(InputObject)
    if not user.is_admin:
        query = query.where(InputObject.owner == user.id)

    # Order by sort field params ie. ["name","ASC"]
    if len(sort) == 2:
        sort_field, sort_order = sort
        if sort_order == "ASC":
            query = query.order_by(getattr(InputObject, sort_field))
        else:
            query = query.order_by(getattr(InputObject, sort_field).desc())

    # Filter by filter field params ie. {"name":"bar"}
    if len(filter):
        for field, value in filter.items():
            if field == "id" or field == "object_id":
                if isinstance(value, list):
                    for v in value:
                        query = query.filter(getattr(InputObject, field) == v)
                else:
                    query = query.filter(getattr(InputObject, field) == value)
            elif isinstance(value, bool):
                query = query.filter(getattr(InputObject, field) == value)
            else:
                query = query.filter(
                    getattr(InputObject, field).like(f"%{str(value)}%")
                )

    if len(range) == 2:
        start, end = range
        query = query.offset(start).limit(end - start + 1)
    else:
        start, end = [0, total_count]  # For content-range header

    # Execute query
    results = await session.exec(query)
    objects = results.all()

    object_objs = [InputObjectRead.model_validate(x) for x in objects]

    response.headers["Content-Range"] = f"objects {start}-{end}/{total_count}"

    return object_objs


@router.put("/{input_object_id}", response_model=InputObjectRead)
async def update_input_object(
    input_object_id: UUID,
    input_object_update: InputObjectUpdate,
    user: User = Depends(get_user_info),
    session: AsyncSession = Depends(get_session),
) -> InputObjectRead:

    query = select(InputObject).where(InputObject.id == input_object_id)
    if not user.is_admin:
        query = query.where(InputObject.owner == user.id)

    res = await session.exec(query)
    obj = res.one_or_none()
    if not obj:
        raise HTTPException(status_code=404, detail="Input Object not found")

    data = input_object_update.model_dump(exclude_unset=True)

    # Update the fields from the request
    for field, value in data.items():
        setattr(obj, field, value)

    session.add(obj)
    await session.commit()
    await session.refresh(obj)

    return obj


@router.delete("/{object_id}")
async def delete_object(
    object_id: UUID,
    user: User = Depends(get_user_info),
    session: AsyncSession = Depends(get_session),
    filter: dict[str, str] | None = None,
    s3: S3Session = Depends(get_s3),
) -> None:
    """Delete an object by id"""

    query = select(InputObject).where(InputObject.id == object_id)
    if not user.is_admin:
        query = query.where(InputObject.owner == user.id)
    res = await session.exec(query)
    object = res.one_or_none()

    if not object:
        raise HTTPException(
            status_code=404,
            detail="Object not found",
        )
    if object:
        # Delete object from S3
        try:
            res = await s3.delete_object(
                Bucket=config.S3_BUCKET_ID,
                Key=f"{config.S3_PREFIX}/{object.id}/inputs/{object.filename}",
            )
        except Exception as e:
            raise HTTPException(
                status_code=500,
                detail=f"Failed to delete object from S3: {e}",
            )

        await session.delete(object)
        await session.commit()
