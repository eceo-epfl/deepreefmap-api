from kubernetes import client, config as k8s_config
from app.config import config
from uuid import UUID
from app.submissions.models import KubernetesExecutionStatus
from typing import Any
from kubernetes.client import CoreV1Api, ApiClient
from cashews import cache
from fastapi.concurrency import run_in_threadpool
import subprocess
import os
import json


def get_k8s_v1() -> client.CoreV1Api | None:
    try:
        list_jobs_runai()
        k8s_config.load_kube_config(config_file=config.KUBECONFIG)
    except Exception:
        return None
    return client.CoreV1Api()


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
                    submission_id=job_data["metadata"].get("name"),
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


def get_k8s_custom_objects() -> client.CoreV1Api:
    k8s_config.load_kube_config(config_file=config.KUBECONFIG)
    return client.CustomObjectsApi()


def fetch_kubernetes_status():
    """This function contains blocking code to fetch Kubernetes status."""
    try:
        k8s = get_k8s_v1()
        if not k8s:
            raise Exception("Kubernetes client initialization failed")
        ret = k8s.list_namespaced_pod(config.NAMESPACE)
        api = ApiClient()
        k8s_jobs = api.sanitize_for_serialization(ret.items)
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
                submission_id=job_data["metadata"].get("name"),
                status=job_data["status"].get("phase"),
                time_started=job_data["status"].get("startTime"),
            )
        )
    return job_status


def delete_job(job_name: str):
    env = os.environ.copy()
    env["KUBECONFIG"] = config.KUBECONFIG
    subprocess.run(
        ["runai", "delete", "job", "-p", config.PROJECT, job_name],
        env=env,
        check=True,
    )


def list_jobs_runai():
    env = os.environ.copy()
    env["KUBECONFIG"] = config.KUBECONFIG
    result = subprocess.run(
        ["runai", "list", "projects"],
        env=env,
        check=True,
        capture_output=True,
    )
    return result.stdout.decode("utf-8")


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
    k8s: CoreV1Api,
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
