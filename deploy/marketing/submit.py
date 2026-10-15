#!/usr/bin/env python3
# Copyright Proa Labs, Inc.
# SPDX-License-Identifier: Apache-2.0

"""Run Cloud Build from CI and return the verified image's immutable reference."""

import json
import os
from pathlib import Path
import re
import signal
import subprocess
import time


def gcloud(*args):
    return subprocess.check_output(["gcloud", *args, "--quiet", "--format=json"], text=True)


if __name__ == "__main__":
    revision = os.environ["GITHUB_SHA"]
    if not re.fullmatch(r"[0-9a-f]{40}", revision):
        raise ValueError("GITHUB_SHA must be a complete commit ID")
    tag = f"{revision}-{os.environ['GITHUB_RUN_ID']}-{os.environ['GITHUB_RUN_ATTEMPT']}"
    result = json.loads(gcloud(
        "builds", "submit", ".", "--async", "--config=deploy/marketing/cloudbuild.yaml",
        "--project=airy-cycle-504821-k9",
        "--service-account=projects/airy-cycle-504821-k9/serviceAccounts/proa-marketing-builder@airy-cycle-504821-k9.iam.gserviceaccount.com",
        "--gcs-source-staging-dir=gs://airy-cycle-504821-k9-marketing-cache/sources",
        f"--substitutions=_REVISION={tag}",
    ))
    build_id = result["id"]
    print(f"Release build: https://console.cloud.google.com/cloud-build/builds/{build_id}?project=airy-cycle-504821-k9", flush=True)

    def cancel(signum, _frame):
        subprocess.run(["gcloud", "builds", "cancel", build_id, "--project=airy-cycle-504821-k9", "--quiet"], check=False)
        raise SystemExit(128 + signum)

    signal.signal(signal.SIGTERM, cancel)
    signal.signal(signal.SIGINT, cancel)
    while result["status"] in ("QUEUED", "WORKING", "PENDING"):
        time.sleep(10)
        result = json.loads(gcloud("builds", "describe", build_id, "--project=airy-cycle-504821-k9"))
    if result["status"] != "SUCCESS":
        raise RuntimeError(f"Release image build {build_id}: {result['status']}")
    expected = f"us-west1-docker.pkg.dev/airy-cycle-504821-k9/releases/proa-marketing:{tag}"
    matches = [image for image in result["results"]["images"] if image["name"] == expected]
    if len(matches) != 1 or not re.fullmatch(r"sha256:[0-9a-f]{64}", matches[0]["digest"]):
        raise RuntimeError("Cloud Build did not return the exact requested image and digest")
    image = f"{expected.rsplit(':', 1)[0]}@{matches[0]['digest']}"
    Path("release-image.json").write_text(json.dumps({
        "revision": revision, "image": image, "build_id": build_id,
        "seconds": result.get("timing", {}),
    }, indent=2) + "\n")
    with open(os.environ["GITHUB_OUTPUT"], "a") as output:
        output.write(f"image={image}\n")
    with open(os.environ["GITHUB_STEP_SUMMARY"], "a") as output:
        output.write(f"Release image built and smoke-tested: `{image}`\n\nCommit: `{revision}`\n")
