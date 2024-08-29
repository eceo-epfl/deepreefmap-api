from app.db import get_session, AsyncSession
from fastapi import Depends, Response
from sqlmodel import select
from typing import Any
import json
from sqlalchemy.sql import func
from sqlalchemy import or_
from uuid import UUID
from app.users.models import User
from app.auth.services import get_user_info


class CRUD:
    def __init__(
        self,
        db_model: Any,
        db_model_read: Any,
        db_model_create: Any,
        db_model_update: Any,
    ):
        self.db_model = db_model
        self.db_model_read = db_model_read
        self.db_model_create = db_model_create
        self.db_model_update = db_model_update

    async def __call__(self, *args: Any, **kwds: Any) -> Any:
        pass

    @property
    def exact_match_fields(
        self,
    ) -> list[str]:
        """Returns a list of all the UUID fields in the model

        These cannot be performed with a likeness query and must have an
        exact match.

        """
        schema = self.db_model.model_json_schema()

        uuid_properties = []
        for prop_name, prop_details in schema["properties"].items():
            prop_type = prop_details.get("type")
            if isinstance(prop_type, list) and "string" in prop_type:
                any_of_types = prop_details.get("anyOf")
                if any_of_types:
                    for any_of_type in any_of_types:
                        if "string" in any_of_type.get("type", []):
                            uuid_properties.append(prop_name)
                            break
                elif (
                    "format" in prop_details
                    and prop_details["format"] == "uuid"
                ):
                    uuid_properties.append(prop_name)
            elif prop_type in ["string", "null"]:  # Allow case when optional
                if (
                    "format" in prop_details
                    and prop_details["format"] == "uuid"
                ):
                    uuid_properties.append(prop_name)

        return uuid_properties

    async def get_model_data(
        self,
        filter: str,
        sort: str,
        range: str,
        user: User = Depends(get_user_info),
        session: AsyncSession = Depends(get_session),
    ) -> list:
        """Returns the data of a model with a filter applied

        Similar to the count query except returns the data instead of the count
        """

        sort = json.loads(sort) if sort else []
        range = json.loads(range) if range else []
        filter = json.loads(filter) if filter else {}

        query = select(self.db_model)

        if not user.is_admin:
            query = query.where(self.db_model.owner == user.id)

        if len(filter):
            for field, value in filter.items():
                if field in self.exact_match_fields:
                    if isinstance(value, list):
                        # Combine multiple filters with OR
                        or_conditions = []
                        for v in value:
                            or_conditions.append(
                                getattr(self.db_model, field) == v
                            )

                        query = query.filter(or_(*or_conditions))
                    else:
                        # If it's not a list, apply a simple equality filter
                        query = query.filter(
                            getattr(self.db_model, field) == value
                        )
                else:
                    if isinstance(value, list):
                        or_conditions = []
                        for v in value:
                            or_conditions.append(
                                getattr(self.db_model, field).like(
                                    f"%{str(v)}%"
                                )
                            )

                        query = query.filter(or_(*or_conditions))
                    elif isinstance(value, bool):
                        if value is True:
                            query = query.filter(
                                getattr(self.db_model, field).has()
                            )
                        else:
                            query = query.filter(
                                ~getattr(self.db_model, field).has()
                            )
                    else:
                        # Apply a LIKE filter for string matching
                        query = query.filter(
                            getattr(self.db_model, field).like(
                                f"%{str(value)}%"
                            )
                        )

        if len(sort) == 2:
            sort_field, sort_order = sort
            # Make sure sort_field is a valid field
            if not hasattr(self.db_model, sort_field):
                raise ValueError(f"{sort_field} is not a valid field")

            if sort_order == "ASC":
                query = query.order_by(getattr(self.db_model, sort_field))
            else:
                query = query.order_by(
                    getattr(self.db_model, sort_field).desc()
                )

        if len(range):
            start, end = range
            query = query.offset(start).limit(end - start)

        res = await session.exec(query)

        return res.all()

    async def get_total_count(
        self,
        response: Response,
        sort: str,
        range: str,
        filter: str,
        user: User = Depends(get_user_info),
        session: AsyncSession = Depends(get_session),
    ) -> int:
        """Returns the count of a model with a filter applied"""

        filter = json.loads(filter) if filter else {}
        range = json.loads(range) if range else []

        query = select(func.count(self.db_model.iterator))
        if not user.is_admin:
            query = query.where(self.db_model.owner == user.id)

        if len(filter):
            for field, value in filter.items():
                if field in self.exact_match_fields:
                    if isinstance(value, list):
                        # Combine multiple filters with OR
                        or_conditions = []
                        for v in value:
                            or_conditions.append(
                                getattr(self.db_model, field) == v
                            )

                        query = query.filter(or_(*or_conditions))
                    else:
                        # If it's not a list, apply a simple equality filter
                        query = query.filter(
                            getattr(self.db_model, field) == value
                        )
                else:
                    if isinstance(value, list):
                        or_conditions = []
                        for v in value:
                            or_conditions.append(
                                getattr(self.db_model, field).like(
                                    f"%{str(v)}%"
                                )
                            )

                        query = query.filter(or_(*or_conditions))
                    elif isinstance(value, bool):
                        if value is True:
                            # If true, the field has a value and the value
                            query = query.filter(
                                getattr(self.db_model, field).has()
                            )
                        else:
                            query = query.filter(
                                ~getattr(self.db_model, field).has()
                            )
                    else:
                        # Apply a LIKE filter for string matching
                        query = query.filter(
                            getattr(self.db_model, field).like(
                                f"%{str(value)}%"
                            )
                        )

        count = await session.exec(query)
        total_count = count.one()

        if len(range) == 2:
            start, end = range
        else:
            start, end = [0, total_count]  # For content-range header

        response.headers["Content-Range"] = (
            f"sensor {start}-{end}/{total_count}"
        )

        return total_count

    async def get_model_by_id(
        self,
        session: AsyncSession,
        user: User = Depends(get_user_info),
        *,
        model_id: UUID,
    ) -> Any:
        """Get a model by id"""

        query = select(self.db_model).where(self.db_model.id == model_id)

        if not user.is_admin:
            query = query.filter(self.db_model.owner == user.id)

        res = await session.exec(query)
        obj = res.one_or_none()

        return obj
