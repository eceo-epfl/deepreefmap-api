FROM python:3.12.3

RUN apt-get update && apt-get install -y curl python3-pip g++ libgeos-dev proj-bin libproj-dev

WORKDIR /root
RUN mkdir .kube

RUN curl -LO "https://dl.k8s.io/release/$(curl -L -s https://dl.k8s.io/release/stable.txt)/bin/linux/amd64/kubectl" && \
    curl -LO "https://dl.k8s.io/release/$(curl -L -s https://dl.k8s.io/release/stable.txt)/bin/linux/amd64/kubectl.sha256" && \
    echo "$(cat kubectl.sha256)  kubectl" | sha256sum -c && \
    install -o root -g root -m 0755 kubectl /usr/local/bin/kubectl && \
    kubectl version --client --output=yaml

ENV POETRY_VERSION=1.8.2
RUN pip install "poetry==$POETRY_VERSION"
ENV PYTHONPATH="$PYTHONPATH:/app"

WORKDIR /app

COPY poetry.lock pyproject.toml /app/
RUN poetry config virtualenvs.create false
RUN poetry install --no-interaction --without dev

COPY alembic.ini prestart.sh /app
COPY migrations /app/migrations
COPY app /app/app

ENTRYPOINT sh prestart.sh
