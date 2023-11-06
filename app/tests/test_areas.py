from fastapi.testclient import TestClient
from sqlmodel import Session, SQLModel, create_engine
from app.db import init_db
from app.main import app
import pytest
from uuid import UUID


@pytest.mark.asyncio
async def test_create_area():
    # Some code here omitted, we will see it later 👈
    client = TestClient(app)
    await init_db()

    response = client.post(
        "/areas", json={"name": "Binntal", "description": "Binntal area"}
    )
    # Some code here omitted, we will see it later 👈
    data = response.json()

    assert response.status_code == 200

    assert data["name"] == "Binntal"
    assert data["description"] == "Binntal area"
    assert UUID(data["id"], version=4)
