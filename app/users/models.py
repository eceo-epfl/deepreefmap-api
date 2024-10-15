from pydantic import BaseModel, model_validator
from typing_extensions import Self


class User(BaseModel):
    id: str
    username: str
    email: str
    first_name: str
    last_name: str
    realm_roles: list
    client_roles: list
    is_admin: bool = False

    @model_validator(mode="after")
    def check_admin(self) -> Self:
        # Check if user is an admin
        if "admin" in self.realm_roles:
            self.is_admin = True

        return self
