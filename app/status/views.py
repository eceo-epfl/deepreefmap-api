from fastapi import APIRouter, Query, Response
from typing import Any
from app.config import config
from kubernetes import client, config as k8s_config

router = APIRouter()


@router.get("/kubernetes/jobs", response_model=Any)
async def get_jobs(
    response: Response,
    *,
    filter: str = Query(None),
    sort: str = Query(None),
    range: str = Query(None),
) -> Any:
    """Get all kubernetes jobs in the namespace"""

    k8s_config.load_kube_config()
    v1 = client.CoreV1Api()
    ret = v1.list_namespaced_pod(config.NAMESPACE)

    return ret.items
