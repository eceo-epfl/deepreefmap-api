from pydantic import model_validator
from pydantic_settings import BaseSettings
from functools import lru_cache
import sys
import httpx
from enum import Enum


class DeploymentType(str, Enum):
    LOCAL = "local"
    DEV = "dev"
    STAGE = "stage"
    PROD = "prod"


class Config(BaseSettings):
    API_PREFIX: str = "/api"
    DEFAULT_SUBMISSION_FPS: int = 15
    FILENAME_CLASS_TO_COLOR: str = "class_to_color.json"
    FILENAME_PERCENTAGE_COVERS: str = "percentage_covers.json"
    DEPLOYMENT: DeploymentType

    # PostGIS settings
    DB_HOST: str | None = None
    DB_PORT: int | None = 5432
    DB_USER: str | None = None
    DB_PASSWORD: str | None = None

    DB_NAME: str | None = None  # postgres
    DB_PREFIX: str | None = None  # "postgresql+asyncpg"

    DB_URL: str | None = None

    # S3 settings
    S3_URL: str
    S3_BUCKET_ID: str
    S3_ACCESS_KEY: str
    S3_SECRET_KEY: str
    INCOMPLETE_OBJECT_TIMEOUT_SECONDS: int
    INCOMPLETE_OBJECT_CHECK_INTERVAL: int
    OBJECT_CONSIDER_ABANDONED: int = 90

    # Key to prefix to all assets in the S3 bucket. Should be distinct to the
    # deployment as to avoid conflicts
    S3_PREFIX: str

    # Kubernetes
    NAMESPACE: str
    PROJECT: str
    KUBECONFIG: str = "/app/.kube/config.yaml"
    DEEPREEFMAP_IMAGE: str
    DEEPREEFMAP_IMAGE_TAG: str

    # Keycloak
    KEYCLOAK_REALM: str
    KEYCLOAK_URL: str
    KEYCLOAK_API_ID: str
    KEYCLOAK_API_SECRET: str
    KEYCLOAK_CLIENT_ID: str  # The UI client ID that gets req'd for frontend

    # Used for serializing and deserializing tokens for downloading files
    SERIALIZER_SECRET_KEY: str
    SERIALIZER_EXPIRY_HOURS: int = 6

    TIMEOUT: httpx.Timeout = httpx.Timeout(
        5.0,
        connect=2.0,
    )
    LIMITS: httpx.Limits = httpx.Limits(
        max_connections=500, max_keepalive_connections=50
    )

    # Redis cache
    CACHE_ENABLED: bool = True
    CACHE_URL: str
    CACHE_PORT: int
    CACHE_TTL: int = 3600
    CACHE_DB: int = 0

    VALID_ROLES: list[str] = ["admin", "user"]

    @model_validator(mode="after")
    @classmethod
    def form_db_url(cls, values: dict) -> dict:
        """Form the DB URL from the settings"""
        if not values.DB_URL:
            values.DB_URL = (
                "{prefix}://{user}:{password}@{host}:{port}/{db}".format(
                    prefix=values.DB_PREFIX,
                    user=values.DB_USER,
                    password=values.DB_PASSWORD,
                    host=values.DB_HOST,
                    port=values.DB_PORT,
                    db=values.DB_NAME,
                )
            )
        return values


@lru_cache()
def get_config():
    return Config()


config = get_config()
