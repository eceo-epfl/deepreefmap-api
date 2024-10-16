from fastapi.security import OAuth2AuthorizationCodeBearer
from keycloak import KeycloakOpenID  # pip require python-keycloak
from app.config import config
from app.users.models import User
from fastapi import HTTPException, Security, Depends, status

# This is used for fastapi docs authentification
oauth2_scheme = OAuth2AuthorizationCodeBearer(
    authorizationUrl=f"{config.KEYCLOAK_URL}",
    tokenUrl=(
        f"{config.KEYCLOAK_URL}/realms/{config.KEYCLOAK_REALM}"
        "/protocol/openid-connect/token"
    ),
)

keycloak_openid = KeycloakOpenID(
    server_url=config.KEYCLOAK_URL,
    client_id=config.KEYCLOAK_API_ID,
    client_secret_key=config.KEYCLOAK_API_SECRET,
    realm_name=config.KEYCLOAK_REALM,
    verify=True,
)


# Get the payload/token from keycloak
def get_payload(
    token: str = Security(oauth2_scheme),
) -> dict:
    try:
        return keycloak_openid.decode_token(token)
    except Exception as e:
        raise HTTPException(
            status_code=status.HTTP_401_UNAUTHORIZED,
            detail=str(e),  # "Invalid authentication credentials",
            headers={"WWW-Authenticate": "Bearer"},
        )


# Get user infos from the payload
def get_user_info(
    payload: dict = Depends(get_payload),
) -> User:
    try:
        # TODO: Would be better to contain approved_user/is_admin logic here,
        # rather than decoupling roles into booleans elsewhere in the code
        user = User(
            id=payload.get("sub"),
            username=payload.get("preferred_username"),
            email=payload.get("email"),
            first_name=payload.get("given_name"),
            last_name=payload.get("family_name"),
            realm_roles=payload.get("realm_access", {}).get("roles", []),
            client_roles=payload.get("realm_access", {}).get("roles", []),
        )
    except Exception as e:
        raise HTTPException(
            status_code=status.HTTP_400_BAD_REQUEST,
            detail=str(e),  # "Invalid authentication credentials",
            headers={"WWW-Authenticate": "Bearer"},
        )

    # If neither 'user' or 'admin' in user.realm_roles, then user is not
    # authorised to perform this operation
    if not any(role in user.realm_roles for role in ["user", "admin"]):
        raise HTTPException(
            status_code=status.HTTP_403_FORBIDDEN,
            detail="You are not authorised to perform this operation",
            headers={"WWW-Authenticate": "Bearer"},
        )
    return user


async def require_admin(
    user: User = Depends(get_user_info),
) -> User:
    if "admin" not in user.realm_roles:
        raise HTTPException(
            status_code=status.HTTP_403_FORBIDDEN,
            detail="You are not authorised to perform this operation",
            headers={"WWW-Authenticate": "Bearer"},
        )
    return user
