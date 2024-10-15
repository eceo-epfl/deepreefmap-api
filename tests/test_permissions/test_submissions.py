# import pytest
# from uuid import uuid4
# from app.config import config
# from app.objects.models import InputObject

# ROUTE = f"{config.API_PREFIX}/submissions"


# @pytest.mark.asyncio
# async def test_get_submission(client, modified_async_session):
#     user_id = uuid4().hex

#     # Create video object straight into DB to avoid checks on a real video
#     input_object = InputObject(
#         owner=user_id,
#         name="Test Video",
#         description="Test Video Description",
#         status="completed",
#         input_type="video",
#         input_metadata={"duration": 10},
#     )
#     modified_async_session.add(input_object)
#     await modified_async_session.commit()
#     await modified_async_session.refresh(input_object)

#     # Create submission
#     res_created = await client.post(
#         f"{ROUTE}",
#         json={
#             "owner": user_id,
#             "name": "Test Submission",
#             "description": "Test Submission Description",
#             "status": "completed",
#             "input_associations": [{"input_object_id": input_object.id.hex}],
#         },
#         headers={"User-Id": user_id, "User-Is-Admin": "false"},
#     )
#     assert res_created.status_code == 200, res_created.text

#     res_retrieved = await client.get(
#         f"{ROUTE}/{res_created.json()['id']}",
#         headers={"User-Id": user_id, "User-Is-Admin": "false"},
#     )

#     assert res_retrieved.status_code == 200  # or the expected status code
#     assert res_retrieved.json()["id"] == res_created.json()["id"]

#     res_retrieved_different_user = await client.get(
#         f"{ROUTE}/{res_created.json()['id']}",
#         headers={"User-Id": uuid4().hex, "User-Is-Admin": "false"},
#     )

#     assert (
#         res_retrieved_different_user.status_code == 404
#     ), "Object was retrieved by a different user"
#     assert (
#         res_retrieved_different_user.json()["detail"] == "Submission not found"
#     ), res_retrieved_different_user.text

#     # Get transect as an admin
#     res_retrieved_admin = await client.get(
#         f"{ROUTE}/{res_created.json()['id']}",
#         headers={"User-Id": user_id, "User-Is-Admin": "true"},
#     )

#     assert res_retrieved_admin.status_code == 200
