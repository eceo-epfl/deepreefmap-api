from fastapi import Depends, FastAPI, APIRouter, Query, Response
from sqlmodel import select, Session
from app.db import get_session, AsyncSession
from app.areas.models import Area, AreaCreate, AreaRead
from uuid import UUID, uuid4
from sqlalchemy import func
import json

router = APIRouter()


areas = [
    {
        "id": "5cfab11a-eed7-4aab-a1d5-7a922bfd760b",
        "name": "Binntal",
        "description": "Binntal Sampling SIte",
        "centroid": [46.38066410843288, 8.277772859269739],
        "location": [
            [46.38138404346455, 8.275374887970651],
            [46.381373016961327, 8.275686748818783],
            [46.381291016188577, 8.275861987109852],
            [46.381426994724642, 8.276352616493062],
            [46.381248189419004, 8.276639826136988],
            [46.381358132061301, 8.277111144266303],
            [46.381161653184343, 8.277133325801916],
            [46.381140602410056, 8.277325261406945],
            [46.381079184037468, 8.277371590208272],
            [46.381067522463766, 8.27776853800472],
            [46.381150191627391, 8.277798181460897],
            [46.381135880902377, 8.278258125016142],
            [46.381096155749994, 8.278320547163011],
            [46.381092924600267, 8.278462328676714],
            [46.381044335187006, 8.278543524406587],
            [46.381019410208282, 8.278669211236688],
            [46.380611988168035, 8.278899294061404],
            [46.380396997911703, 8.277001754710239],
            [46.380570664645916, 8.277114747441429],
            [46.380730788307801, 8.276994299445642],
            [46.380861076231497, 8.276778836425491],
            [46.380937792673556, 8.276433322137946],
            [46.380953020375138, 8.276143591294879],
            [46.380995856157242, 8.275955143550593],
            [46.380975837276637, 8.275712146169464],
            [46.381002224056424, 8.275387917019298],
            [46.38138404346455, 8.275374887970651],
        ],
    }
]


async def add_areas_to_db():
    from app.db import engine
    from sqlalchemy.orm import sessionmaker
    from shapely.geometry import Polygon

    async_session = sessionmaker(
        engine, class_=AsyncSession, expire_on_commit=False
    )
    async with async_session() as session:
        for area in areas:
            record = Area(
                uuid=area["id"],
                name=area["name"],
                description=area["description"],
                # centroid=area["centroid"],
                geom=Polygon(area["location"]).wkt,
            )
            session.add(record)
        await session.commit()


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


@router.get("")
async def get_areas(
    response: Response,
    session: AsyncSession = Depends(get_session),
    *,
    sort: list[str] | None = None,
    range: list[int] | None = None,
    filter: dict | None = None,
):
    """Get all areas"""

    query = select(Area)
    # Query all areas, including sort, range and filter query params according
    # to react-admin spec:
    # ?sort=["title","ASC"]&range=[0, 24]&filter={"title":"bar"}

    # Order by sort field params ie. ["name","ASC"]
    if sort:
        sort_field, sort_order = sort
        if sort_order == "ASC":
            query = query.order_by(Area.name)
        else:
            query = query.order_by(Area.name.desc())

    if range:
        start, end = range
        query = query.offset(start).limit(end - start + 1)

    # Filter by filter field params ie. {"name":"bar"}
    if filter:
        for field, value in filter.items():
            if field in ["name", "description"]:
                query = query.where(getattr(Area, field).like(f"%{value}%"))

    # Execute query
    results = await session.execute(query)
    areas = results.scalars().all()

    # Do a query to satisfy total count for "Content-Range" header
    query = await session.execute(select(func.count(Area.id)))
    total_count = query.scalar_one()

    # Return a Content-Range header for react-admin pagination

    if range:
        start, end = range
    else:
        start, end = [0, total_count]

    response.headers["Content-Range"] = f"areas {start}-{end}/{total_count}"

    payload = []
    for area in areas:
        payload.append(
            AreaRead(
                id=area.uuid,
                name=area.name,
                description=area.description,
                geom=area.geom,
            )
        )
    return payload


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
