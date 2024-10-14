from kubernetes import client, config as k8s_config
from app.config import config
from uuid import UUID
from app.submissions.models import (
    KubernetesExecutionStatus,
)
from typing import Any
from kubernetes.client import CoreV1Api, ApiClient
import subprocess
import os
from cashews import cache


def get_k8s_v1() -> client.CoreV1Api | None:
    try:
        list_jobs_runai()
        k8s_config.load_kube_config(config_file=config.KUBECONFIG)
    except Exception:
        return None

    return client.CoreV1Api()


def get_k8s_custom_objects() -> client.CoreV1Api:
    k8s_config.load_kube_config(config_file=config.KUBECONFIG)
    return client.CustomObjectsApi()


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
    """Executes the runai command to delete a job"""
    env = os.environ.copy()
    env["KUBECONFIG"] = config.KUBECONFIG

    # Use subprocess to use the runai interface to delete job
    subprocess.run(
        [
            "runai",
            "delete",
            "job",
            "-p",
            config.PROJECT,
            job_name,
        ],
        env=env,
        check=True,
    )

    return


def list_jobs_runai():
    """Executes a runai list jobs command

    This function will list all jobs in the current project.. BUT! It will
    also refresh the oidc token in the kubeconfig which expires faster than
    the key itself. So we should run this with the healthz
    """

    env = os.environ.copy()
    env["KUBECONFIG"] = config.KUBECONFIG

    # Use subprocess to use the runai interface to delete job
    result = subprocess.run(
        [
            "runai",
            "list",
            "projects",
        ],
        env=env,
        check=True,
        capture_output=True,
    )

    return result.stdout.decode("utf-8")


@cache.early(ttl="30s", early_ttl="10s", key="k8s:status")
async def get_kubernetes_status():
    print("Fetching Kubernetes status")
    try:
        k8s = get_k8s_v1()
        ret = k8s.list_namespaced_pod(config.NAMESPACE)
        api = ApiClient()
        k8s_jobs = api.sanitize_for_serialization(ret.items)
        kubernetes_status = True
    except Exception:
        k8s_jobs = []

    return k8s_jobs, kubernetes_status
