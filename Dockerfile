FROM ubuntu:22.04 as rcp-stage

RUN apt-get update && apt-get install -y wget curl python3-pip python3.11

WORKDIR /root

RUN wget --content-disposition https://rcp-caas-test.rcp.epfl.ch/cli/linux
RUN chmod +x runai
RUN mkdir .kube
RUN curl -LO "https://dl.k8s.io/release/$(curl -L -s https://dl.k8s.io/release/stable.txt)/bin/linux/amd64/kubectl" && \
    curl -LO "https://dl.k8s.io/release/$(curl -L -s https://dl.k8s.io/release/stable.txt)/bin/linux/amd64/kubectl.sha256" && \
    echo "$(cat kubectl.sha256)  kubectl" | sha256sum --check && \
    install -o root -g root -m 0755 kubectl /usr/local/bin/kubectl && \
    kubectl version --client --output=yaml

ENV POETRY_VERSION=1.6.1
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
