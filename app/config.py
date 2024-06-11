from pydantic import model_validator
from pydantic_settings import BaseSettings
from functools import lru_cache
import sys


class Config(BaseSettings):
    API_V1_PREFIX: str = "/v1"
    DEFAULT_SUBMISSION_FPS: int = 15
    FILENAME_CLASS_TO_COLOR: str = "class_to_color.json"
    FILENAME_PERCENTAGE_COVERS: str = "percentage_covers.json"

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

    # Key to prefix to all assets in the S3 bucket. Should be distinct to the
    # deployment as to avoid conflicts
    S3_PREFIX: str

    # Kubernetes
    NAMESPACE: str
    PROJECT: str
    KUBECONFIG: str = "/app/.kube/config.yaml"
    DEEPREEFMAP_IMAGE: str
    DEEPREEFMAP_IMAGE_TAG: str

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
