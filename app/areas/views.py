from fastapi import Depends, FastAPI, APIRouter, Query, Response
from sqlmodel import select, Session
from app.db import get_session, AsyncSession
from app.areas.models import Area, AreaCreate, AreaRead
from uuid import UUID, uuid4

router = APIRouter()


areas = [
    {
        "id": "5cfab11a-eed7-4aab-a1d5-7a922bfd760b",
        "name": "Binntal",
        "description": "Binntal Sampling SIte",
        "centroid": [46.38066410843288, 8.277772859269739],
        "location": [
            [46.38138404346455, 8.275374887970651, 0.0],
            [46.381373016961327, 8.275686748818783, 0.0],
            [46.381291016188577, 8.275861987109852, 0.0],
            [46.381426994724642, 8.276352616493062, 0.0],
            [46.381248189419004, 8.276639826136988, 0.0],
            [46.381358132061301, 8.277111144266303, 0.0],
            [46.381161653184343, 8.277133325801916, 0.0],
            [46.381140602410056, 8.277325261406945, 0.0],
            [46.381079184037468, 8.277371590208272, 0.0],
            [46.381067522463766, 8.27776853800472, 0.0],
            [46.381150191627391, 8.277798181460897, 0.0],
            [46.381135880902377, 8.278258125016142, 0.0],
            [46.381096155749994, 8.278320547163011, 0.0],
            [46.381092924600267, 8.278462328676714, 0.0],
            [46.381044335187006, 8.278543524406587, 0.0],
            [46.381019410208282, 8.278669211236688, 0.0],
            [46.380611988168035, 8.278899294061404, 0.0],
            [46.380396997911703, 8.277001754710239, 0.0],
            [46.380570664645916, 8.277114747441429, 0.0],
            [46.380730788307801, 8.276994299445642, 0.0],
            [46.380861076231497, 8.276778836425491, 0.0],
            [46.380937792673556, 8.276433322137946, 0.0],
            [46.380953020375138, 8.276143591294879, 0.0],
            [46.380995856157242, 8.275955143550593, 0.0],
            [46.380975837276637, 8.275712146169464, 0.0],
            [46.381002224056424, 8.275387917019298, 0.0],
            [46.38138404346455, 8.275374887970651, 0.0],
        ],
    }
]


def convert_json_to_model():
    return [
        AreaRead(
            name=area["name"],
            description=area["description"],
            id=area["id"],
            centroid=area["centroid"],
            location=area["location"],
        )
        for area in areas
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


@router.get("", response_model=list[AreaRead])
async def get_areas(
    response: Response,
    session: AsyncSession = Depends(get_session),
    *,
    sort: list[str] | None = None,
    range: list[int] | None = None,
    filter: dict[str, str] | None = None,
):
    """Get all areas"""

    result = await session.execute(select(Area))
    areas = result.scalars().all()

    # Return a Content-Range header for react-admin pagination

    # if range:
    # start, end = range
    # total = len(areas)
    # return convert_json_to_model(), {
    # "Content-Range": f"areas {start}-{end}/{total}"
    # }
    # else:
    if True:
        start = 0
        end = 1
        total = 1
        response.headers["Content-Range"] = f"areas {start}-{end}/{total}"

    return convert_json_to_model()

    return [
        AreaRead(
            id=area.uuid,
            name=area.name,
            description=area.description,
        )
        for area in areas
    ]


@router.post("/", response_model=AreaRead)
async def create_area(
    area: AreaCreate,
    session: AsyncSession = Depends(get_session),
) -> AreaRead:
    """Creates an area"""

    area = Area(name=area.name, description=area.description)
    session.add(area)
    await session.commit()
    await session.refresh(area)

    return AreaRead(
        name=area.name,
        description=area.description,
        id=area.uuid,
    )


@router.put("/{area_id}", response_model=AreaRead)
async def update_area(
    area_id: UUID,
    session: AsyncSession = Depends(get_session),
) -> AreaRead:
    res = await session.execute(select(Area).where(Area.uuid == area_id))
    area = res.one()

    # Update area
    area.name = area.name
    area.description = area.description

    await session.add(area)
    await session.commit()
    await session.refresh(area)

    return AreaRead(
        name=area.name,
        description=area.description,
        id=area.uuid,
    )


@router.put("/", response_model=list[AreaRead])
async def update_areas(
    session: AsyncSession = Depends(get_session),
    filter: dict[str, str] | None = None,
) -> list[AreaRead]:
    pass


@router.delete("/")
async def delete_areas(
    session: AsyncSession = Depends(get_session),
) -> None:
    pass


@router.delete("/{area_id}", response_model=AreaRead)
async def delete_area(
    session: AsyncSession = Depends(get_session),
    filter: dict[str, str] | None = None,
) -> None:
    pass
