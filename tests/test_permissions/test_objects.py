import pytest
from uuid import uuid4
from app.config import config
from app.objects.models import InputObject
from app.users.models import User

ROUTE = f"{config.API_PREFIX}/objects"


@pytest.mark.asyncio
async def test_get_object_different_non_admin_user(
    test_user_one, client_two_user, modified_async_session
):
    # Create video object straight into DB to avoid checks on a real video
    input_object = InputObject(
        owner=test_user_one.id,
        name="Test Video",
        description="Test Video Description",
        status="completed",
        input_type="video",
        input_metadata={"duration": 10},
    )
    modified_async_session.add(input_object)
    await modified_async_session.commit()
    await modified_async_session.refresh(input_object)

    res_retrieved = await client_two_user.get(
        f"{ROUTE}/{input_object.id}",
    )

    # Expect 404 as user is not admin nor the same user
    assert res_retrieved.status_code == 404


@pytest.mark.asyncio
async def test_get_object_same_user_non_admin(
    test_user_one, client_one_user, modified_async_session
):
    # Create video object straight into DB to avoid checks on a real video
    input_object = InputObject(
        owner=test_user_one.id,
        name="Test Video",
        description="Test Video Description",
        status="completed",
        input_type="video",
        input_metadata={"duration": 10},
    )
    modified_async_session.add(input_object)
    await modified_async_session.commit()
    await modified_async_session.refresh(input_object)

    res_retrieved = await client_one_user.get(
        f"{ROUTE}/{input_object.id}",
    )
    print(res_retrieved, f"{ROUTE}/{input_object.id.hex}", res_retrieved.text)

    assert res_retrieved.status_code == 200
    assert res_retrieved.json()["id"] == str(input_object.id)


@pytest.mark.asyncio
async def test_get_object_admin_user(
    client_three_admin, modified_async_session
):
    user_id = uuid4().hex  # Different user to test suite
    user = User(
        id=user_id,
        is_admin=False,
        username="test_user",
        email="localtest@example.com",
        first_name="Test",
        last_name="User",
        realm_roles=["user"],
        client_roles=["user"],
    )

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

    res_retrieved = await client_three_admin.get(
        f"{ROUTE}/{input_object.id}",
    )

    # Expect 200 as data is visible by admin
    assert res_retrieved.status_code == 200
