"""Run opt-in API round trips against a dedicated Scality test prefix."""

import argparse
import os
import subprocess
import uuid
from pathlib import Path
from urllib.parse import urlparse


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--allow-remote-writes", action="store_true")
    args = parser.parse_args()
    if not args.allow_remote_writes:
        parser.error("Use --allow-remote-writes only after authorizing Scality test uploads")
    required = ("DATABASE_URL", "S3_URL", "S3_BUCKET_ID", "S3_ACCESS_KEY", "S3_SECRET_KEY")
    if any(not os.environ.get(key) for key in required):
        parser.error("Set DATABASE_URL and the S3 connection variables")
    if urlparse(os.environ["DATABASE_URL"]).hostname not in {"localhost", "127.0.0.1", "::1"}:
        parser.error("DATABASE_URL must point to a local scratch PostgreSQL server")
    env = dict(os.environ, S3_TEST_PREFIX=f"archive-smoke/{uuid.uuid4()}")
    print(f"Test objects will remain under {env['S3_TEST_PREFIX']} for inspection and operator cleanup", flush=True)
    root = Path(__file__).resolve().parents[1]
    for target in ("test_end_to_end_upload_and_download", "test_complete_verifies_small_objects_whole",
                   "test_content_md5_rejects_corruption_and_accepts_original_bytes"):
        subprocess.run(["cargo", "test", "--test", "archive", target, "--", "--exact", "--nocapture"],
                       cwd=root, env=env, check=True)


if __name__ == "__main__":
    main()
