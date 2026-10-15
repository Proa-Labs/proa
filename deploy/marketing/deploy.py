#!/usr/bin/env python3
# Copyright Proa Labs, Inc.
# SPDX-License-Identifier: Apache-2.0

"""Deploy a CI-verified image digest to one Railway service and await success."""

import json
import os
from pathlib import Path
import re
import subprocess
import time
import urllib.request


def api(query, variables):
    request = urllib.request.Request(
        "https://backboard.railway.app/graphql/v2",
        data=json.dumps({"query": query, "variables": variables}).encode(),
        headers={"Project-Access-Token": os.environ["RAILWAY_TOKEN"],
                 "Content-Type": "application/json", "User-Agent": "proa-release-ci/1"},
    )
    with urllib.request.urlopen(request, timeout=30) as response:
        result = json.load(response)
    if result.get("errors"):
        raise RuntimeError(result["errors"])
    return result["data"]


def deploy(image, service, environment):
    if not re.fullmatch(r"us-west1-docker\.pkg\.dev/airy-cycle-504821-k9/releases/proa-marketing@sha256:[0-9a-f]{64}", image):
        raise ValueError("Deploy requires a complete digest from the marketing release repository")
    variables = {"service": service, "environment": environment}
    registry_token = subprocess.check_output([
        "gcloud", "auth", "print-access-token", "--impersonate-service-account",
        "proa-railway-pull@airy-cycle-504821-k9.iam.gserviceaccount.com", "--quiet",
    ], text=True).strip()
    api("""mutation($service:String!,$environment:String!,$input:ServiceInstanceUpdateInput!) {
        serviceInstanceUpdate(serviceId:$service,environmentId:$environment,input:$input)
    }""", variables | {"input": {
        "source": {"image": image, "repo": None}, "railwayConfigFile": None,
        "healthcheckPath": "/healthz", "healthcheckTimeout": 60,
        "restartPolicyType": "ON_FAILURE", "restartPolicyMaxRetries": 3,
        "registryCredentials": {"username": "oauth2accesstoken", "password": registry_token},
    }})
    # Verify the source before requesting deployment. A source update can be
    # staged by Railway; fail instead of redeploying the previous Git source.
    instance = api("""query($service:String!,$environment:String!) {
        serviceInstance(serviceId:$service,environmentId:$environment) { source { image repo } }
    }""", variables)["serviceInstance"]
    if instance["source"]["image"] != image or instance["source"].get("repo"):
        raise RuntimeError("Railway did not apply the requested image source")
    # Image sources do not need GitHub build triggers. Remove only the
    # selected environment's main trigger, leaving PR environments intact.
    triggers = api("""query($project:String!,$service:String!,$environment:String!) {
        deploymentTriggers(projectId:$project,serviceId:$service,environmentId:$environment,first:100) {
            edges { node { id repository branch environmentId } }
        }
    }""", variables | {"project": "36d8b540-8f29-4abf-befa-6f3f80646be7"})["deploymentTriggers"]
    for edge in triggers["edges"]:
        trigger = edge["node"]
        if trigger["environmentId"] != environment or trigger["repository"] != "Proa-Labs/proa" or trigger["branch"] != "main":
            raise RuntimeError("Unexpected Railway trigger; refusing to remove it")
        api("mutation($id:String!){deploymentTriggerDelete(id:$id)}", {"id": trigger["id"]})
    deployment = api("""mutation($service:String!,$environment:String!) {
        serviceInstanceDeployV2(serviceId:$service,environmentId:$environment)
    }""", variables)["serviceInstanceDeployV2"]
    started = time.monotonic()
    for _ in range(120):
        data = api("query($id:String!){deployment(id:$id){status meta}}", {"id": deployment})["deployment"]
        if data["status"] == "SUCCESS":
            if (data.get("meta") or {}).get("imageDigest") != image.split("@", 1)[1]:
                raise RuntimeError("Railway deployed a different image digest")
            return {"deployment": deployment, "image": image, "seconds": round(time.monotonic() - started, 3)}
        if data["status"] in ("FAILED", "CRASHED", "REMOVED", "SKIPPED"):
            raise RuntimeError(f"Railway deployment {deployment}: {data['status']}")
        time.sleep(5)
    raise TimeoutError(f"Railway deployment {deployment} did not finish within ten minutes")


if __name__ == "__main__":
    result = deploy(os.environ["RELEASE_IMAGE"], os.environ["RAILWAY_SERVICE_ID"], os.environ["RAILWAY_ENVIRONMENT_ID"])
    Path("railway-release.json").write_text(json.dumps(result, indent=2) + "\n")
    print(json.dumps(result))
    if os.environ.get("GITHUB_STEP_SUMMARY"):
        with open(os.environ["GITHUB_STEP_SUMMARY"], "a") as output:
            output.write(f"Railway deployed the verified release image in **{result['seconds']}s**.\n\nDeployment: `{result['deployment']}`\n")
