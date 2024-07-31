from fastapi import APIRouter, Query, Response
from typing import Any
from app.config import config
from kubernetes import client, config as k8s_config
from kubernetes.client import CoreV1Api, ApiClient
from app.submissions.k8s import (
    get_k8s_v1,
)
from fastapi import Depends
from app.status.models import StatusRead, S3Status
from app.objects.service import get_s3
from aioboto3 import Session as S3Session

router = APIRouter()


@router.get("", response_model=StatusRead)
async def get_jobs(
    k8s: CoreV1Api = Depends(get_k8s_v1),
    s3: S3Session = Depends(get_s3),
) -> Any:
    """Get all kubernetes jobs in the namespace"""

    # Get k8s jobs
    ret = k8s.list_namespaced_pod(config.NAMESPACE)
    api = ApiClient()
    k8s_jobs = api.sanitize_for_serialization(ret.items)

    # Get total usage (items, size) for inputs, outputs
    s3_local = S3Status()
    response = await s3.list_objects_v2(
        Bucket=config.S3_BUCKET_ID,
        Prefix=f"{config.S3_PREFIX}/inputs/",
    )

    s3_local.input_object_count = len(response.get("Contents"))
    for obj in response.get("Contents"):
        s3_local.input_size += obj.get("Size")

    response = await s3.list_objects_v2(
        Bucket=config.S3_BUCKET_ID,
        Prefix=f"{config.S3_PREFIX}/outputs/",
    )
    s3_local.output_object_count = len(response.get("Contents"))
    for obj in response.get("Contents"):
        s3_local.output_size += obj.get("Size")

    response = await s3.list_objects_v2(
        Bucket=config.S3_BUCKET_ID,
        Prefix=f"{config.S3_PREFIX}/",
    )
    s3_local.total_object_count = len(response.get("Contents"))
    for obj in response.get("Contents"):
        s3_local.total_size += obj.get("Size")

    obj = StatusRead(kubernetes=k8s_jobs, s3_local=s3_local)

    return obj
