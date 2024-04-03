from kubernetes import client, config as k8s_config
from app.config import config


def get_k8s_v1() -> client.CoreV1Api:
    k8s_config.load_kube_config(config_file=config.CONFIG_FILE)
    return client.CoreV1Api()


def get_k8s_custom_objects() -> client.CoreV1Api:
    k8s_config.load_kube_config(config_file=config.CONFIG_FILE)
    return client.CustomObjectsApi()
