from typing import Any
from fastapi import Depends, APIRouter, Query, Response
from app.config import config
from uuid import UUID
from app.users.models import User
from app.auth.services import require_admin
from keycloak import KeycloakAdmin, KeycloakOpenIDConnection
from pydantic import BaseModel
from enum import Enum
import json


router = APIRouter()


class UserRoles(str, Enum):
    admin = "admin"
    user = "user"


class UserUpdate(BaseModel):
    role: UserRoles


def get_keycloak_admin():
    keycloak_connection = KeycloakOpenIDConnection(
        server_url=config.KEYCLOAK_URL,
        realm_name=config.KEYCLOAK_REALM,
        client_id=config.KEYCLOAK_API_ID,
        client_secret_key=config.KEYCLOAK_API_SECRET,
        verify=True,
    )
    return KeycloakAdmin(connection=keycloak_connection)


class KeycloakUser(BaseModel):
    username: str | None
    firstName: str | None
    lastName: str | None
    email: str | None
    id: str | None
    loginMethod: str | None
    admin: bool | None
    approved_user: bool | None = False


def get_user(
    user_id: str,
    keycloak_admin: KeycloakAdmin,
) -> KeycloakUser:

    user = keycloak_admin.get_user(user_id)

    roles = keycloak_admin.get_realm_roles_of_user(user_id=user_id)
    admin = any(role["name"] == "admin" for role in roles)
    approved_user = any(role["name"] == "user" for role in roles) or admin

    return KeycloakUser(
        email=user.get("email"),
        username=user.get("username"),
        id=user.get("id"),
        firstName=user.get("firstName"),
        lastName=user.get("lastName"),
        admin=admin,
        approved_user=approved_user,
        loginMethod=(
            user.get("attributes", {}).get("login-method", ["EPFL"])[0].upper()
        ),
    )


@router.get("/{user_id}", response_model=KeycloakUser)
async def get_one_user(
    user_id: str,
    keycloak_admin: KeycloakAdmin = Depends(get_keycloak_admin),
    user: User = Depends(require_admin),
) -> KeycloakUser:
    """Get a user by id"""

    return get_user(user_id, keycloak_admin)


@router.get("", response_model=list[KeycloakUser])
async def get_users(
    response: Response,
    user: User = Depends(require_admin),
    keycloak: KeycloakAdmin = Depends(get_keycloak_admin),
    *,
    filter: str = Query(None),
    sort: str = Query(None),
    range: str = Query(None),
) -> list[KeycloakUser]:
    """Get a list of users

    This endpoint is used to get a list of users from Keycloak. It is used
    to populate the list of users in the admin UI.

    Filtering by username is only supported when the admin filter is set to
    False.
    """

    sort = json.loads(sort) if sort else []
    range = json.loads(range) if range else []
    filter = json.loads(filter) if filter else {}

    # Return only current users, to reduce load on Keycloak
    admin_users = keycloak.get_realm_role_members("admin")
    general_users = keycloak.get_realm_role_members("user")

    user_dict = {}

    for user in general_users:
        user_dict[user["id"]] = user
        user_dict[user["id"]]["admin"] = False
        user_dict[user["id"]]["approved_user"] = True

    for user in admin_users:
        user_dict[user["id"]] = user
        user_dict[user["id"]]["admin"] = True
        user_dict[user["id"]]["approved_user"] = True

    if "users_only" in filter and filter["users_only"] is True:
        users = list(user_dict.values())
    else:
        # Return all users and match them with the current user list
        query = {"first": range[0], "max": range[1] - range[0] + 1}

        if "username" in filter:
            query["username"] = filter["username"]

        users = keycloak.get_users(query=query)

    if len(range) == 2:
        start, end = range
    else:
        start, end = [0, len(users) + 1]

    response.headers["Content-Range"] = f"users {start}-{end}/{len(users)}"

    user_objs = []
    for user in users[start : end + 1]:
        approved = False
        admin = False
        if user.get("id") in user_dict:
            approved = user_dict[user["id"]].get("approved_user", False)
            admin = user_dict[user["id"]].get("admin", False)
        user_objs.append(
            KeycloakUser(
                email=user.get("email"),
                username=user.get("username"),
                id=user.get("id"),
                firstName=user.get("firstName"),
                lastName=user.get("lastName"),
                admin=admin,
                approved_user=approved,
                loginMethod=user.get("attributes", {})
                .get("login-method", ["EPFL"])[0]
                .upper(),
            )
        )

    return user_objs


@router.put("/{user_id}")
async def update_user(
    user_id: UUID,
    user_update: UserUpdate,
    keycloak_admin: KeycloakAdmin = Depends(get_keycloak_admin),
    user: User = Depends(require_admin),
) -> Any:
    """Updates the role of the user"""

    # Everything here works in list, although we only have one role, let's
    # keep it that way for future-proofing
    roles_to_assign = [user_update.role.value]

    # Get the role objects from keycloak
    realm_roles = keycloak_admin.get_realm_roles(["admin", "user"])
    roles = [role for role in realm_roles if role["name"] in roles_to_assign]
    current_userroles = keycloak_admin.get_realm_roles_of_user(user_id=user_id)

    roles_to_add = []
    roles_to_delete = []
    for role in roles:
        for userrole in current_userroles:
            if role["id"] == userrole["id"]:
                continue
        else:
            roles_to_add.append(role)

    for userrole in current_userroles:
        for role in roles:
            if role["id"] == userrole["id"]:
                continue
        else:
            roles_to_delete.append(userrole)

    keycloak_admin.assign_realm_roles(
        user_id=user_id,
        roles=roles_to_add,
    )
    keycloak_admin.delete_realm_roles_of_user(
        user_id=user_id,
        roles=roles_to_delete,
    )
    return get_user(user_id, keycloak_admin)


@router.delete("/{user_id}")
async def delete_user(
    user_id: str,
    keycloak_admin: KeycloakAdmin = Depends(get_keycloak_admin),
    user: User = Depends(require_admin),
) -> Any:
    """Deleting a user removes them from the roles of 'admin' and 'user'

    Removing these roles means they are now not an approved_user.
    """

    # Get the role objects from keycloak
    realm_roles = keycloak_admin.get_realm_roles(["admin", "user"])
    roles = [role for role in realm_roles if role["name"] in ["admin", "user"]]
    current_userroles = keycloak_admin.get_realm_roles_of_user(user_id=user_id)

    roles_to_delete = []
    for userrole in current_userroles:
        for role in roles:
            if role["id"] == userrole["id"]:
                roles_to_delete.append(userrole)

    keycloak_admin.delete_realm_roles_of_user(
        user_id=user_id,
        roles=roles_to_delete,
    )
    return get_user(user_id, keycloak_admin)
