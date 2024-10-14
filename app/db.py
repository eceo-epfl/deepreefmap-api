from sqlalchemy.ext.asyncio import create_async_engine
from sqlmodel.ext.asyncio.session import AsyncSession
from sqlalchemy.orm import sessionmaker
from app.config import config
from typing import AsyncGenerator
from cashews import cache


engine = create_async_engine(
    config.DB_URL, echo=False, future=True, pool_pre_ping=True
)
async_session = sessionmaker(
    engine, class_=AsyncSession, expire_on_commit=False
)


async def get_session() -> AsyncGenerator[AsyncSession, None]:
    async with async_session() as session:
        yield session


# Cache setup
cache.setup(
    f"redis://{config.CACHE_URL}:{config.CACHE_PORT}/",
    db=config.CACHE_DB,
    enable=config.CACHE_ENABLED,
    # suppress=True,
    # client_side=True,
    # socket_connect_timeout=0.1,
    # retry_on_timeout=False,
)
