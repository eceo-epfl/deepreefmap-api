from pydantic import BaseSettings
from functools import lru_cache


class Config(BaseSettings):
    API_V1_PREFIX = "/api/v1"


@lru_cache()
def get_config():
    return Config()


config = get_config()
