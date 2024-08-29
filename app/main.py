from fastapi import FastAPI, status, Depends
from fastapi.middleware.cors import CORSMiddleware
from app.config import config
from pydantic import BaseModel
from app.db import get_session, AsyncSession
from sqlalchemy.sql import text
from app.submissions.k8s import get_k8s_v1
from kubernetes.client import CoreV1Api
from app.auth.models import KeycloakConfig

from app.submissions.views import (
    router as submissions_router,
    job_log_router as submission_job_logs_router,
)
from app.objects.views import router as objects_router
from app.status.views import router as status_router
from app.transects.views import router as transects_router
from app.users.views import router as users_router

app = FastAPI()

origins = ["*"]

app.add_middleware(
    CORSMiddleware,
    allow_origins=origins,
    allow_credentials=True,
    allow_methods=["*"],
    allow_headers=["*"],
)


class HealthCheck(BaseModel):
    """Response model to validate and return when performing a health check."""

    status: str = "OK"


@app.get(f"{config.API_PREFIX}/config/keycloak")
async def get_keycloak_config() -> KeycloakConfig:
    return KeycloakConfig(
        clientId=config.KEYCLOAK_CLIENT_ID,
        realm=config.KEYCLOAK_REALM,
        url=config.KEYCLOAK_URL,
    )


@app.get(
    "/healthz",
    tags=["healthcheck"],
    summary="Perform a Health Check",
    response_description="Return HTTP Status Code 200 (OK)",
    status_code=status.HTTP_200_OK,
    response_model=HealthCheck,
)
async def get_health(
    session: AsyncSession = Depends(get_session),
    k8s: CoreV1Api = Depends(get_k8s_v1),
) -> HealthCheck:
    """
    Endpoint to perform a healthcheck on for kubernetes liveness and
    readiness probes.
    """
    # Execute DB Query to check DB connection
    await session.exec(text("SELECT 1"))

    # Query kubernetes API to check RCP:RunAI connection
    k8s.list_namespaced_pod(config.NAMESPACE)

    return HealthCheck(status="OK")


# Routes for Deep Reef Map
app.include_router(
    submissions_router,
    prefix=f"{config.API_PREFIX}/submissions",
    tags=["submissions"],
)
app.include_router(
    objects_router,
    prefix=f"{config.API_PREFIX}/objects",
    tags=["objects"],
)
app.include_router(
    status_router,
    prefix=f"{config.API_PREFIX}/status",
    tags=["status"],
)
app.include_router(
    transects_router,
    prefix=f"{config.API_PREFIX}/transects",
    tags=["transects"],
)
app.include_router(
    users_router,
    prefix=f"{config.API_PREFIX}/users",
    tags=["users"],
)
app.include_router(
    submission_job_logs_router,
    prefix=f"{config.API_PREFIX}/submission_job_logs",
    tags=["submissions", "logs"],
)
