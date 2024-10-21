from fastapi import APIRouter
from typing import Any
from app.submissions.k8s import get_kubernetes_status
from fastapi import Depends
from app.status.models import StatusRead, S3Status
from app.objects.service import get_s3_status
from app.users.models import User
from app.auth.services import get_user_info
from app.db import get_session, AsyncSession

router = APIRouter()


@router.get("", response_model=StatusRead)
async def get_jobs(
    user: User = Depends(get_user_info),
    session: AsyncSession = Depends(get_session),
) -> Any:
    """Get all kubernetes jobs in the namespace"""
    k8s_jobs, kubernetes_status = await get_kubernetes_status(session)
    s3_local, s3_status = await get_s3_status()

    obj = StatusRead(
        kubernetes=k8s_jobs if user.is_admin else [],
        s3_local=s3_local if user.is_admin else S3Status(),
        s3_status=s3_status,
        kubernetes_status=kubernetes_status,
    )

    return obj
