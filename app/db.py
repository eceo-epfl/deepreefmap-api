import os
from sqlmodel import SQLModel, create_engine
from sqlmodel.ext.asyncio.session import AsyncSession, AsyncEngine
from sqlalchemy.orm import sessionmaker
from geoalchemy2 import load_spatialite
from sqlalchemy.event import listen
from sqlalchemy import event
from app.areas.models import Area
from app.sensors.models import Sensor, SensorData
from app.config import config
from datetime import datetime


engine = AsyncEngine(create_engine(config.DB_URL, echo=True, future=True))


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
# Sample data
sensor_data = """
0;2023.05.30 06:45;4;22,1875;22,1875;22,625;334;202;0;
1;2023.05.30 07:00;4;22;22,125;22;334;202;0;
2;2023.05.30 07:15;4;21,125;21,5;21,3125;331;202;0;
3;2023.05.30 07:30;4;20,625;21;21;329;202;0;
4;2023.05.30 07:45;4;20,5625;20,8125;20,75;328;202;0;
5;2023.05.30 08:00;4;20,4375;20,625;20,625;327;202;0;
6;2023.05.30 08:15;4;20,375;20,5;20,5;327;202;0;
7;2023.05.30 08:30;4;20,375;20,5;20,5;327;202;0;
8;2023.05.30 08:45;4;20,375;20,5;20,4375;327;202;0;
9;2023.05.30 09:00;4;20,375;20,4375;20,375;326;202;0;
10;2023.05.30 09:15;4;20,375;20,4375;20,375;327;202;0;
11;2023.05.30 09:30;4;20,375;20,4375;20,375;327;202;0;
12;2023.05.30 09:45;4;20,375;20,5;20,5;327;202;0;
13;2023.05.30 10:00;4;20,4375;20,5;20,5;327;202;0;
"""


# Function to convert sample data to SensorData objects
def convert_sensor_data_to_objects(sample_data: str):
    sensor_data_objects = []
    rows = sample_data.strip().split("\n")
    for row in rows:
        values = row.split(";")
        values[1] = datetime.strptime(values[1], "%Y.%m.%d %H:%M")
        values[3:5] = [
            float(value.replace(",", ".")) if value and "," in value else None
            for value in values[3:5]
        ]
        for snsr in sensors:
            sensor_data_objects.append(
                SensorData(
                    instrument_seq=int(values[0]),
                    time=values[1],
                    time_zone=int(values[2])
                    if values[2] is not None
                    else None,
                    temperature_1=values[3],
                    temperature_2=values[4],
                    temperature_3=values[5],
                    soil_moisture_count=values[6],
                    shake=int(values[7]) if values[7] is not None else None,
                    error_flat=int(values[8])
                    if values[8] is not None
                    else None,
                    sensor_id=snsr["id"],
                )
            )
    return sensor_data_objects


async def init_db():
    async def seed_db():
        from app.db import engine
        from sqlalchemy.orm import sessionmaker
        from shapely.geometry import Polygon
        from shapely.geometry import Point

        async_session = sessionmaker(
            engine, class_=AsyncSession, expire_on_commit=False
        )
        async with async_session() as session:
            # for area in areas:
            #     record = Area(
            #         id=area["id"],
            #         name=area["name"],
            #         description=area["description"],
            #         geom=Polygon(area["location"]).wkt,
            #     )
            #     session.add(record)
            # for sensor in sensors:
            #     record = Sensor(
            #         id=sensor["id"],
            #         name=sensor["name"],
            #         description=sensor["description"],
            #         geom=Point(sensor["location"]).wkt,
            #         area_id=sensor["area_id"],
            #     )
            #     session.add(record)

            sensor_data_objects = convert_sensor_data_to_objects(sensor_data)
            for obj in sensor_data_objects:
                session.add(obj)

            await session.commit()

    async with engine.begin() as conn:
        pass
        # Drop all and start with a seeded DB whilst in development
        # Create database if it does not exist.

        # await conn.run_sync(SQLModel.metadata.drop_all)
        # await conn.run_sync(SQLModel.metadata.create_all)
        # await seed_db()


async def get_session() -> AsyncSession:
    async_session = sessionmaker(
        engine, class_=AsyncSession, expire_on_commit=False
    )
    async with async_session() as session:
        yield session
