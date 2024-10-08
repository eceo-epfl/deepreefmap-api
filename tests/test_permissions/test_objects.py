import pytest
from uuid import uuid4
from app.config import config
from app.objects.models import InputObject
from app.users.models import User

ROUTE = f"{config.API_PREFIX}/objects"


@pytest.mark.asyncio
async def test_get_submission(client, modified_async_session):

    user_id = uuid4().hex
    user = User(id=user_id, is_admin=False)

    # Create video object straight into DB to avoid checks on a real video
    input_object = InputObject(
        owner=user.id,
        name="Test Video",
        description="Test Video Description",
        status="completed",
        input_type="video",
        input_metadata={"duration": 10},
    )
    modified_async_session.add(input_object)
    await modified_async_session.commit()
    await modified_async_session.refresh(input_object)

    res_retrieved = await client.get(
        f"{ROUTE}/{input_object.id.hex}",
    )

    assert res_retrieved.status_code == 200  # or the expected status code
    assert res_retrieved.json()["id"] == str(input_object.id)

    res_retrieved_different_user = await client.get(
        f"{ROUTE}/{input_object.id.hex}",
        headers={"User-Id": uuid4().hex, "User-Is-Admin": "false"},
    )

    assert (
        res_retrieved_different_user.status_code == 404
    ), "Object was retrieved by a different user"
    assert (
        res_retrieved_different_user.json()["detail"] == "Object not found"
    ), res_retrieved_different_user.text

    # Get transect as an admin
    res_retrieved_admin = await client.get(
        f"{ROUTE}/{input_object.id.hex}",
        headers={"User-Id": user_id, "User-Is-Admin": "true"},
    )

    assert res_retrieved_admin.status_code == 200
