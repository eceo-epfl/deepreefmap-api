from kubernetes import client, config as k8s_config
from functools import lru_cache


@lru_cache()
def get_k8s_v1() -> client.CoreV1Api:
    k8s_config.load_kube_config()
    return client.CoreV1Api()


@lru_cache()
def get_k8s_custom_objects() -> client.CoreV1Api:
    k8s_config.load_kube_config()
    return client.CustomObjectsApi()
