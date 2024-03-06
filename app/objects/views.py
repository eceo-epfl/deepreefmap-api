from fastapi import Depends, APIRouter, HTTPException
from app.db import get_session, AsyncSession
from app.objects.models import (
    InputObject,
    InputObjectRead,
)
from app.objects.service import get_s3, S3Connection
from app.config import config
from fastapi import File, UploadFile

router = APIRouter()


@router.post("/inputs", response_model=InputObjectRead)
async def create_object(
    files: list[UploadFile] = File(...),
    session: AsyncSession = Depends(get_session),
    s3: S3Connection = Depends(get_s3),
) -> None:
    """Creates an input object record from one or more video files"""

    if len(files) == 0:
        raise HTTPException(
            status_code=400,
            detail="At least one file must be provided",
        )
    # Check that there are distinct filenames in the list of files
    filenames = [file.filename for file in files]
    if len(filenames) != len(set(filenames)):
        raise HTTPException(
            status_code=400,
            detail="All files must have distinct filenames",
        )

    # Create new object DB object with no fields
    object = InputObject()

    session.add(object)
    await session.commit()
    await session.refresh(object)

    # Use the generated DB object ID to create a prefix for the S3 bucket
    prefix = f"{config.S3_PREFIX}/{object.id}/inputs"
    try:
        for file_obj in files:
            content = await file_obj.read()

            # Write bytes to S3 bucket
            response = s3.session.put_object(
                Bucket=config.S3_BUCKET_ID,
                Key=f"{prefix}/{file_obj.filename}",
                Body=content,
            )

            # Validate that response is 200: OK
            if response["ResponseMetadata"]["HTTPStatusCode"] != 200:
                # Error in uploading, raise exception
                raise Exception(
                    "Failed to upload file to S3: "
                    f"{response['ResponseMetadata']}"
                )

    except Exception as e:
        await session.delete(object)
        await session.commit()
        s3.session.delete_objects(
            Bucket=config.S3_BUCKET_ID,
            Delete={"Objects": [{"Key": f"{prefix}/{file_obj.filename}"}]},
        )
        raise HTTPException(
            status_code=500,
            detail=f"Failed to upload file to S3: {e}",
        )

    return object
