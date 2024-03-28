from fastapi import FastAPI, status, Depends
from fastapi.middleware.cors import CORSMiddleware
from app.config import config
from app.submissions.views import router as submissions_router
from app.objects.views import router as objects_router
from app.status.views import router as status_router
from pydantic import BaseModel
from app.db import get_session, AsyncSession
from sqlalchemy.sql import text

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
) -> HealthCheck:
    """
    Endpoint to perform a healthcheck on for kubernetes liveness and
    readiness probes.
    """
    # Execute DB Query to check DB connection
    await session.exec(text("SELECT 1"))

    return HealthCheck(status="OK")


# Routes for Deep Reef Map
app.include_router(
    submissions_router,
    prefix=f"{config.API_V1_PREFIX}/submissions",
    tags=["submissions"],
)
app.include_router(
    objects_router,
    prefix=f"{config.API_V1_PREFIX}/objects",
    tags=["objects"],
)
app.include_router(
    status_router,
    prefix=f"{config.API_V1_PREFIX}/status",
    tags=["status"],
)
