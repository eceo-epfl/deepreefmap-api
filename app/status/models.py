from sqlmodel import SQLModel
from typing import Any


class S3Status(SQLModel):
    total_object_count: int = 0
    input_object_count: int = 0
    output_object_count: int = 0
    total_size: int = 0
    input_size: int = 0
    output_size: int = 0


class StatusRead(SQLModel):
    kubernetes: list[Any]
    s3_local: S3Status | None = None
    s3_global: S3Status | None = None
