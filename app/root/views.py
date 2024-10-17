from app.db import get_session, AsyncSession
from sqlalchemy.sql import text
from app.submissions.k8s import get_kubernetes_status
from app.auth.models import KeycloakConfig
from app.config import config
from app.objects.service import get_s3_status
from fastapi import (
    status,
    Depends,
    HTTPException,
    APIRouter,
    Request,
    BackgroundTasks,
)
from fastapi.responses import JSONResponse
from app.root.models import HealthCheck
from app.objects import hooks
from app.auth.services import get_user_info, get_payload
from app.objects.service import get_s3
from aioboto3 import Session as S3Session

router = APIRouter()


@router.get(f"{config.API_PREFIX}/config")
async def get_keycloak_config() -> KeycloakConfig:
    return KeycloakConfig(
        clientId=config.KEYCLOAK_CLIENT_ID,
        realm=config.KEYCLOAK_REALM,
        url=config.KEYCLOAK_URL,
        deployment=config.DEPLOYMENT,
    )


@router.get(
    "/healthz",
    tags=["healthcheck"],
    summary="Perform a Health Check",
    response_description="Return HTTP Status Code 200 (OK)",
    status_code=status.HTTP_200_OK,
    response_model=HealthCheck,
)
async def get_health(
    session: AsyncSession = Depends(get_session),
) -> HealthCheck:
    """
    Endpoint to perform a healthcheck on for kubernetes liveness and
    readiness probes.
    """
    # Execute DB Query to check DB connection
    await session.exec(text("SELECT 1"))

    # Query kubernetes API to check RCP:RunAI connection
    _, k8s = await get_kubernetes_status()
    _, s3 = await get_s3_status()

    if not k8s or not s3:
        raise HTTPException(
            status_code=status.HTTP_503_SERVICE_UNAVAILABLE,
            detail="Kubernetes or S3 service is not available",
        )
    return HealthCheck(status="OK")


@router.post("/tus_hooks")
async def handle_file_upload(
    request: Request,
    session: AsyncSession = Depends(get_session),
    s3: S3Session = Depends(get_s3),
    *,
    background: BackgroundTasks,
):
    # Read the JSON payload from the request
    payload = await request.json()
    print(payload)
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

    return JSONResponse(
        content={"message": "No hook found for this event type"},
        status_code=404,
    )
