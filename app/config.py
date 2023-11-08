from pydantic import BaseSettings
from functools import lru_cache


class Config(BaseSettings):
    API_V1_PREFIX = "/v1"

    # PostGIS settings
    DB_HOST: str = "localhost"
    DB_PORT: int = 5432
    DB_USER: str = "postgres"
    DB_PASSWORD: str = "psql"

    DB_NAME: str = "postgres"
    DB_PREFIX: str = "postgresql+asyncpg"

    # Form the DB URL
    DB_URL: str = (
        f"{DB_PREFIX}://{DB_USER}:{DB_PASSWORD}@{DB_HOST}:{DB_PORT}/{DB_NAME}"
    )


@lru_cache()
def get_config():
    return Config()


config = get_config()
