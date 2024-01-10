# ECEO API
The backend API for the ECEO lab

### Seed the database with some data

Assuming the credentials for the database are `postgres:psql@localhost:5433/postgres` and the database is empty, run the following command to seed the database with some data:

`poetry run python seed_db.py postgresql+asyncpg://postgres:psql@localhost:5433/postgres`

### Run the server
Build the docker image:

`docker build -t eceo-api .`

Run a PostGIS server, then run the docker image:
```
docker run \
    -e DB_HOST=<postgis hostname>
    -e DB_PORT=<postgis port>
    -e DB_USER=<postgis user>
    -e DB_PASSWORD=<postgis password>
    -e DB_NAME=<postgis dbname>
    -e DB_PREFIX=postgresql+asyncpg
    docker.io/library/eceo-api:latest
```

The [deepreefmap-ui repository](https://github.com/LabECEO/deepreefmap-ui) has a development docker-compose.yaml file to load the API, BFF, PostGIS and UI all together, assuming all repositories are cloned locally.