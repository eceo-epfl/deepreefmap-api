import pytest
from typing import Generator, AsyncGenerator
from fastapi.testclient import TestClient
from uuid import uuid4
from app.transects.models import Transect, TransectCreate, TransectUpdate


@pytest.mark.asyncio
async def test_get_transect(
    client: Generator[TestClient, None, None],
    async_session: AsyncGenerator,
) -> None:
    """Test permissions of get one endpoint

    Test whether a non-admin user can get only their transect by id and not
    others
    """

    # Create a non-admin user
    user_id = uuid4()

    client.post(
        "/transect",
        json=TransectCreate(
            user_id=user_id,
            name="Test Transect",
            description="Test Transect Description",
            longitude_start=0.0,
            latitude_start=0.0,
            longitude_end=2.0,
            latitude_end=2.0,
        ).model_dump(),
    )
