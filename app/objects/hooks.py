from app.users.models import User
from app.objects.models import InputObject
from app.config import config
from fastapi import HTTPException, status, BackgroundTasks
from fastapi.responses import JSONResponse
from sqlalchemy.orm import Session
from sqlmodel import update
from app.objects.utils import (
    generate_video_statistics,
    delete_incomplete_object,
)
from aioboto3 import Session as S3Session
import datetime


async def pre_create(
    session: Session,
    user: User,
    payload: dict,
) -> JSONResponse:

    # Extracting the TransectID, filename, size and filetype from metadata
    transect_id = payload["Event"]["HTTPRequest"]["Header"]["Transect-Id"][0]
    filename = payload["Event"]["Upload"]["MetaData"]["filename"]
    filetype = payload["Event"]["Upload"]["MetaData"]["filetype"]

    if filetype not in config.ALLOWED_FILETYPES:
        raise HTTPException(
            status_code=status.HTTP_400_BAD_REQUEST,
            detail="Filetype not allowed",
        )
    size_in_bytes = payload["Event"]["Upload"]["Size"]

    object = InputObject(
        filename=filename,
        size_bytes=size_in_bytes,
        all_parts_received=False,
        last_part_received_utc=datetime.datetime.now(),
        processing_message="Upload initiated",
        transect_id=transect_id if transect_id else None,
        owner=user.id,
    )
    session.add(object)

    await session.commit()
    await session.refresh(object)

    s3_key = f"{config.INPUT_FOLDER_PREFIX}{str(object.id)}"

    # Respond with a custom ID for tusd to upload to s3
    return JSONResponse(
        status_code=200,
        content={"ChangeFileInfo": {"ID": s3_key}},
    )


async def post_receive(
    session: Session,
    user: User,
    payload: dict,
) -> JSONResponse:
    upload_id = payload["Event"]["Upload"]["ID"]

    # Split the UUID out of the upload_id to get the object ID.
    # Also the + separator between UUID and TUSd upload ID
    object_id = upload_id.split(config.INPUT_FOLDER_PREFIX)[1].split("+")[0]
    size_in_bytes = payload["Event"]["Upload"]["Size"]
    offset = payload["Event"]["Upload"]["Offset"]

    # Calculate the total uploaded percentage
    uploaded_percentage = (offset / size_in_bytes) * 100

    # Print the uploaded percentage
    print(f"Upload progress: {uploaded_percentage:.2f}%")

    # Update the object
    update_query = (
        update(InputObject)
        .where(InputObject.id == object_id)
        .values(
            processing_message=f"Upload progress: {uploaded_percentage:.2f}%",
            last_part_received_utc=datetime.datetime.now(),
        )
    )
    await session.exec(update_query)
    await session.commit()

    # Respond with a 200 to acknowledge the request
    return JSONResponse(
        status_code=200, content={"message": "Upload progress received"}
    )


async def pre_finish(
    session: Session,
    user: User,
    payload: dict,
) -> JSONResponse:
    upload_id = payload["Event"]["Upload"]["ID"]

    # Split the UUID out of the upload_id to get the object ID.
    # Also the + separator between UUID and TUSd upload ID
    object_id = upload_id.split(config.INPUT_FOLDER_PREFIX)[1].split("+")[0]

    # Update the object
    update_query = (
        update(InputObject)
        .where(InputObject.id == object_id)
        .values(
            processing_message="Upload completed",
            all_parts_received=True,
            last_part_received_utc=datetime.datetime.now(),
        )
    )
    await session.exec(update_query)
    await session.commit()

    # Respond with a 200 to acknowledge the request
    return JSONResponse(
        status_code=200, content={"message": "Upload completed"}
    )


async def post_create(
    session: Session,
    user: User,
    payload: dict,
    s3: S3Session,
    background_tasks: BackgroundTasks,
) -> JSONResponse:
    upload_id = payload["Event"]["Upload"]["ID"]

    # Split the UUID out of the upload_id to get the object ID.
    # Also the + separator between UUID and TUSd upload ID
    object_id = upload_id.split(config.INPUT_FOLDER_PREFIX)[1].split("+")[0]

    # Update the object
    update_query = (
        update(InputObject)
        .where(InputObject.id == object_id)
        .values(
            processing_message="Upload started",
            last_part_received_utc=datetime.datetime.now(),
        )
    )
    await session.exec(update_query)
    await session.commit()
    background_tasks.add_task(delete_incomplete_object, object_id, s3, session)
    # Respond with a 200 to acknowledge the request
    return JSONResponse(status_code=200, content={"message": "Upload started"})


async def post_finish(
    session: Session,
    user: User,
    payload: dict,
    s3: S3Session,
    background_tasks: BackgroundTasks,
) -> JSONResponse:
    upload_id = payload["Event"]["Upload"]["ID"]

    # Split the UUID out of the upload_id to get the object ID.
    # Also the + separator between UUID and TUSd upload ID
    object_id = upload_id.split(config.INPUT_FOLDER_PREFIX)[1].split("+")[0]

    # Update the object
    update_query = (
        update(InputObject)
        .where(InputObject.id == object_id)
        .values(
            processing_message="Upload completed",
            all_parts_received=True,
            last_part_received_utc=datetime.datetime.now(),
        )
    )
    await session.exec(update_query)
    await session.commit()

    background_tasks.add_task(
        generate_video_statistics, object_id, s3, session
    )
    # Respond with a 200 to acknowledge the request
    return JSONResponse(
        status_code=200, content={"message": "Upload completed"}
    )
