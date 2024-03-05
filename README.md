# deepreefmap-api
The backend API for the deepreefmap project in the ECEO lab

### Run the server
Build the docker image:

`docker build -t deepreefmap-api .`

Run a postgres server, then run the docker image:
```
docker run \
    -e DB_HOST=<postgres hostname>
    -e DB_PORT=<postgres port>
    -e DB_USER=<postgres user>
    -e DB_PASSWORD=<postgres password>
    -e DB_NAME=<postgres dbname>
    -e DB_PREFIX=postgresql+asyncpg
    docker.io/library/deepreefmap-api:latest
```

The [deepreefmap-ui repository](https://github.com/LabECEO/deepreefmap-ui) has a development docker-compose.yaml file to load the API, BFF, PostGIS and UI all together, assuming all repositories are cloned locally.