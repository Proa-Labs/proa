# Copyright Proa Labs, Inc.
# SPDX-License-Identifier: Apache-2.0

from __future__ import annotations

import hashlib
import hmac
import http.client
import fcntl
import json
import os
from pathlib import Path
import runpy
import socket
import stat
import subprocess
import sys
import tempfile
import time
from types import SimpleNamespace
import unittest
from unittest import mock

from deploy.test_preview_archive_sync import (
    APPLICATION_UUID,
    FAKE_DOCKER,
    IMAGE_ID,
    REVISION,
    archive_container,
)


ROOT = Path(__file__).resolve().parents[1]
CONTROL = ROOT / "deploy/server/proa-preview-archive-control"
CONTROL_PATH = "/_proa/origin/preview-archive"
SECRET = "7" * 64
NOW = 2_000_000
DEPLOYMENT = "dy2vxce7udvt9q2otsiwb6o1"

# A control request can legitimately spend one second waiting for sync.lock,
# run two sequential Docker commands with two-second deadlines, and then fsync
# its durable state. Keep the client deadline above that composed server budget
# so filesystem scheduling jitter is not mistaken for a hung request.
CONTROL_REQUEST_TIMEOUT_SECONDS = 10


class PreviewArchiveControlTest(unittest.TestCase):
    def setUp(self) -> None:
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.state_path = self.root / "docker-state.json"
        self.archive_state = self.root / "archive-state"
        self.archive_state.mkdir(mode=0o700)
        self.routes = self.archive_state / "routes"
        self.routes.mkdir(mode=0o755)
        self.fake_docker = self.root / "docker"
        self.fake_docker.write_text(FAKE_DOCKER)
        self.fake_docker.chmod(self.fake_docker.stat().st_mode | stat.S_IXUSR)
        self.secret_file = self.root / "archive-control.secret"
        self.secret_file.write_text(SECRET + "\n")
        self.secret_file.chmod(0o600)
        self.port = self.available_port()
        self.process: subprocess.Popen[str] | None = None

        self.write_archive(created=NOW - 100)
        self.start_server()

    def tearDown(self) -> None:
        if self.process is not None:
            if self.process.poll() is None:
                self.process.terminate()
            try:
                self.process.communicate(timeout=3)
            except subprocess.TimeoutExpired:
                self.process.kill()
                self.process.communicate(timeout=3)

    @staticmethod
    def available_port() -> int:
        with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as listener:
            listener.bind(("127.0.0.1", 0))
            return int(listener.getsockname()[1])

    def write_archive(self, *, created: int, healthy: bool = True) -> None:
        name, archive = archive_container(
            14,
            DEPLOYMENT,
            created,
            healthy=healthy,
        )
        self.state_path.write_text(
            json.dumps(
                {
                    "commands": [],
                    "containers": {name: archive},
                    "images": [IMAGE_ID],
                    "networks": ["proa-preview"],
                }
            )
        )
        deployments = self.archive_state / "deployments"
        deployments.mkdir(mode=0o700, parents=True, exist_ok=True)
        (deployments / DEPLOYMENT).write_text(
            f"14 {REVISION} {created} {IMAGE_ID}\n"
        )

    def start_server(self) -> None:
        env = os.environ.copy()
        env.update(
            {
                "FAKE_DOCKER_STATE": str(self.state_path),
                "PROA_ARCHIVE_APPLICATION_UUID": APPLICATION_UUID,
                "PROA_ARCHIVE_APPLICATION_NAME": "proa-marketing",
                "PROA_ARCHIVE_PREVIEW_DOMAIN": "proa.cool",
                "PROA_ARCHIVE_STATE_DIR": str(self.archive_state),
                "PROA_ARCHIVE_ROUTER_CONFIG_DIR": str(self.routes),
                "PROA_ARCHIVE_DOCKER_BIN": str(self.fake_docker),
                "PROA_ARCHIVE_CONTROL_SECRET_FILE": str(self.secret_file),
                "PROA_ARCHIVE_CONTROL_PORT": str(self.port),
                "PROA_ARCHIVE_CONTROL_NOW_EPOCH": str(NOW),
                "PROA_ARCHIVE_CONTROL_LOCK_TIMEOUT_SECONDS": "1",
                "PROA_ARCHIVE_CONTROL_DOCKER_TIMEOUT_SECONDS": "2",
                "PROA_ARCHIVE_PROVISIONAL_TTL_SECONDS": "900",
            }
        )
        self.process = subprocess.Popen(
            [sys.executable, str(CONTROL)],
            cwd=ROOT,
            env=env,
            text=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
        )
        deadline = time.monotonic() + 5
        while time.monotonic() < deadline:
            if self.process.poll() is not None:
                stdout, stderr = self.process.communicate(timeout=1)
                self.fail(f"control service exited early\nstdout={stdout}\nstderr={stderr}")
            try:
                with socket.create_connection(("127.0.0.1", self.port), timeout=0.1):
                    return
            except OSError:
                time.sleep(0.02)
        self.fail("control service did not start")

    def payload(self, operation: str) -> dict[str, object]:
        return {
            "v": 1,
            "operation": operation,
            "application_uuid": APPLICATION_UUID,
            "pr_number": 14,
            "expected_commit": REVISION,
            "deployment_uuid": DEPLOYMENT,
            "run_id": 123456789,
            "run_attempt": 2,
            "issued_at": NOW,
        }

    def request(
        self,
        payload: dict[str, object],
        *,
        timestamp: int | None = None,
        signing_secret: str = SECRET,
        signature: str | None = None,
    ) -> tuple[int, dict[str, object]]:
        body = json.dumps(payload, separators=(",", ":"), sort_keys=True).encode()
        timestamp = int(payload["issued_at"]) if timestamp is None else timestamp
        timestamp_text = str(timestamp)
        canonical = (
            f"v1\nPOST\n{CONTROL_PATH}\n{timestamp_text}\n".encode() + body
        )
        digest = hmac.new(
            signing_secret.encode(), canonical, hashlib.sha256
        ).hexdigest()
        headers = {
            "Content-Type": "application/json",
            "X-Proa-Archive-Timestamp": timestamp_text,
            "X-Proa-Archive-Signature": signature or f"sha256={digest}",
        }
        connection = http.client.HTTPConnection(
            "127.0.0.1",
            self.port,
            timeout=CONTROL_REQUEST_TIMEOUT_SECONDS,
        )
        try:
            connection.request("POST", CONTROL_PATH, body=body, headers=headers)
            response = connection.getresponse()
            response_body = json.loads(response.read())
            return response.status, response_body
        finally:
            connection.close()

    def docker_state(self) -> dict[str, object]:
        return json.loads(self.state_path.read_text())

    def write_route(self) -> Path:
        route = self.routes / f"{DEPLOYMENT}.yml"
        route.write_text('{"http":{}}\n')
        route.chmod(0o644)
        return route

    def test_health_check_is_available_without_control_authentication(self) -> None:
        connection = http.client.HTTPConnection(
            "127.0.0.1",
            self.port,
            timeout=CONTROL_REQUEST_TIMEOUT_SECONDS,
        )
        try:
            connection.request("GET", "/healthz")
            response = connection.getresponse()
            body = json.loads(response.read())
        finally:
            connection.close()
        self.assertEqual(response.status, 200)
        self.assertEqual(body, {"ok": True})

    def test_promote_writes_exact_approval_and_is_idempotent(self) -> None:
        status, body = self.request(self.payload("promote"))
        self.assertEqual(status, 200, body)
        self.assertEqual(body["result"], "promoted")
        marker = self.archive_state / "approved" / DEPLOYMENT
        self.assertEqual(
            marker.read_text().split(),
            ["14", REVISION, str(NOW), IMAGE_ID],
        )
        claim = self.archive_state / "hostname-claims" / DEPLOYMENT[:12]
        self.assertEqual(claim.read_text().split(), [DEPLOYMENT])

        repeated_status, repeated_body = self.request(self.payload("promote"))
        self.assertEqual(repeated_status, 200, repeated_body)
        self.assertEqual(repeated_body["result"], "promoted")

    def test_fresh_promotion_rejects_a_colliding_hostname_claim(self) -> None:
        colliding_deployment = DEPLOYMENT[:12] + "0" * 12
        claim = self.archive_state / "hostname-claims" / DEPLOYMENT[:12]
        claim.write_text(colliding_deployment + "\n")

        status, body = self.request(self.payload("promote"))

        self.assertEqual(status, 409, body)
        self.assertEqual(body["error"], "hostname_claim_conflict")
        self.assertEqual(claim.read_text().split(), [colliding_deployment])
        self.assertFalse((self.archive_state / "approved" / DEPLOYMENT).exists())

    def test_idempotent_promotion_revalidates_the_hostname_claim(self) -> None:
        status, body = self.request(self.payload("promote"))
        self.assertEqual(status, 200, body)

        claim = self.archive_state / "hostname-claims" / DEPLOYMENT[:12]
        claim.unlink()
        repeated_status, repeated_body = self.request(self.payload("promote"))
        self.assertEqual(repeated_status, 200, repeated_body)
        self.assertEqual(claim.read_text().split(), [DEPLOYMENT])

        colliding_deployment = DEPLOYMENT[:12] + "0" * 12
        claim.write_text(colliding_deployment + "\n")

        repeated_status, repeated_body = self.request(self.payload("promote"))

        self.assertEqual(repeated_status, 409, repeated_body)
        self.assertEqual(repeated_body["error"], "hostname_claim_conflict")
        self.assertEqual(claim.read_text().split(), [colliding_deployment])
        self.assertTrue((self.archive_state / "approved" / DEPLOYMENT).exists())

    def test_revoke_is_idempotent_for_an_unapproved_archive(self) -> None:
        route = self.write_route()
        status, body = self.request(self.payload("revoke"))
        self.assertEqual(status, 200, body)
        self.assertEqual(body["result"], "revoked")
        archive_name = f"proa-preview-archive-{DEPLOYMENT}"
        state = self.docker_state()
        self.assertNotIn(archive_name, state["containers"])
        self.assertIn(["rm", "-fv", archive_name], state["commands"])
        retired = self.archive_state / "retired" / DEPLOYMENT
        self.assertEqual(retired.read_text().split(), ["14", REVISION, str(NOW)])
        self.assertFalse(route.exists())

        original_tombstone = f"14 {REVISION} {NOW - 500}\n"
        retired.write_text(original_tombstone)
        route = self.write_route()
        repeated_status, repeated_body = self.request(self.payload("revoke"))
        self.assertEqual(repeated_status, 200, repeated_body)
        self.assertEqual(repeated_body["result"], "revoked")
        self.assertEqual(retired.read_text(), original_tombstone)
        self.assertFalse(route.exists())

    def test_present_revoke_preserves_a_matching_retired_tombstone(self) -> None:
        retired = self.archive_state / "retired" / DEPLOYMENT
        original_tombstone = f"14 {REVISION} {NOW - 500}\n"
        retired.write_text(original_tombstone)
        route = self.write_route()

        status, body = self.request(self.payload("revoke"))

        self.assertEqual(status, 200, body)
        self.assertEqual(body["result"], "revoked")
        self.assertEqual(retired.read_text(), original_tombstone)
        self.assertFalse(route.exists())
        archive_name = f"proa-preview-archive-{DEPLOYMENT}"
        self.assertNotIn(archive_name, self.docker_state()["containers"])

    def test_present_revoke_rejects_a_conflicting_retired_tombstone(self) -> None:
        retired = self.archive_state / "retired" / DEPLOYMENT
        conflicting_tombstone = f"15 {'a' * 40} {NOW - 500}\n"
        retired.write_text(conflicting_tombstone)
        route = self.write_route()

        status, body = self.request(self.payload("revoke"))

        self.assertEqual(status, 409, body)
        self.assertEqual(body["error"], "retired_identity_mismatch")
        self.assertEqual(retired.read_text(), conflicting_tombstone)
        self.assertTrue(route.exists())
        archive_name = f"proa-preview-archive-{DEPLOYMENT}"
        self.assertIn(archive_name, self.docker_state()["containers"])
        self.assertEqual(self.docker_state()["commands"], [])

    def test_absent_revoke_rejects_a_conflicting_retired_tombstone(self) -> None:
        state = self.docker_state()
        state["containers"] = {}
        self.state_path.write_text(json.dumps(state))
        (self.archive_state / "deployments" / DEPLOYMENT).unlink()
        retired = self.archive_state / "retired" / DEPLOYMENT
        conflicting_tombstone = f"15 {'a' * 40} {NOW - 500}\n"
        retired.write_text(conflicting_tombstone)
        route = self.write_route()

        status, body = self.request(self.payload("revoke"))

        self.assertEqual(status, 409, body)
        self.assertEqual(body["error"], "retired_identity_mismatch")
        self.assertEqual(retired.read_text(), conflicting_tombstone)
        self.assertTrue(route.exists())
        self.assertEqual(self.docker_state()["commands"], [])

    def test_revoke_tombstones_before_the_snapshotter_creates_local_state(self) -> None:
        state = self.docker_state()
        state["containers"] = {}
        self.state_path.write_text(json.dumps(state))
        (self.archive_state / "deployments" / DEPLOYMENT).unlink()

        status, body = self.request(self.payload("revoke"))

        self.assertEqual(status, 200, body)
        self.assertEqual(body["result"], "revoked")
        self.assertEqual(
            (self.archive_state / "retired" / DEPLOYMENT).read_text().split(),
            ["14", REVISION, str(NOW)],
        )
        claim = self.archive_state / "hostname-claims" / DEPLOYMENT[:12]
        self.assertEqual(claim.read_text().strip(), DEPLOYMENT)
        self.assertEqual(stat.S_IMODE(claim.stat().st_mode), 0o600)

    def test_revoke_unlinks_route_before_a_failed_docker_removal(self) -> None:
        route = self.write_route()
        state = self.docker_state()
        state["fail_rm"] = True
        self.state_path.write_text(json.dumps(state))

        status, body = self.request(self.payload("revoke"))

        self.assertEqual(status, 503, body)
        self.assertEqual(body["error"], "archive_removal_failed")
        self.assertFalse(route.exists())
        archive_name = f"proa-preview-archive-{DEPLOYMENT}"
        self.assertIn(archive_name, self.docker_state()["containers"])
        self.assertTrue((self.archive_state / "retired" / DEPLOYMENT).exists())

    def test_route_unlink_fsyncs_the_opened_parent_directory(self) -> None:
        route = self.write_route()
        legacy_route = self.routes / f"{DEPLOYMENT}.json"
        legacy_route.write_text('{"http":{"routers":{"legacy":{}}}}\n')
        legacy_route.chmod(0o644)
        namespace = runpy.run_path(str(CONTROL))
        fsync_targets: list[os.stat_result] = []
        real_fsync = os.fsync

        def record_fsync(descriptor: int) -> None:
            fsync_targets.append(os.fstat(descriptor))
            real_fsync(descriptor)

        with mock.patch.object(namespace["os"], "fsync", side_effect=record_fsync):
            namespace["unlink_archive_route"](
                SimpleNamespace(route_dir=self.routes), DEPLOYMENT
            )

        self.assertFalse(route.exists())
        self.assertFalse(legacy_route.exists())
        self.assertEqual(len(fsync_targets), 1)
        self.assertTrue(stat.S_ISDIR(fsync_targets[0].st_mode))

    def test_revoke_refuses_an_unsafe_replaced_route_directory(self) -> None:
        original_routes = self.routes.with_name("routes-original")
        self.routes.rename(original_routes)
        attacker_routes = self.root / "attacker-routes"
        attacker_routes.mkdir(mode=0o755)
        attacker_route = attacker_routes / f"{DEPLOYMENT}.yml"
        attacker_route.write_text('{"http":{"routers":{"attacker":{}}}}\n')
        self.routes.symlink_to(attacker_routes, target_is_directory=True)

        status, body = self.request(self.payload("revoke"))

        self.assertEqual(status, 503, body)
        self.assertEqual(body["error"], "route_state_unavailable")
        self.assertTrue(attacker_route.exists())
        archive_name = f"proa-preview-archive-{DEPLOYMENT}"
        self.assertIn(archive_name, self.docker_state()["containers"])

    def test_absent_revoke_rejects_a_colliding_hostname_claim(self) -> None:
        state = self.docker_state()
        state["containers"] = {}
        self.state_path.write_text(json.dumps(state))
        (self.archive_state / "deployments" / DEPLOYMENT).unlink()
        colliding_deployment = DEPLOYMENT[:12] + "000000000000"
        claim = self.archive_state / "hostname-claims" / DEPLOYMENT[:12]
        claim.write_text(colliding_deployment + "\n")

        status, body = self.request(self.payload("revoke"))

        self.assertEqual(status, 409, body)
        self.assertEqual(body["error"], "hostname_claim_conflict")
        self.assertEqual(claim.read_text().strip(), colliding_deployment)
        self.assertFalse((self.archive_state / "retired" / DEPLOYMENT).exists())

    def test_revoke_cannot_delete_an_approved_archive(self) -> None:
        promoted_status, promoted_body = self.request(self.payload("promote"))
        self.assertEqual(promoted_status, 200, promoted_body)

        status, body = self.request(self.payload("revoke"))
        self.assertEqual(status, 409, body)
        self.assertEqual(body["error"], "archive_already_approved")
        archive_name = f"proa-preview-archive-{DEPLOYMENT}"
        self.assertIn(archive_name, self.docker_state()["containers"])
        self.assertFalse((self.archive_state / "retired" / DEPLOYMENT).exists())

    def test_invalid_or_stale_hmac_is_rejected_without_mutation(self) -> None:
        status, body = self.request(
            self.payload("promote"),
            signing_secret="8" * 64,
        )
        self.assertEqual(status, 401, body)
        self.assertFalse((self.archive_state / "approved" / DEPLOYMENT).exists())

        stale_payload = self.payload("promote")
        stale_payload["issued_at"] = NOW - 61
        stale_status, stale_body = self.request(stale_payload)
        self.assertEqual(stale_status, 401, stale_body)
        self.assertEqual(stale_body["error"], "stale_signature")

    def test_request_shape_and_signed_timestamp_are_exact(self) -> None:
        extra = self.payload("promote")
        extra["unexpected"] = True
        status, body = self.request(extra)
        self.assertEqual(status, 400, body)
        self.assertEqual(body["error"], "invalid_request_shape")

        mismatch = self.payload("promote")
        status, body = self.request(mismatch, timestamp=NOW - 1)
        self.assertEqual(status, 400, body)
        self.assertEqual(body["error"], "issued_at_mismatch")

    def test_identity_mismatch_never_removes_container(self) -> None:
        state = self.docker_state()
        archive_name = f"proa-preview-archive-{DEPLOYMENT}"
        state["containers"][archive_name]["Config"]["Labels"][
            "proa.preview.archive.revision"
        ] = "a" * 40
        self.state_path.write_text(json.dumps(state))

        status, body = self.request(self.payload("revoke"))
        self.assertEqual(status, 409, body)
        self.assertEqual(body["error"], "archive_identity_mismatch")
        self.assertIn(archive_name, self.docker_state()["containers"])

    def test_mixed_case_protected_labels_are_rejected_without_mutation(self) -> None:
        archive_name = f"proa-preview-archive-{DEPLOYMENT}"
        cases = (
            ("Traefik.HTTP.Routers.rogue.Rule", "Host(`proa.so`)"),
            ("COOLIFY.Managed", "true"),
            ("Proa.Preview.Archive.Deployment", DEPLOYMENT),
        )
        for operation in ("promote", "revoke"):
            for label, value in cases:
                with self.subTest(operation=operation, label=label):
                    self.write_archive(created=NOW - 100)
                    state = self.docker_state()
                    state["containers"][archive_name]["Config"]["Labels"][label] = value
                    self.state_path.write_text(json.dumps(state))

                    status, body = self.request(self.payload(operation))

                    self.assertEqual(status, 409, body)
                    self.assertEqual(body["error"], "archive_identity_mismatch")
                    self.assertIn(archive_name, self.docker_state()["containers"])
                    self.assertFalse(
                        (self.archive_state / "approved" / DEPLOYMENT).exists()
                    )
                    self.assertFalse(
                        (self.archive_state / "retired" / DEPLOYMENT).exists()
                    )

    def test_runtime_contract_drift_never_promotes_or_removes_container(self) -> None:
        archive_name = f"proa-preview-archive-{DEPLOYMENT}"
        mutations = (
            lambda archive: archive["HostConfig"].__setitem__(
                "ReadonlyRootfs", False
            ),
            lambda archive: archive.__setitem__(
                "Mounts", [{"Type": "bind", "Destination": "/host"}]
            ),
            lambda archive: archive["NetworkSettings"]["Networks"].__setitem__(
                "bridge", {}
            ),
            lambda archive: archive["Config"].__setitem__("User", "0:0"),
        )
        for mutate in mutations:
            with self.subTest(mutation=mutate):
                self.write_archive(created=NOW - 100)
                state = self.docker_state()
                mutate(state["containers"][archive_name])
                self.state_path.write_text(json.dumps(state))

                status, body = self.request(self.payload("promote"))

                self.assertEqual(status, 409, body)
                self.assertEqual(body["error"], "archive_runtime_mismatch")
                self.assertIn(archive_name, self.docker_state()["containers"])

    def test_log_configuration_drift_cannot_promote_or_revoke(self) -> None:
        archive_name = f"proa-preview-archive-{DEPLOYMENT}"
        mutations = (
            lambda archive: archive["HostConfig"]["LogConfig"].__setitem__(
                "Type", "json-file"
            ),
            lambda archive: archive["HostConfig"]["LogConfig"]["Config"].__setitem__(
                "max-size", "100m"
            ),
            lambda archive: archive["HostConfig"]["LogConfig"]["Config"].__setitem__(
                "mode", "non-blocking"
            ),
        )
        for operation in ("promote", "revoke"):
            for mutate in mutations:
                with self.subTest(operation=operation, mutation=mutate):
                    self.write_archive(created=NOW - 100)
                    state = self.docker_state()
                    mutate(state["containers"][archive_name])
                    self.state_path.write_text(json.dumps(state))

                    status, body = self.request(self.payload(operation))

                    self.assertEqual(status, 409, body)
                    self.assertEqual(body["error"], "archive_runtime_mismatch")
                    self.assertIn(archive_name, self.docker_state()["containers"])
                    self.assertFalse(
                        (self.archive_state / "approved" / DEPLOYMENT).exists()
                    )
                    self.assertFalse(
                        (self.archive_state / "retired" / DEPLOYMENT).exists()
                    )

    def test_environment_drift_cannot_promote_or_revoke(self) -> None:
        archive_name = f"proa-preview-archive-{DEPLOYMENT}"

        def replace_environment(archive: dict, key: str, value: str) -> None:
            environment = archive["Config"]["Env"]
            environment[:] = [
                f"{key}={value}" if item.startswith(f"{key}=") else item
                for item in environment
            ]

        cases = (
            (
                "extra secret",
                lambda archive: archive["Config"]["Env"].append(
                    "SECRET_TOKEN=must-not-cross-the-control-boundary"
                ),
            ),
            (
                "duplicate key",
                lambda archive: archive["Config"]["Env"].append("RUST_LOG=trace"),
            ),
            (
                "malformed entry",
                lambda archive: archive["Config"]["Env"].append(
                    "not-an-environment-entry"
                ),
            ),
            (
                "attacker path",
                lambda archive: replace_environment(
                    archive, "PATH", "/app/bin:$PATH"
                ),
            ),
            (
                "invalid rust log",
                lambda archive: replace_environment(
                    archive, "RUST_LOG", "info;curl attacker.invalid"
                ),
            ),
            (
                "relative static directory",
                lambda archive: replace_environment(
                    archive, "STATIC_DIR", "app/static"
                ),
            ),
            (
                "relative showcase directory",
                lambda archive: replace_environment(
                    archive, "SHOWCASE_PUBLIC_DIR", "app/showcase"
                ),
            ),
            (
                "invalid load-test flag",
                lambda archive: replace_environment(
                    archive, "PROA_CONSOLE_LOAD_TESTS", "2"
                ),
            ),
        )
        for operation in ("promote", "revoke"):
            for case, mutate in cases:
                with self.subTest(operation=operation, case=case):
                    self.write_archive(created=NOW - 100)
                    state = self.docker_state()
                    mutate(state["containers"][archive_name])
                    self.state_path.write_text(json.dumps(state))

                    status, body = self.request(self.payload(operation))

                    self.assertEqual(status, 409, body)
                    self.assertEqual(body["error"], "archive_identity_mismatch")
                    self.assertIn(archive_name, self.docker_state()["containers"])
                    self.assertFalse(
                        (self.archive_state / "approved" / DEPLOYMENT).exists()
                    )
                    self.assertFalse(
                        (self.archive_state / "retired" / DEPLOYMENT).exists()
                    )

    def test_expired_provisional_archive_cannot_be_promoted(self) -> None:
        self.write_archive(created=NOW - 900)

        status, body = self.request(self.payload("promote"))
        self.assertEqual(status, 409, body)
        self.assertEqual(body["error"], "provisional_expired")
        archive_name = f"proa-preview-archive-{DEPLOYMENT}"
        self.assertNotIn(archive_name, self.docker_state()["containers"])
        self.assertEqual(
            (self.archive_state / "retired" / DEPLOYMENT).read_text().split(),
            ["14", REVISION, str(NOW)],
        )
        claim = self.archive_state / "hostname-claims" / DEPLOYMENT[:12]
        self.assertEqual(claim.read_text().strip(), DEPLOYMENT)

    def test_expiry_unlinks_route_before_a_failed_docker_removal(self) -> None:
        self.write_archive(created=NOW - 900)
        route = self.write_route()
        state = self.docker_state()
        state["fail_rm"] = True
        self.state_path.write_text(json.dumps(state))

        status, body = self.request(self.payload("promote"))

        self.assertEqual(status, 503, body)
        self.assertEqual(body["error"], "archive_removal_failed")
        self.assertFalse(route.exists())
        archive_name = f"proa-preview-archive-{DEPLOYMENT}"
        self.assertIn(archive_name, self.docker_state()["containers"])
        self.assertTrue((self.archive_state / "retired" / DEPLOYMENT).exists())

    def test_unhealthy_archive_cannot_be_promoted(self) -> None:
        self.write_archive(created=NOW - 100, healthy=False)

        status, body = self.request(self.payload("promote"))
        self.assertEqual(status, 409, body)
        self.assertEqual(body["error"], "archive_not_healthy")
        self.assertFalse((self.archive_state / "approved" / DEPLOYMENT).exists())

    def test_lock_contention_is_a_retryable_service_error(self) -> None:
        lock_path = self.archive_state / "sync.lock"
        with lock_path.open("a+") as lock_file:
            fcntl.flock(lock_file.fileno(), fcntl.LOCK_EX | fcntl.LOCK_NB)
            status, body = self.request(self.payload("promote"))
            fcntl.flock(lock_file.fileno(), fcntl.LOCK_UN)

        self.assertEqual(status, 503, body)
        self.assertEqual(body["error"], "archive_lock_timeout")
        self.assertFalse((self.archive_state / "approved" / DEPLOYMENT).exists())

    def test_docker_failure_is_retryable_not_misclassified_as_absent(self) -> None:
        self.state_path.unlink()

        status, body = self.request(self.payload("promote"))
        self.assertEqual(status, 503, body)
        self.assertEqual(body["error"], "docker_unavailable")


if __name__ == "__main__":
    unittest.main()
