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

router = APIRouter()


@router.post("")
async def upload_file(
    upload_length: int = Header(..., alias="Upload-Length"),
    content_type: str = Header(..., alias="Content-Type"),
    session: AsyncSession = Depends(get_session),
    s3: S3Session = Depends(get_s3),
    *,
    background_tasks: BackgroundTasks,
) -> str:
    # Handle chunked file upload
    try:
        object = InputObject(
            size_bytes=upload_length,
            all_parts_received=False,
            last_part_received_utc=datetime.datetime.now(),
            processing_message="Upload started",
        )
        session.add(object)

        await session.commit()
        await session.refresh(object)
        print("OBJECT", object)

        key = f"{config.S3_PREFIX}/inputs/{str(object.id)}"
        # Create multipart upload and add the upload id to the object
        response = await s3.create_multipart_upload(
            Bucket=config.S3_BUCKET_ID,
            Key=key,
        )

        # Wait for the response to return the upload id
        object.upload_id = response["UploadId"]
        await session.commit()

        # Create a worker to monitor stale uploads and delete if outside
        # of threshold in config
        background_tasks.add_task(
            delete_incomplete_object, object.id, s3, session
        )

    except Exception as e:
        print(e)
        await session.rollback()
        raise HTTPException(
            status_code=500,
            detail="Failed to create object",
        )

    return str(object.id)


@router.patch("")
async def upload_chunk(
    request: Request,
    patch: str = Query(...),
    s3: S3Session = Depends(get_s3),
    session: AsyncSession = Depends(get_session),
    upload_offset: int = Header(..., alias="Upload-Offset"),
    upload_length: int = Header(..., alias="Upload-Length"),
    upload_name: str = Header(..., alias="Upload-Name"),
    content_type: str = Header(..., alias="Content-Type"),
    content_length: int = Header(..., alias="Content-Length"),
    *,
    background_tasks: BackgroundTasks,
):
    """Handle chunked file upload"""

    # Clean " from patch
    patch = patch.replace('"', "")

    # Get the object prefix from the DB
    query = select(InputObject).where(InputObject.id == patch)
    res = await session.exec(query)
    object = res.one_or_none()
    if not object:
        raise HTTPException(
            status_code=404,
            detail="Object not found",
        )

    final_part = False
    # Get the part number from the offset
    if upload_length - upload_offset == content_length:
        # The last object is probably not the same size as the other parts, so
        # we need to check if the part number is the last part
        last_part = object.parts[-1]
        part_number = last_part["PartNumber"] + 1
    else:
        # Calculate the part number from the division of the offset by the part
        # size (content-length) from the upload length
        part_number = (int(upload_offset) // int(content_length)) + 1

    if upload_offset + content_length == upload_length:
        final_part = True

    # Upload the chunk to S3
    key = f"{config.S3_PREFIX}/inputs/{str(object.id)}"

    data = await request.body()

    print(
        f"Working on part number {part_number}, chunk {upload_offset} "
        f"{int(upload_offset)+int(content_length)} "
        f"of {upload_length} bytes "
        f"({int(upload_offset)/int(upload_length)*100}%)"
    )
    try:
        part = await s3.upload_part(
            Bucket=config.S3_BUCKET_ID,
            Key=key,
            UploadId=object.upload_id,
            PartNumber=part_number,
            Body=data,
        )
        if object.parts is None or not object.parts:
            object.parts = []

        object.parts += [
            {
                "PartNumber": part_number,
                "ETag": part["ETag"],
                "Size": content_length,
                "Offset": upload_offset,
                "Length": content_length,
            }
        ]
        update_query = (
            update(InputObject)
            .where(InputObject.id == object.id)
            .values(
                parts=object.parts,
                filename=upload_name,
                last_part_received_utc=datetime.datetime.now(),
            )
        )
        await session.exec(update_query)
        await session.commit()

        if final_part:
            # Complete the multipart upload

            # Simplify the parts list to only include the PartNumber and ETag
            parts_list = [
                {"PartNumber": x["PartNumber"], "ETag": x["ETag"]}
                for x in object.parts
            ]
            res = await s3.complete_multipart_upload(
                Bucket=config.S3_BUCKET_ID,
                Key=key,
                UploadId=object.upload_id,
                MultipartUpload={"Parts": parts_list},
            )

            update_query = (
                update(InputObject)
                .where(InputObject.id == object.id)
                .values(
                    last_part_received_utc=datetime.datetime.now(),
                    all_parts_received=True,
                )
            )
            await session.exec(update_query)
            await session.commit()

            # Create background task to generate video statistics
            background_tasks.add_task(
                generate_video_statistics, object.id, s3, session
            )

    except Exception as e:
        print(e)
        raise HTTPException(
            status_code=500,
            detail=f"Failed to upload file to S3: {e}",
        )

    await session.refresh(object)
    return object


@router.head("")
async def check_uploaded_chunks(
    response: Response,
    patch: str = Query(...),
    session: AsyncSession = Depends(get_session),
):
    """Responds with the offset of the next expected chunk"""
    # Get the InputObject from the DB
    # Clean " from patch
    patch = patch.replace('"', "")
    query = select(InputObject).where(InputObject.id == patch)
    res = await session.exec(query)
    object = res.one_or_none()
    if not object:
        raise HTTPException(
            status_code=404,
            detail="Object not found",
        )

    # Calculate the next expected offset
    next_expected_offset = 0
    if object.parts:
        last_part = object.parts[-1]
        next_expected_offset = last_part["Offset"] + last_part["Length"]

    # Return headers with Upload-Offset
    response.headers["Upload-Offset"] = str(next_expected_offset)

    return


@router.get("/{object_id}", response_model=InputObjectRead)
async def get_object(
    session: AsyncSession = Depends(get_session),
    s3: S3Session = Depends(get_s3),
    *,
    object_id: UUID,
) -> InputObjectRead:
    """Get an object by id"""

    query = select(InputObject).where(InputObject.id == object_id)
    res = await session.exec(query)
    obj = res.one_or_none()

    return obj


@router.post("/{object_id}", response_model=Any)
async def regenerate_statistics(
    session: AsyncSession = Depends(get_session),
    s3: S3Session = Depends(get_s3),
    *,
    object_id: UUID,
    background_tasks: BackgroundTasks,
) -> InputObjectRead:
    """Get an object by id"""

    query = select(InputObject).where(InputObject.id == str(object_id))
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
    session: AsyncSession = Depends(get_session),
) -> InputObjectRead:

    res = await session.exec(
        select(InputObject).where(InputObject.id == input_object_id)
    )
    obj = res.one_or_none()
    if not obj:
        raise HTTPException(status_code=404, detail="Input Object not found")

    data = input_object_update.model_dump(exclude_unset=True)

    # Update the fields from the request
    for field, value in data.items():
        print(f"Updating: {field}, {value}")
        setattr(obj, field, value)

    session.add(obj)
    await session.commit()
    await session.refresh(obj)

    return obj


@router.delete("/{object_id}")
async def delete_object(
    object_id: UUID,
    session: AsyncSession = Depends(get_session),
    filter: dict[str, str] | None = None,
    s3: S3Session = Depends(get_s3),
) -> None:
    """Delete an object by id"""
    res = await session.exec(
        select(InputObject).where(InputObject.id == object_id)
    )
    object = res.one_or_none()

    if object:
        # Delete object from S3
        print(f"DELETING OBJECT FROM S3: {object.id}")
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
