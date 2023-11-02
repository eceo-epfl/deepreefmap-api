from typing import Union

from fastapi import FastAPI, status
from fastapi.middleware.cors import CORSMiddleware
from config import config
from models.config import KeycloakConfig
from models.health import HealthCheck
from areas.models import Area
from db import init_db

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
    ## Perform a Health Check
    Endpoint to perform a healthcheck on. This endpoint can primarily be used Docker
    to ensure a robust container orchestration and management is in place. Other
    services which rely on proper functioning of the API service will not deploy if this
    endpoint returns any other HTTP status code except 200 (OK).
    Returns:
        HealthCheck: Returns a JSON response with the health status
    """
    return HealthCheck(status="OK")


from areas.views import router as areas_router

app.include_router(areas_router, prefix="/areas", tags=["areas"])
