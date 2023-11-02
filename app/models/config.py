from pydantic import BaseModel


class KeycloakConfig(BaseModel):
    """Parameters for frontend access to Keycloak"""

    clientId: str
    realm: str
    url: str
