from fastapi import Depends, FastAPI, APIRouter, Query, Response
from sqlmodel import select, Session
from app.db import get_session, AsyncSession
from app.sensors.models import Sensor, SensorCreate, SensorRead
from uuid import UUID, uuid4

router = APIRouter()


sensors = [
    {
        "id": "d285bc99-e336-4d05-918d-ac162f7bda98",
        "name": "Sensor 0",
        "description": "Description for Sensor 0",
        "location": [46.38230786578320419, 8.27814012307909941],
        "area_id": "5cfab11a-eed7-4aab-a1d5-7a922bfd760b",
    },
    {
        "id": "660ad4ec-840b-49d4-b600-570b759c3230",
        "name": "Sensor 1",
        "description": "Description for Sensor 1",
        "location": [46.38186623337439585, 8.27799151928392263],
        "area_id": "5cfab11a-eed7-4aab-a1d5-7a922bfd760b",
    },
    {
        "id": "35ff4ae0-588f-4a7f-a021-29a872946241",
        "name": "Sensor 2",
        "description": "Description for Sensor 2",
        "location": [46.3814186891362894, 8.27799497732366341],
        "area_id": "5cfab11a-eed7-4aab-a1d5-7a922bfd760b",
    },
    {
        "id": "5de91495-3b7b-4915-b25c-c947ca5aef78",
        "name": "Sensor 3",
        "description": "Description for Sensor 3",
        "location": [46.38096931499390507, 8.27792233228177921],
        "area_id": "5cfab11a-eed7-4aab-a1d5-7a922bfd760b",
    },
    {
        "id": "b175386b-9676-4545-9889-b74d45b8cf6d",
        "name": "Sensor 4",
        "description": "Description for Sensor 4",
        "location": [46.38051451536809111, 8.27793605246943365],
        "area_id": "5cfab11a-eed7-4aab-a1d5-7a922bfd760b",
    },
    {
        "id": "1e88dc29-e47b-4ddd-b73a-5ea96f9219ea",
        "name": "Sensor 5",
        "description": "Description for Sensor 5",
        "location": [46.38006648492084594, 8.27800520319045852],
        "area_id": "5cfab11a-eed7-4aab-a1d5-7a922bfd760b",
    },
    {
        "id": "0e5f7a3d-5ce6-4369-8f80-a139fdcd8452",
        "name": "Sensor 6",
        "description": "Description for Sensor 6",
        "location": [46.38246671481461902, 8.27737143569202516],
        "area_id": "5cfab11a-eed7-4aab-a1d5-7a922bfd760b",
    },
    {
        "id": "f9146dc3-57ed-4cdf-bfa0-41394f72b5cd",
        "name": "Sensor 7",
        "description": "Description for Sensor 7",
        "location": [46.38201420584354651, 8.27739902828637142],
        "area_id": "5cfab11a-eed7-4aab-a1d5-7a922bfd760b",
    },
    {
        "id": "f5ab5b6c-2408-4cb5-b707-70639596433b",
        "name": "Sensor 8",
        "description": "Description for Sensor 8",
        "location": [46.38156470346784488, 8.27734367531285642],
        "area_id": "5cfab11a-eed7-4aab-a1d5-7a922bfd760b",
    },
    {
        "id": "499561d3-b4c4-4081-af75-6880705f6873",
        "name": "Sensor 9",
        "description": "Description for Sensor 9",
        "location": [46.38111991010548962, 8.27729876981988433],
        "area_id": "5cfab11a-eed7-4aab-a1d5-7a922bfd760b",
    },
    {
        "id": "6ffb2f2e-d273-4bc9-89e2-14869e93f4d3",
        "name": "Sensor 10",
        "description": "Description for Sensor 10",
        "location": [46.38066971731334576, 8.27733677186708761],
        "area_id": "5cfab11a-eed7-4aab-a1d5-7a922bfd760b",
    },
    {
        "id": "9d4c46d4-d779-4446-88b0-851710d86311",
        "name": "Sensor 11",
        "description": "Description for Sensor 11",
        "location": [46.38021723391943141, 8.27736090633433008],
        "area_id": "5cfab11a-eed7-4aab-a1d5-7a922bfd760b",
    },
]


def convert_json_to_model():
    return [
        SensorRead(
            name=sensor["name"],
            description=sensor["description"],
            id=sensor["id"],
            location=sensor["location"],
            area_id=sensor["area_id"],
        )
        for sensor in sensors
    ]


# @router.get("/{area_id}", response_model=AreaRead)
# async def get_area(
#     session: AsyncSession = Depends(get_session),
#     *,
#     area_id: UUID,
#     sort: list[str] | None = None,
#     range: list[int] | None = None,
#     filter: dict[str, str] | None = None,
# ) -> AreaRead:
#     pass


@router.get("", response_model=list[SensorRead])
async def get_sensors(
    response: Response,
    session: AsyncSession = Depends(get_session),
    *,
    sort: list[str] | None = None,
    range: list[int] | None = None,
    filter: str | None = None,
):
    """Get all sensors"""

    result = await session.execute(select(Sensor))
    sensors = result.scalars().all()

    start = 0
    end = len(convert_json_to_model())
    total = len(convert_json_to_model())
    response.headers["Content-Range"] = f"sensors {start}-{end}/{total}"

    return convert_json_to_model()

    return [
        AreaRead(
            id=area.uuid,
            name=area.name,
            description=area.description,
        )
        for sensor in sensors
    ]
