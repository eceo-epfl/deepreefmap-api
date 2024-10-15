# import pytest
# from uuid import uuid4
# from app.config import config
# from app.transects.models import Transect
# from app.users.models import User
# from geoalchemy2 import WKTElement

# ROUTE = f"{config.API_PREFIX}/transects"


# @pytest.mark.asyncio
# async def test_get_transect_same_user_non_admin(
#     test_user_one, client_one_user, modified_async_session
# ):
#     # Create transect object directly in DB
#     transect = Transect(
#         owner=test_user_one.id,
#         name="Test Transect",
#         description="Test Transect Description",
#         geom=WKTElement("LINESTRING(0 0, 2 2)", srid=4326),
#     )
#     modified_async_session.add(transect)
#     await modified_async_session.commit()
#     await modified_async_session.refresh(transect)

#     print(transect)
#     print(transect.id.hex, str(transect.id))

#     res_retrieved = await client_one_user.get(f"{ROUTE}/{transect.id.hex}")
#     print(res_retrieved.json())
#     assert res_retrieved.status_code == 200
#     assert res_retrieved.json()["id"] == str(transect.id)


# @pytest.mark.asyncio
# async def test_get_transect_different_non_admin_user(
#     test_user_one, client_two_user, modified_async_session
# ):
#     # Create transect object directly in DB
#     transect = Transect(
#         owner=test_user_one.id,
#         name="Test Transect",
#         description="Test Transect Description",
#         geom=WKTElement("LINESTRING(0 0, 2 2)", srid=4326),
#     )
#     modified_async_session.add(transect)
#     await modified_async_session.commit()
#     await modified_async_session.refresh(transect)

#     res_retrieved = await client_two_user.get(f"{ROUTE}/{transect.id}")
#     assert (
#         res_retrieved.status_code == 404
#     )  # Should return 404 as different user


# @pytest.mark.asyncio
# async def test_get_transect_admin_user(
#     test_user_one, client_three_admin, modified_async_session
# ):
#     # Create a different user and transect object directly in DB
#     transect = Transect(
#         owner=test_user_one.id,
#         name="Test Transect 2",
#         description="Test Transect Description",
#         geom=WKTElement("LINESTRING(0 0, 2 2)", srid=4326),
#     )
#     modified_async_session.add(transect)
#     await modified_async_session.commit()
#     await modified_async_session.refresh(transect)

#     res_retrieved = await client_three_admin.get(f"{ROUTE}/{transect.id}")
#     assert res_retrieved.status_code == 200  # Admin can access
