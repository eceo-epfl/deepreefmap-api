from pydantic import BaseModel, field_validator
import datetime


class S3Object(BaseModel):
    filename: str
    last_updated: datetime.datetime
    size_bytes: int
    hash_md5sum: str

    @field_validator("filename")
    def form_filename(cls, value: str) -> str:
        return value.split("/")[-1]

    @field_validator("hash_md5sum")
    def remove_quotes(cls, value: str) -> str:
        return value.replace('"', "")
