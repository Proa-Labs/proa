#!/usr/bin/env python3
# Copyright Proa Labs, Inc.
# SPDX-License-Identifier: Apache-2.0

"""Deploy a CI-verified image digest to one Railway service and await success."""

import json
import os
from pathlib import Path
import re
import signal
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


INSTANCE_QUERY = """query($service:String!,$environment:String!) {
    serviceInstance(serviceId:$service,environmentId:$environment) {
        source { image repo } railwayConfigFile
    }
}"""
UPDATE = """mutation($service:String!,$environment:String!,$input:ServiceInstanceUpdateInput!) {
    serviceInstanceUpdate(serviceId:$service,environmentId:$environment,input:$input)
}"""


def stage_and_deploy(image, variables, registry_credentials):
    previous = api(INSTANCE_QUERY, variables)["serviceInstance"]
    requested = False
    try:
        api(UPDATE, variables | {"input": {
            "source": {"image": image, "repo": None}, "railwayConfigFile": None,
            "healthcheckPath": "/healthz", "healthcheckTimeout": 60,
            "restartPolicyType": "ON_FAILURE", "restartPolicyMaxRetries": 3,
            "registryCredentials": registry_credentials,
        }})
        instance = api(INSTANCE_QUERY, variables)["serviceInstance"]
        if instance["source"]["image"] != image or instance["source"].get("repo"):
            raise RuntimeError("Railway did not apply the requested image source")
        deployment = api("""mutation($service:String!,$environment:String!) {
            serviceInstanceDeployV2(serviceId:$service,environmentId:$environment)
        }""", variables)["serviceInstanceDeployV2"]
        requested = True
        return deployment
    finally:
        if not requested:
            # Handle API errors and SIGTERM between staging and requesting the
            # deployment. Never replace a source changed by another operator.
            current = api(INSTANCE_QUERY, variables)["serviceInstance"]
            if (current.get("source") or {}).get("image") == image:
                api(UPDATE, variables | {"input": {
                    "source": previous["source"] or {"image": None, "repo": None},
                    "railwayConfigFile": previous["railwayConfigFile"],
                }})


def verify_revision(url, revision):
    if not url.startswith("https://"):
        raise ValueError("Railway readiness verification requires an HTTPS URL")
    last = None
    for _ in range(12):
        try:
            request = urllib.request.Request(url, headers={"Cache-Control": "no-cache"})
            with urllib.request.urlopen(request, timeout=10) as response:
                if response.status == 200 and response.headers.get("x-proa-revision") == revision:
                    return
                last = response.headers.get("x-proa-revision")
        except OSError as error:
            last = str(error)
        time.sleep(5)
    raise RuntimeError(f"Railway /readyz did not serve commit {revision}; last response: {last}")


def deploy(image, service, environment, revision, ready_url):
    if not re.fullmatch(r"[0-9a-f]{40}", revision):
        raise ValueError("Deploy requires a complete source commit ID")
    if not ready_url.startswith("https://"):
        raise ValueError("Railway readiness verification requires an HTTPS URL")
    if not re.fullmatch(r"us-west1-docker\.pkg\.dev/airy-cycle-504821-k9/releases/proa-marketing@sha256:[0-9a-f]{64}", image):
        raise ValueError("Deploy requires a complete digest from the marketing release repository")
    registry_credentials = {
        "username": os.environ.get("RAILWAY_REGISTRY_USERNAME"),
        "password": os.environ.get("RAILWAY_REGISTRY_PASSWORD"),
    }
    if not all(registry_credentials.values()) or registry_credentials["username"] == "oauth2accesstoken":
        raise RuntimeError("Production cutover blocked: configure durable private-registry credentials first")
    variables = {"service": service, "environment": environment}
    # Image sources do not need GitHub build triggers. Remove only the
    # selected environment's main trigger, leaving PR environments intact.
    trigger_query = """query($project:String!,$service:String!,$environment:String!) {
        deploymentTriggers(projectId:$project,serviceId:$service,environmentId:$environment,first:100) {
            edges { node { id repository branch environmentId } }
        }
    }"""
    trigger_variables = variables | {"project": "36d8b540-8f29-4abf-befa-6f3f80646be7"}
    triggers = api(trigger_query, trigger_variables)["deploymentTriggers"]
    selected_triggers = [edge["node"] for edge in triggers["edges"]]
    for trigger in selected_triggers:
        if trigger["environmentId"] != environment or trigger["repository"] != "Proa-Labs/proa" or trigger["branch"] != "main":
            raise RuntimeError("Unexpected Railway trigger; refusing to remove it")
    deployment = stage_and_deploy(image, variables, registry_credentials)
    # Remove triggers only after deployment has been requested; cancellation
    # before that point restores the previous source in stage_and_deploy.
    remaining = api(trigger_query, trigger_variables)["deploymentTriggers"]
    remaining_ids = {edge["node"]["id"] for edge in remaining["edges"]}
    if not remaining_ids.issubset({trigger["id"] for trigger in selected_triggers}):
        raise RuntimeError("Railway triggers changed during cutover; refusing to remove them")
    for trigger_id in sorted(remaining_ids):
        api("mutation($id:String!){deploymentTriggerDelete(id:$id)}", {"id": trigger_id})
    started = time.monotonic()
    for _ in range(120):
        data = api("query($id:String!){deployment(id:$id){status meta}}", {"id": deployment})["deployment"]
        if data["status"] == "SUCCESS":
            if (data.get("meta") or {}).get("imageDigest") != image.split("@", 1)[1]:
                raise RuntimeError("Railway deployed a different image digest")
            verify_revision(ready_url, revision)
            return {"deployment": deployment, "image": image, "revision": revision, "seconds": round(time.monotonic() - started, 3)}
        if data["status"] in ("FAILED", "CRASHED", "REMOVED", "SKIPPED"):
            raise RuntimeError(f"Railway deployment {deployment}: {data['status']}")
        time.sleep(5)
    raise TimeoutError(f"Railway deployment {deployment} did not finish within ten minutes")


if __name__ == "__main__":
    def cancel(signum, _frame):
        raise SystemExit(128 + signum)

    signal.signal(signal.SIGTERM, cancel)
    signal.signal(signal.SIGINT, cancel)
    result = deploy(os.environ["RELEASE_IMAGE"], os.environ["RAILWAY_SERVICE_ID"],
                    os.environ["RAILWAY_ENVIRONMENT_ID"], os.environ["GITHUB_SHA"],
                    os.environ["RAILWAY_READY_URL"])
    Path("railway-release.json").write_text(json.dumps(result, indent=2) + "\n")
    print(json.dumps(result))
    if os.environ.get("GITHUB_STEP_SUMMARY"):
        with open(os.environ["GITHUB_STEP_SUMMARY"], "a") as output:
            output.write(f"Railway deployed the verified release image in **{result['seconds']}s**.\n\nDeployment: `{result['deployment']}`\n")
