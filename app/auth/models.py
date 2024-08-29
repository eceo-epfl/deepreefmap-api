from pydantic import BaseModel


class authConfiguration(BaseModel):
    server_url: str
    realm: str
    client_id: str
    client_secret: str
    authorization_url: str
    token_url: str


class DownloadToken(BaseModel):
    token: str


class HealthCheck(BaseModel):
    """Response model to validate and return when performing a health check."""

    status: str = "OK"


class KeycloakConfig(BaseModel):
    """Parameters for frontend access to Keycloak"""

    clientId: str
    realm: str
    url: str
