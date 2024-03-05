from pydantic import root_validator
from pydantic_settings import BaseSettings
from functools import lru_cache


class Config(BaseSettings):
    API_V1_PREFIX: str = "/v1"

    # PostGIS settings
    DB_HOST: str
    DB_PORT: int  # 5432
    DB_USER: str
    DB_PASSWORD: str

    DB_NAME: str  # postgres
    DB_PREFIX: str  # "postgresql+asyncpg"

    DB_URL: str | None = None

    # S3 settings
    S3_URL: str
    S3_BUCKET_ID: str
    S3_ACCESS_KEY: str
    S3_SECRET_KEY: str

    # Key to prefix to all assets in the S3 bucket. Should be distinct to the
    # deployment as to avoid conflicts
    S3_PREFIX: str

    # Kubernetes
    NAMESPACE: str = "runai-enacit-deepreefmap-dev"

    @root_validator(pre=True)
    def form_db_url(cls, values: dict) -> dict:
        """Form the DB URL from the settings"""
        if "DB_URL" not in values:
            values["DB_URL"] = (
                "{prefix}://{user}:{password}@{host}:{port}/{db}".format(
                    prefix=values["DB_PREFIX"],
                    user=values["DB_USER"],
                    password=values["DB_PASSWORD"],
                    host=values["DB_HOST"],
                    port=values["DB_PORT"],
                    db=values["DB_NAME"],
                )
            )
        return values


@lru_cache()
def get_config():
    return Config()


config = get_config()
