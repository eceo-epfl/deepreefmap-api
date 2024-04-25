# Get RunAI executable (only possible on EPFL network)
wget --content-disposition https://rcp-caas-test.rcp.epfl.ch/cli/linux
chmod +x runai
mv runai /usr/local/bin

# Run migrations
poetry run alembic upgrade head
mkdir -p /app/.kube && cp /root/.kube/config.yaml /app/.kube/config.yaml
uvicorn --host=0.0.0.0 --timeout-keep-alive=0 app.main:app --reload
