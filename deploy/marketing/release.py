#!/usr/bin/env python3
# Copyright Proa Labs, Inc.
# SPDX-License-Identifier: Apache-2.0

"""Build a verified marketing image or deploy its immutable digest to Railway."""

import argparse
import json
import os
from pathlib import Path
import re
import signal
import subprocess
import tempfile
import time
import urllib.error
import urllib.request


RUNTIME_REPOSITORY = "ghcr.io/proa-labs/proa-marketing"


def package_visibility(token):
    request = urllib.request.Request(
        "https://api.github.com/orgs/Proa-Labs/packages/container/proa-marketing",
        headers={"Authorization": f"Bearer {token}", "Accept": "application/vnd.github+json",
                 "User-Agent": "proa-release-ci/1"},
    )
    try:
        with urllib.request.urlopen(request, timeout=30) as response:
            return json.load(response)["visibility"]
    except urllib.error.HTTPError as error:
        if error.code == 404:
            return None  # First push creates a private package by default.
        raise


def publish_runtime_image(source, tag):
    """Copy only the checked runtime image; keep compiler caches in GCP."""
    token = os.environ["GHCR_PUSH_TOKEN"]
    if package_visibility(token) not in (None, "private"):
        raise RuntimeError("Refusing to publish the runtime image to a non-private GHCR package")
    destination = f"{RUNTIME_REPOSITORY}:{tag}"
    with tempfile.TemporaryDirectory(prefix="proa-release-auth-") as directory:
        environment = os.environ | {"DOCKER_CONFIG": directory}
        google_token = subprocess.check_output(
            ["gcloud", "auth", "print-access-token", "--quiet",
             "--impersonate-service-account=proa-railway-pull@airy-cycle-504821-k9.iam.gserviceaccount.com"],
            text=True, timeout=60,
        ).strip()
        for registry, username, password in (
            ("us-west1-docker.pkg.dev", "oauth2accesstoken", google_token),
            ("ghcr.io", os.environ["GHCR_USERNAME"], token),
        ):
            subprocess.run(
                ["crane", "auth", "login", registry, "--username", username, "--password-stdin"],
                input=password, text=True, check=True, env=environment, timeout=60,
            )
        subprocess.run(["crane", "copy", source, destination], check=True,
                       env=environment, timeout=600)
        digest = subprocess.check_output(["crane", "digest", destination], text=True,
                                         env=environment, timeout=60).strip()
    if digest != source.split("@", 1)[1]:
        raise RuntimeError("GHCR image digest differs from the image checked by Cloud Build")
    if package_visibility(token) != "private":
        raise RuntimeError("GHCR runtime package must remain private")
    return f"{RUNTIME_REPOSITORY}@{digest}"


def gcloud(*args):
    return subprocess.check_output(["gcloud", *args, "--quiet", "--format=json"], text=True, timeout=120)


def await_build(result):
    """Cancel a submitted build whenever polling exits without a terminal result."""
    build_id = result["id"]
    finished = False
    failures = 0
    try:
        while True:
            if result["status"] not in ("QUEUED", "WORKING", "PENDING", "STATUS_UNKNOWN"):
                finished = True
                return result
            time.sleep(10)
            try:
                result = json.loads(gcloud("builds", "describe", build_id, "--project=airy-cycle-504821-k9"))
                failures = 0
            except (subprocess.SubprocessError, json.JSONDecodeError):
                failures += 1
                if failures >= 3:
                    raise
    finally:
        if not finished:
            subprocess.run([
                "gcloud", "builds", "cancel", build_id,
                "--project=airy-cycle-504821-k9", "--quiet",
            ], check=True, timeout=60)


def build_image():
    revision = os.environ["GITHUB_SHA"]
    if not re.fullmatch(r"[0-9a-f]{40}", revision):
        raise ValueError("GITHUB_SHA must be a complete commit ID")
    tag = f"{revision}-{os.environ['GITHUB_RUN_ID']}-{os.environ['GITHUB_RUN_ATTEMPT']}"
    result = json.loads(gcloud(
        "builds", "submit", ".", "--async", "--config=deploy/marketing/cloudbuild.yaml",
        "--project=airy-cycle-504821-k9",
        "--service-account=projects/airy-cycle-504821-k9/serviceAccounts/proa-marketing-builder@airy-cycle-504821-k9.iam.gserviceaccount.com",
        "--gcs-source-staging-dir=gs://airy-cycle-504821-k9-marketing-cache/sources",
        f"--substitutions=_REVISION={tag},_COMMIT={revision}",
    ))
    build_id = result["id"]
    print(f"Release build: https://console.cloud.google.com/cloud-build/builds/{build_id}?project=airy-cycle-504821-k9", flush=True)

    result = await_build(result)
    if result["status"] != "SUCCESS":
        raise RuntimeError(f"Release image build {build_id}: {result['status']}")
    expected = f"us-west1-docker.pkg.dev/airy-cycle-504821-k9/releases/proa-marketing:{tag}"
    matches = [image for image in result["results"]["images"] if image["name"] == expected]
    if len(matches) != 1 or not re.fullmatch(r"sha256:[0-9a-f]{64}", matches[0]["digest"]):
        raise RuntimeError("Cloud Build did not return the exact requested image and digest")
    source_image = f"{expected.rsplit(':', 1)[0]}@{matches[0]['digest']}"
    image = publish_runtime_image(source_image, tag)
    Path("release-image.json").write_text(json.dumps({
        "revision": revision, "image": image, "source_image": source_image, "build_id": build_id,
        "seconds": result.get("timing", {}),
    }, indent=2) + "\n")
    with open(os.environ["GITHUB_OUTPUT"], "a") as output:
        output.write(f"image={image}\n")
    with open(os.environ["GITHUB_STEP_SUMMARY"], "a") as output:
        output.write(f"Release image built and smoke-tested: `{image}`\n\nCommit: `{revision}`\n")


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
        healthcheckPath healthcheckTimeout restartPolicyType restartPolicyMaxRetries
        latestDeployment { id meta }
    }
}"""
UPDATE = """mutation($service:String!,$environment:String!,$input:ServiceInstanceUpdateInput!) {
    serviceInstanceUpdate(serviceId:$service,environmentId:$environment,input:$input)
}"""


def require_registry_credentials(variables):
    # Railway masks stored credentials. Never rotate them during cutover: a
    # failed staging operation could not restore the previous password.
    config = api("""query($environment:String!) {
        environment(id:$environment) { config(decryptVariables:false) }
    }""", variables)["environment"]["config"]
    credentials = (config.get("services", {}).get(variables["service"], {})
                   .get("deploy", {}).get("registryCredentials"))
    if not credentials or not credentials.get("username") or not credentials.get("password"):
        raise RuntimeError("Production cutover blocked: configure durable private-registry credentials in Railway first")
    if credentials["username"] == "oauth2accesstoken":
        raise RuntimeError("Production cutover blocked: expiring private-registry credentials are not supported")


def recover_deployment(image, variables, previous_id):
    """Reconcile an uncertain submission without repeating it or undoing it."""
    for attempt in range(12):
        if attempt:
            time.sleep(5)
        try:
            instance = api(INSTANCE_QUERY, variables)["serviceInstance"]
        except Exception:
            continue
        source = instance.get("source") or {}
        deployment = instance.get("latestDeployment") or {}
        if source.get("image") != image or source.get("repo"):
            break
        if (deployment.get("id") and deployment["id"] != previous_id
                and (deployment.get("meta") or {}).get("imageDigest") == image.split("@", 1)[1]):
            return deployment["id"]
    raise RuntimeError("Railway deployment request has an uncertain outcome; source was left untouched. "
                       "Inspect deployments and Git triggers before retrying cutover")


def stage_and_deploy(image, variables):
    previous = api(INSTANCE_QUERY, variables)["serviceInstance"]
    request_attempted = False
    try:
        api(UPDATE, variables | {"input": {
            "source": {"image": image, "repo": None}, "railwayConfigFile": None,
            "healthcheckPath": "/healthz", "healthcheckTimeout": 60,
            "restartPolicyType": "ON_FAILURE", "restartPolicyMaxRetries": 3,
        }})
        instance = api(INSTANCE_QUERY, variables)["serviceInstance"]
        if instance["source"]["image"] != image or instance["source"].get("repo"):
            raise RuntimeError("Railway did not apply the requested image source")
        previous_id = (instance.get("latestDeployment") or {}).get("id")
        # Mark uncertainty before the call: a timeout or signal cannot prove
        # that Railway rejected it. Never restore a source after this point.
        request_attempted = True
        try:
            return api("""mutation($service:String!,$environment:String!) {
                serviceInstanceDeployV2(serviceId:$service,environmentId:$environment)
            }""", variables)["serviceInstanceDeployV2"]
        except Exception:
            return recover_deployment(image, variables, previous_id)
    finally:
        if not request_attempted:
            # Handle API errors and SIGTERM between staging and requesting the
            # deployment. Never replace a source changed by another operator.
            current = api(INSTANCE_QUERY, variables)["serviceInstance"]
            if current.get("source") == {"image": image, "repo": None}:
                api(UPDATE, variables | {"input": {
                    "source": previous["source"] or {"image": None, "repo": None},
                    **{key: previous[key] for key in (
                        "railwayConfigFile", "healthcheckPath", "healthcheckTimeout",
                        "restartPolicyType", "restartPolicyMaxRetries",
                    )},
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
    if not re.fullmatch(re.escape(RUNTIME_REPOSITORY) + r"@sha256:[0-9a-f]{64}", image):
        raise ValueError("Deploy requires a complete digest from the marketing release repository")
    variables = {"service": service, "environment": environment}
    require_registry_credentials(variables)
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
    deployment = stage_and_deploy(image, variables)
    # Remove triggers only once the deployment ID is known, including recovery
    # after a lost response. An uncertain submission never rolls back its source.
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


def deploy_image():
    result = deploy(os.environ["RELEASE_IMAGE"], os.environ["RAILWAY_SERVICE_ID"],
                    os.environ["RAILWAY_ENVIRONMENT_ID"], os.environ["GITHUB_SHA"],
                    os.environ["RAILWAY_READY_URL"])
    Path("railway-release.json").write_text(json.dumps(result, indent=2) + "\n")
    print(json.dumps(result))
    if os.environ.get("GITHUB_STEP_SUMMARY"):
        with open(os.environ["GITHUB_STEP_SUMMARY"], "a") as output:
            output.write(f"Railway deployed the verified release image in **{result['seconds']}s**.\n\nDeployment: `{result['deployment']}`\n")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("command", choices=("build", "deploy"))
    args = parser.parse_args()

    def cancel(signum, _frame):
        raise SystemExit(128 + signum)

    signal.signal(signal.SIGTERM, cancel)
    signal.signal(signal.SIGINT, cancel)
    if args.command == "build":
        build_image()
    else:
        deploy_image()


if __name__ == "__main__":
    main()
