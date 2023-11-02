FROM python:3.11.6-alpine3.18

ENV POETRY_VERSION=1.6.1
RUN pip install "poetry==$POETRY_VERSION"
ENV PYTHONPATH="$PYTHONPATH:/app"

WORKDIR /app

COPY poetry.lock pyproject.toml /app/
RUN poetry config virtualenvs.create false
RUN poetry install --no-interaction --without dev

COPY app /app/

ENTRYPOINT uvicorn --host=0.0.0.0 --timeout-keep-alive=0 main:app