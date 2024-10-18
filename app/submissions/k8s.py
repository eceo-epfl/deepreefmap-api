from kubernetes import client, config as k8s_config
from app.config import config
from uuid import UUID
from app.submissions.models import KubernetesExecutionStatus
from typing import Any
from kubernetes.client import CoreV1Api, ApiClient, CustomObjectsApi
from cashews import cache
from fastapi.concurrency import run_in_threadpool
import os
import json
import yaml
import requests


def refresh_oidc_token(kubeconfig_path):
    # Read the kubeconfig file
    with open(kubeconfig_path, "r") as f:
        kubeconfig = yaml.safe_load(f)

    # Find the current context
    current_context_name = kubeconfig.get("current-context")
    contexts = kubeconfig.get("contexts", [])
    current_context = next(
        (ctx for ctx in contexts if ctx["name"] == current_context_name), None
    )
    if not current_context:
        raise Exception("Current context not found in kubeconfig")

    # Get the user associated with the current context
    user_name = current_context["context"]["user"]
    users = kubeconfig.get("users", [])
    user = next((u for u in users if u["name"] == user_name), None)
    if not user:
        raise Exception(f"User {user_name} not found in kubeconfig")

    auth_provider = user["user"].get("auth-provider")
    if not auth_provider or auth_provider.get("name") != "oidc":
        raise Exception("Auth provider is not OIDC")

    # Extract the refresh token, client ID, and idp-issuer-url
    config = auth_provider.get("config", {})
    refresh_token = config.get("refresh-token")
    client_id = config.get("client-id")
    idp_issuer_url = config.get("idp-issuer-url")
    if not refresh_token or not idp_issuer_url or not client_id:
        raise Exception("Required fields not found in kubeconfig")

    # Prepare the token endpoint URL
    token_endpoint = f"{idp_issuer_url}/protocol/openid-connect/token"

    # Prepare the POST request data
    data = {
        "grant_type": "refresh_token",
        "refresh_token": refresh_token,
        "client_id": client_id,
    }

    # Send the POST request to refresh the token
    response = requests.post(token_endpoint, data=data)
    if response.status_code != 200:
        raise Exception(f"Failed to refresh token: {response.text}")

    token_response = response.json()
    new_id_token = token_response.get("id_token")
    if not new_id_token:
        raise Exception("id_token not found in token response")

    # Update the kubeconfig in memory
    # Remove the auth-provider section and set the token
    user["user"].pop("auth-provider", None)
    user["user"]["token"] = new_id_token

    return kubeconfig


def get_k8s_v1():
    try:
        # Path to your kubeconfig file
        kubeconfig_path = config.KUBECONFIG

        # Refresh the token and get the updated kubeconfig
        kubeconfig_dict = refresh_oidc_token(kubeconfig_path)

        # Load the kubeconfig from the updated dictionary
        k8s_config.load_kube_config_from_dict(config_dict=kubeconfig_dict)

        # Create the CoreV1Api client
        api_client = client.CoreV1Api()
        return api_client

    except Exception as e:
        print(f"Failed to load kubeconfig: {e}")
        return None


def fetch_jobs_for_submission(submission_id: UUID) -> list[dict[str, Any]]:
    """Fetch Kubernetes jobs for a specific submission."""
    try:
        k8s = get_k8s_v1()
        if not k8s:
            raise Exception("Kubernetes client initialization failed")
        jobs = k8s.list_namespaced_pod(config.NAMESPACE).items
        filtered_jobs = [
            job for job in jobs if str(submission_id) in job.metadata.name
        ]
        job_status = []
        api = ApiClient()
        for job in filtered_jobs:
            job_data = api.sanitize_for_serialization(job)
            job_status.append(
                KubernetesExecutionStatus(
                    job_id=job_data["metadata"].get("name"),
                    status=job_data["status"].get("phase"),
                    time_started=job_data["status"].get("startTime"),
                )
            )
        return job_status
    except Exception as e:
        print(f"Error fetching jobs for submission {submission_id}: {e}")
        return []


@cache.early(ttl="5m", early_ttl="5s", key="submission:{submission_id}:jobs")
async def get_cached_submission_jobs(
    submission_id: UUID,
) -> list[dict[str, Any]]:
    """Fetch cached jobs for submission asynchronously."""
    return await run_in_threadpool(fetch_jobs_for_submission, submission_id)


def get_k8s_custom_objects() -> client.CustomObjectsApi:
    try:

        # Return the CustomObjectsApi client with the updated kubeconfig
        return client.CustomObjectsApi()

    except Exception as e:
        print(f"Failed to load kubeconfig: {e}")
        return None


def fetch_kubernetes_status():
    """This function contains blocking code to fetch Kubernetes status."""
    try:
        k8s = get_k8s_v1()
        if not k8s:
            raise Exception("Kubernetes client initialization failed")
        ret = k8s.list_namespaced_pod(config.NAMESPACE)
        pods_info = [  # Retrieve only the necessary information
            {
                "name": pod.metadata.name,
                "status": pod.status.phase,
            }
            for pod in ret.items
        ]

        api = ApiClient()
        k8s_jobs = api.sanitize_for_serialization(pods_info)
        kubernetes_status = True
    except Exception as e:
        print(f"Error fetching Kubernetes status: {e}")
        k8s_jobs = []
        kubernetes_status = False
    return k8s_jobs, kubernetes_status


@cache.early(ttl="30s", early_ttl="10s", key="k8s:status")
async def get_kubernetes_status() -> Any:
    """Offload the blocking Kubernetes status fetch to a thread."""

    print("Fetching Kubernetes status...")
    return await run_in_threadpool(fetch_kubernetes_status)


def get_jobs_for_submission(
    k8s: CoreV1Api,
    submission_id: UUID,
) -> list[dict[str, Any]]:
    jobs = k8s.list_namespaced_pod(config.NAMESPACE)
    jobs = jobs.items
    jobs = [job for job in jobs if str(submission_id) in job.metadata.name]
    job_status = []
    for job in jobs:
        api = ApiClient()
        job_data = api.sanitize_for_serialization(job)
        job_status.append(
            KubernetesExecutionStatus(
                job_id=job_data["metadata"].get("name"),
                status=job_data["status"].get("phase"),
                time_started=job_data["status"].get("startTime"),
            )
        )
    return job_status


def delete_job(k8s: CustomObjectsApi, job_name: str) -> bool:
    env = os.environ.copy()
    env["KUBECONFIG"] = config.KUBECONFIG
    print(f"Deleting job {job_name} of project {config.PROJECT}")
    api_response = k8s.delete_namespaced_custom_object(
        namespace=config.NAMESPACE,
        plural="trainingworkloads",
        name=job_name,
        group="run.ai",
        version="v2alpha1",
    )
    if (
        api_response
        and "status" in api_response
        and api_response["status"] == "Success"
    ):
        return True

    return False


def fetch_job_log(job_id: str) -> str:
    """Fetch Kubernetes job logs."""
    k8s = get_k8s_v1()
    if k8s:
        log = k8s.read_namespaced_pod_log(
            name=str(job_id), namespace=config.NAMESPACE
        )
        return "\n".join([line.split("\r")[-1] for line in log.split("\n")])
    return "No logs available"


@cache.early(ttl="30m", early_ttl="10s", key="job:{job_id}:log")
async def get_cached_job_log(job_id: str) -> str:
    print(f"Fetching job log for {job_id}")
    return await run_in_threadpool(fetch_job_log, job_id)


def fetch_jobs():
    jobs = []
    try:
        k8s = get_k8s_v1()
        if not k8s:
            return []
        jobs = k8s.list_namespaced_pod(config.NAMESPACE)
    except Exception as e:
        print(f"Error fetching jobs: {e}")
        jobs = []
    return jobs


@cache.early(ttl="30m", early_ttl="10s", key="jobs:all")
async def fetch_cached_jobs():
    print("Fetching all k8s jobs...")
    return await run_in_threadpool(fetch_jobs)


def submit_job(
    k8s: CustomObjectsApi,
    name: str,
    fps: int,
    timestamp: str,
    submission_id: UUID,
    input_object_ids: list[UUID],
):

    submission_id = str(submission_id)
    input_object_ids = [str(obj_id) for obj_id in input_object_ids]
    job = {
        "apiVersion": "run.ai/v2alpha1",
        "kind": "TrainingWorkload",
        "metadata": {
            "name": name,
            "namespace": config.NAMESPACE,
            "labels": {"project": config.PROJECT},
        },
        "spec": {
            "environment": {
                "items": {
                    "FPS": {"value": str(fps)},
                    "INPUT_OBJECT_IDS": {
                        "value": json.dumps(input_object_ids)
                    },
                    "S3_ACCESS_KEY": {"value": config.S3_ACCESS_KEY},
                    "S3_BUCKET_ID": {"value": config.S3_BUCKET_ID},
                    "S3_PREFIX": {"value": config.S3_PREFIX},
                    "S3_SECRET_KEY": {"value": config.S3_SECRET_KEY},
                    "S3_URL": {"value": config.S3_URL},
                    "SUBMISSION_ID": {"value": str(submission_id)},
                    "TIMESTAMP": {"value": timestamp},
                }
            },
            "gpu": {
                "value": "1",
            },
            "image": {
                "value": (
                    f"{config.DEEPREEFMAP_IMAGE}:"
                    f"{config.DEEPREEFMAP_IMAGE_TAG}"
                ),
            },
            "imagePullPolicy": {
                "value": "Always",
            },
            "name": {
                "value": name,
            },
            "runAsGid": {
                "value": 1000,
            },
            "runAsUid": {
                "value": 1000,
            },
            "runAsUser": {
                "value": True,
            },
        },
    }

    # Create the job
    api_response = k8s.create_namespaced_custom_object(
        namespace=config.NAMESPACE,
        plural="trainingworkloads",
        body=job,
        version="v2alpha1",
        group="run.ai",
    )

    return api_response
