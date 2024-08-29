import pytest
from uuid import uuid4
from app.config import config


@pytest.mark.asyncio
async def test_get_transect(client):
    user_id = uuid4().hex
    res_created = await client.post(
        f"{config.API_PREFIX}/transects",
        json={
            "owner": user_id,
            "name": "Test Transect",
            "description": "Test Transect Description",
            "longitude_start": 0.0,
            "latitude_start": 0.0,
            "longitude_end": 2.0,
            "latitude_end": 2.0,
        },
        headers={"User-Id": user_id, "User-Is-Admin": "false"},
    )
    assert res_created.status_code == 200  # or the expected status code

    res_retrieved = await client.get(
        f"{config.API_PREFIX}/transects/{res_created.json()['id']}",
        headers={"User-Id": user_id, "User-Is-Admin": "false"},
    )

    assert res_retrieved.status_code == 200  # or the expected status code
    assert res_retrieved.json()["id"] == res_created.json()["id"]

    res_retrieved_different_user = await client.get(
        f"{config.API_PREFIX}/transects/{res_created.json()['id']}",
        headers={"User-Id": uuid4().hex, "User-Is-Admin": "false"},
    )

    assert res_retrieved_different_user.status_code == 404
    assert (
        res_retrieved_different_user.json()["detail"]
        == f"ID: {res_created.json()['id']} not found"
    ), "Transect was retrieved by a different user"

    # Get transect as an admin
    res_retrieved_admin = await client.get(
        f"{config.API_PREFIX}/transects/{res_created.json()['id']}",
        headers={"User-Id": user_id, "User-Is-Admin": "true"},
    )

    assert res_retrieved_admin.status_code == 200
