from fastapi import Depends, APIRouter, HTTPException, Request, Query
from app.db import get_session, AsyncSession
from app.objects.models import (
    InputObject,
    InputObjectRead,
    InputObjectUpdate,
)
from app.objects.service import get_s3
from app.config import config
from fastapi import File, UploadFile
from uuid import UUID
from fastapi import Depends, APIRouter, Query, Response, HTTPException, Body
from sqlmodel import select, update
from app.db import get_session, AsyncSession
from uuid import UUID
from sqlalchemy import func
import json
from typing import Any
from aioboto3 import Session as S3Session
from app.config import config
from fastapi import File, UploadFile, Header
from kubernetes import client, config as k8s_config
import os
import asyncio

router = APIRouter()


@router.post("/upload")
async def upload_file(
    upload_length: int = Header(..., alias="Upload-Length"),
    content_type: str = Header(..., alias="Content-Type"),
    session: AsyncSession = Depends(get_session),
    s3: S3Session = Depends(get_s3),
) -> str:
    # Handle chunked file upload
    try:
        object = InputObject(size_bytes=upload_length)
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
        print("RESPONSE", response)
        # Wait for the response to return the upload id
        # await asyncio.sleep(1)
        object.upload_id = response["UploadId"]
        await session.commit()
    except Exception as e:
        print(e)
        await session.rollback()
        raise HTTPException(
            status_code=500,
            detail="Failed to create object",
        )

    return str(object.id)


@router.patch("/upload")
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
        f"of {upload_length} bytes ({int(upload_offset)/int(upload_length)*100}%)"
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
            .values(parts=object.parts, filename=upload_name)
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
                .values(hash_md5sum=res["ETag"].strip('"'))
            )
            await session.exec(update_query)
            await session.commit()

            print("COMPLETED UPLOAD", res)
    except Exception as e:
        print(e)
        raise HTTPException(
            status_code=500,
            detail=f"Failed to upload file to S3: {e}",
        )

    await session.refresh(object)
    return object


@router.head("/upload")
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
    print("NEXT OFFSET", next_expected_offset)

    # Return headers with Upload-Offset
    response.headers["Upload-Offset"] = str(next_expected_offset)

    return


@router.delete("/upload")
async def delete_upload(
    body: str = Body(...),
    session: AsyncSession = Depends(get_session),
    s3: S3Session = Depends(get_s3),
):
    """Delete the upload object from S3 and the database"""
    object_id = body.replace('"', "")

    # Get object from DB
    query = select(InputObject).where(InputObject.id == object_id)
    res = await session.exec(query)
    object = res.one_or_none()
    if not object:
        raise HTTPException(
            status_code=404,
            detail="Object not found",
        )

    # Delete chunked object from S3
    await s3.abort_multipart_upload(
        Bucket=config.S3_BUCKET_ID,
        Key=f"{config.S3_PREFIX}/inputs/{object.id}",
        UploadId=object.upload_id,
    )


# async def delete_object(
#     object_id: UUID,
#     session: AsyncSession = Depends(get_session),
#     filter: dict[str, str] | None = None,
#     s3: S3Session = Depends(get_s3),
# ) -> None:
#     """Delete an object by id"""
#     res = await session.exec(
#         select(InputObject).where(InputObject.id == object_id)
#     )
#     object = res.one_or_none()

#     if object:
#         # Delete object from S3
#         print(f"DELETING OBJECT FROM S3: {object.id}")
#         try:
#             res = await s3.delete_object(
#                 Bucket=config.S3_BUCKET_ID,
#                 Key=f"{config.S3_PREFIX}/{object.id}/inputs/{object.filename}",
#             )
#         except Exception as e:
#             raise HTTPException(
#                 status_code=500,
#                 detail=f"Failed to delete object from S3: {e}",
#             )

#         await session.delete(object)
#         await session.commit()

#     return res


@router.post("/inputs", response_model=InputObjectRead)
async def create_object(
    file: UploadFile = File(...),
    session: AsyncSession = Depends(get_session),
    s3: S3Session = Depends(get_s3),
) -> None:
    """Creates an input object record from one or more video files"""

    # Create new object DB object with no fields
    object = InputObject(filename=file.filename)

    session.add(object)
    await session.commit()
    await session.refresh(object)

    # Use the generated DB object ID to create a prefix for the S3 bucket
    prefix = f"{config.S3_PREFIX}/inputs"
    try:
        content = await file.read()

        # Write bytes to S3 bucket
        response = await s3.put_object(
            Bucket=config.S3_BUCKET_ID,
            Key=f"{prefix}/{object.id}",
            Body=content,
        )
        print(response)
        for key, val in response.items():
            print(f"Key: {key}, Value: {val}")
        # Validate that response is 200: OK
        if response["ResponseMetadata"]["HTTPStatusCode"] != 200:
            # Error in uploading, raise exception
            raise Exception(
                "Failed to upload file to S3: "
                f"{response['ResponseMetadata']}"
            )
        object.hash_md5sum = response["ETag"].strip('"')
        object.size_bytes = len(content)
        await session.commit()

    except Exception as e:
        print(f"Failed to upload file to S3: {e}")
        await session.delete(object)
        await session.commit()
        await s3.delete_objects(
            Bucket=config.S3_BUCKET_ID,
            Delete={"Objects": [{"Key": f"{prefix}/{object.id}"}]},
        )
        raise HTTPException(
            status_code=500,
            detail=f"Failed to upload file to S3: {e}",
        )

    return object


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
    object = res.one_or_none()
    object_dict = object.model_dump() if object else {}

    res = await session.exec(query)
    return InputObjectRead(**object_dict)


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
                        count_query = count_query.filter(
                            getattr(InputObject, field) == v
                        )
                else:
                    count_query = count_query.filter(
                        getattr(InputObject, field) == value
                    )
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
