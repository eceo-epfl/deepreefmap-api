from typing import Union

from fastapi import FastAPI, status
from fastapi.middleware.cors import CORSMiddleware
from app.config import config
from app.areas.models import Area
from app.db import init_db
from app.areas.views import router as areas_router
from pydantic import BaseModel

app = FastAPI()


@app.on_event("startup")
async def on_startup():
    await init_db()


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
def get_health() -> HealthCheck:
    """
    Endpoint to perform a healthcheck on for kubenernetes liveness and
    readiness probes.
    """
    return HealthCheck(status="OK")


app.include_router(
    areas_router,
    prefix=f"{config.API_V1_PREFIX}/areas",
    tags=["areas"],
)
