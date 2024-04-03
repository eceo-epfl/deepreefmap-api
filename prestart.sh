# Run migrations
poetry run alembic upgrade head
mkdir -p /app/.kube && cp /root/.kube/config.yaml /app/.kube/config.yaml
uvicorn --host=0.0.0.0 --timeout-keep-alive=0 app.main:app --reload
