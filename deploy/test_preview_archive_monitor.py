# Copyright Proa Labs, Inc.
# SPDX-License-Identifier: Apache-2.0

from __future__ import annotations

from contextlib import contextmanager
from http.server import BaseHTTPRequestHandler, HTTPServer, ThreadingHTTPServer
import json
import os
from pathlib import Path
import socket
import socketserver
import stat
import subprocess
import tempfile
import threading
import time
import unittest


ROOT = Path(__file__).resolve().parents[1]
SERVER = ROOT / "deploy" / "server"
MONITOR = SERVER / "proa-preview-archive-monitor"


def non_loopback_ipv4() -> str:
    probe = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
    try:
        # Connecting a UDP socket selects a local interface without sending a
        # packet. The documentation-only target cannot answer this probe.
        probe.connect(("192.0.2.1", 9))
        address = probe.getsockname()[0]
    except OSError as error:
        raise unittest.SkipTest("no non-loopback IPv4 interface available") from error
    finally:
        probe.close()
    if address == "0.0.0.0" or address.startswith("127."):
        raise unittest.SkipTest("no non-loopback IPv4 interface available")
    return address


class _HealthHandler(BaseHTTPRequestHandler):
    status = 200
    payload: object = {"ok": True}

    def do_GET(self) -> None:  # noqa: N802 - stdlib handler API
        body = json.dumps(self.payload, separators=(",", ":")).encode()
        self.send_response(self.status)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def log_message(self, _format: str, *_args: object) -> None:
        return


@contextmanager
def health_server(*, status: int = 200, payload: object = None):
    handler = type(
        "HealthHandler",
        (_HealthHandler,),
        {
            "status": status,
            "payload": {"ok": True} if payload is None else payload,
        },
    )
    server = HTTPServer(("127.0.0.1", 0), handler)
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    try:
        yield server.server_port
    finally:
        server.shutdown()
        server.server_close()
        thread.join(timeout=2)


@contextmanager
def malformed_health_server():
    class MalformedHealthHandler(socketserver.BaseRequestHandler):
        def handle(self) -> None:
            self.request.recv(4096)
            self.request.sendall(b"not-http\r\n\r\n")

    server = socketserver.TCPServer(("127.0.0.1", 0), MalformedHealthHandler)
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    try:
        yield server.server_address[1]
    finally:
        server.shutdown()
        server.server_close()
        thread.join(timeout=2)


@contextmanager
def router_server(
    *,
    status: int = 200,
    body: bytes = b"OK",
    bind_address: str = "127.0.0.1",
):
    class RouterHandler(BaseHTTPRequestHandler):
        def do_GET(self) -> None:  # noqa: N802 - stdlib handler API
            self.send_response(status)
            self.send_header("Content-Type", "text/plain")
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)

        def log_message(self, _format: str, *_args: object) -> None:
            return

    server = HTTPServer((bind_address, 0), RouterHandler)
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    try:
        yield server.server_port
    finally:
        server.shutdown()
        server.server_close()
        thread.join(timeout=2)


@contextmanager
def exact_route_server(
    *,
    hostname: str,
    revision: str,
    status: int = 200,
    body: bytes = b"ready",
    bind_address: str = "127.0.0.1",
):
    class ExactRouteHandler(BaseHTTPRequestHandler):
        def do_GET(self) -> None:  # noqa: N802 - stdlib handler API
            response_body = body
            response_status = status
            if self.path != "/readyz" or self.headers.get("Host") != hostname:
                response_status = 404
                response_body = b"not found"
            self.send_response(response_status)
            self.send_header("Content-Type", "text/plain")
            self.send_header("Content-Length", str(len(response_body)))
            self.send_header("Cache-Control", "no-store")
            self.send_header("x-proa-revision", revision)
            self.end_headers()
            self.wfile.write(response_body)

        def log_message(self, _format: str, *_args: object) -> None:
            return

    server = HTTPServer((bind_address, 0), ExactRouteHandler)
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    try:
        yield server.server_port
    finally:
        server.shutdown()
        server.server_close()
        thread.join(timeout=2)


@contextmanager
def exact_routes_server(
    routes: dict[str, tuple[int, str]], *, delay_seconds: float = 0.0
):
    tracker: dict[str, object] = {
        "attempted": [],
        "active": 0,
        "max_active": 0,
    }
    tracker_lock = threading.Lock()

    class ExactRoutesHandler(BaseHTTPRequestHandler):
        def do_GET(self) -> None:  # noqa: N802 - stdlib handler API
            hostname = self.headers.get("Host", "")
            with tracker_lock:
                attempted = tracker["attempted"]
                assert isinstance(attempted, list)
                attempted.append(hostname)
                active = tracker["active"]
                assert isinstance(active, int)
                active += 1
                tracker["active"] = active
                maximum = tracker["max_active"]
                assert isinstance(maximum, int)
                tracker["max_active"] = max(maximum, active)
            # Leave the counted window before writing the response. A client
            # worker starts its next probe as soon as it has read this one, so
            # counting the response write and the bookkeeping after it lets a
            # finished handler overlap its own successor and report more
            # concurrency than the client pool can actually reach.
            try:
                if delay_seconds:
                    time.sleep(delay_seconds)
            finally:
                with tracker_lock:
                    active = tracker["active"]
                    assert isinstance(active, int)
                    tracker["active"] = active - 1

            expected = routes.get(hostname)
            if self.path != "/readyz" or expected is None:
                response_status = 404
                revision = "0" * 40
                response_body = b"not found"
            else:
                response_status, revision = expected
                response_body = b"ready" if response_status == 200 else b"unhealthy"
            self.send_response(response_status)
            self.send_header("Content-Type", "text/plain")
            self.send_header("Content-Length", str(len(response_body)))
            self.send_header("Cache-Control", "no-store")
            self.send_header("x-proa-revision", revision)
            self.end_headers()
            self.wfile.write(response_body)

        def log_message(self, _format: str, *_args: object) -> None:
            return

    class ExactRoutesServer(ThreadingHTTPServer):
        request_queue_size = 32

    server = ExactRoutesServer(("127.0.0.1", 0), ExactRoutesHandler)
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    try:
        yield server.server_port, tracker
    finally:
        server.shutdown()
        server.server_close()
        thread.join(timeout=2)


class PreviewArchiveMonitorTest(unittest.TestCase):
    def setUp(self) -> None:
        self.temporary = tempfile.TemporaryDirectory()
        self.state = Path(self.temporary.name) / "state"
        self.state.mkdir(mode=0o700)
        self.state.chmod(0o700)
        self.routes = self.state / "routes"
        self.routes.mkdir(mode=0o755)
        (self.state / "deployments").mkdir(mode=0o700)

    def tearDown(self) -> None:
        self.temporary.cleanup()

    def run_monitor(
        self,
        *arguments: str,
        now: int = 1_000,
        port: int | None = None,
        router_port: int | None = None,
        **environment: str,
    ) -> subprocess.CompletedProcess[str]:
        env = os.environ.copy()
        env.update(
            {
                "PROA_ARCHIVE_STATE_DIR": str(self.state),
                "PROA_ARCHIVE_NOW_EPOCH": str(now),
                "PROA_ARCHIVE_ROUTER_CONFIG_DIR": str(self.routes),
                "PROA_ARCHIVE_ROUTER_IP": "127.0.0.1",
            }
        )
        if port is not None:
            env["PROA_ARCHIVE_CONTROL_PORT"] = str(port)
        if router_port is not None:
            env["PROA_ARCHIVE_ROUTER_HEALTH_PORT"] = str(router_port)
        env.update(environment)
        return subprocess.run(
            [str(MONITOR), *arguments],
            check=False,
            capture_output=True,
            text=True,
            env=env,
            timeout=5,
        )

    def test_successful_sync_records_private_atomic_heartbeat(self) -> None:
        result = self.run_monitor("record-success", now=1_234)

        self.assertEqual(result.returncode, 0, result.stderr)
        heartbeat = self.state / "last-success"
        self.assertEqual(heartbeat.read_text(), "1234\n")
        self.assertEqual(stat.S_IMODE(heartbeat.stat().st_mode), 0o600)
        self.assertIn("event=heartbeat_recorded epoch=1234", result.stdout)

    def test_fresh_heartbeat_and_controller_are_healthy(self) -> None:
        (self.state / "last-success").write_text("900\n")
        (self.state / "last-success").chmod(0o600)
        with health_server() as port, router_server() as archive_router_port:
            result = self.run_monitor(
                "check",
                now=1_000,
                port=port,
                router_port=archive_router_port,
            )

        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("event=healthy heartbeat_age_seconds=100", result.stdout)
        self.assertIn(f"router_port={archive_router_port}", result.stdout)
        self.assertEqual(result.stderr, "")

    def test_router_health_and_routes_use_configured_ipv4_but_control_stays_local(self) -> None:
        (self.state / "last-success").write_text("900\n")
        (self.state / "last-success").chmod(0o600)
        hostname, revision, _deployment = self.write_route()
        router_host = non_loopback_ipv4()
        with (
            health_server() as control_port,
            router_server(bind_address=router_host) as health_port,
            exact_route_server(
                hostname=hostname,
                revision=revision,
                bind_address=router_host,
            ) as traffic_port,
        ):
            result = self.run_monitor(
                "check",
                now=1_000,
                port=control_port,
                router_port=health_port,
                PROA_ARCHIVE_ROUTER_IP=router_host,
                PROA_ARCHIVE_ROUTER_PORT=str(traffic_port),
            )

        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("route_checked=true", result.stdout)

    def test_router_host_requires_a_canonical_ipv4_address(self) -> None:
        (self.state / "last-success").write_text("900\n")
        (self.state / "last-success").chmod(0o600)
        for value in (
            "localhost",
            "::1",
            "127.000.0.1",
            "127.0.0.1 ",
            "2130706433",
        ):
            with self.subTest(value=value):
                result = self.run_monitor(
                    "check",
                    now=1_000,
                    PROA_ARCHIVE_ROUTER_IP=value,
                )

                self.assertNotEqual(result.returncode, 0)
                self.assertIn("event=configuration_invalid", result.stderr)

    def test_stale_or_missing_heartbeat_emits_priority_alert(self) -> None:
        missing = self.run_monitor("check", now=1_000)
        self.assertNotEqual(missing.returncode, 0)
        self.assertIn("<1>preview_archive_alert event=heartbeat_missing", missing.stderr)

        (self.state / "last-success").write_text("100\n")
        (self.state / "last-success").chmod(0o600)
        stale = self.run_monitor(
            "check",
            now=1_000,
            PROA_ARCHIVE_HEARTBEAT_MAX_AGE_SECONDS="120",
        )
        self.assertNotEqual(stale.returncode, 0)
        self.assertIn("event=heartbeat_stale age_seconds=900", stale.stderr)

    def test_malformed_or_future_heartbeat_fails_closed(self) -> None:
        heartbeat = self.state / "last-success"
        heartbeat.write_text("not-an-epoch\n")
        heartbeat.chmod(0o600)
        malformed = self.run_monitor("check", now=1_000)
        self.assertNotEqual(malformed.returncode, 0)
        self.assertIn("event=heartbeat_malformed", malformed.stderr)

        heartbeat.write_text("1031\n")
        future = self.run_monitor("check", now=1_000)
        self.assertNotEqual(future.returncode, 0)
        self.assertIn("event=heartbeat_from_future", future.stderr)

    def test_unhealthy_controller_emits_fixed_alert_without_body(self) -> None:
        (self.state / "last-success").write_text("900\n")
        (self.state / "last-success").chmod(0o600)
        with (
            health_server(status=503, payload={"secret": "must-not-log"}) as port,
            router_server() as archive_router_port,
        ):
            result = self.run_monitor(
                "check", now=1_000, port=port, router_port=archive_router_port
            )

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("event=control_unhealthy", result.stderr)
        self.assertNotIn("must-not-log", result.stderr)

        with malformed_health_server() as port, router_server() as archive_router_port:
            malformed = self.run_monitor(
                "check", now=1_000, port=port, router_port=archive_router_port
            )

        self.assertNotEqual(malformed.returncode, 0)
        self.assertIn("event=control_unhealthy", malformed.stderr)
        self.assertNotIn("event=internal_error", malformed.stderr)

    def test_unhealthy_archive_router_emits_fixed_alert_without_body(self) -> None:
        (self.state / "last-success").write_text("900\n")
        (self.state / "last-success").chmod(0o600)
        with health_server() as port, router_server(
            status=503, body=b"secret must not log"
        ) as archive_router_port:
            result = self.run_monitor(
                "check", now=1_000, port=port, router_port=archive_router_port
            )

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("event=router_unhealthy", result.stderr)
        self.assertNotIn("secret", result.stderr)

    def write_route(
        self,
        *,
        deployment: str = "a" * 24,
        revision: str = "b" * 40,
    ) -> tuple[str, str, str]:
        hostname = f"proa-marketing-{deployment[:12]}.proa.cool"
        router = f"proa-preview-archive-{deployment[:12]}"
        route = {
            "http": {
                "middlewares": {f"{router}-gzip": {"compress": {}}},
                "routers": {
                    router: {
                        "entryPoints": ["archive"],
                        "middlewares": [f"{router}-gzip"],
                        "rule": f"Host(`{hostname}`) && PathPrefix(`/`)",
                        "service": router,
                    }
                },
                "services": {
                    router: {
                        "loadBalancer": {
                            "servers": [
                                {
                                    "url": f"http://proa-preview-archive-{deployment}:5555"
                                }
                            ]
                        }
                    }
                },
            }
        }
        route_path = self.routes / f"{deployment}.yml"
        route_path.write_text(json.dumps(route, sort_keys=True) + "\n")
        route_path.chmod(0o644)
        metadata = self.state / "deployments" / deployment
        metadata.write_text(f"14 {revision} 900 sha256:{'c' * 64}\n")
        metadata.chmod(0o600)
        return hostname, revision, deployment

    def test_monitor_attests_a_real_exact_route_and_revision(self) -> None:
        (self.state / "last-success").write_text("900\n")
        (self.state / "last-success").chmod(0o600)
        hostname, revision, _deployment = self.write_route()
        with (
            health_server() as control_port,
            router_server() as health_port,
            exact_route_server(hostname=hostname, revision=revision) as traffic_port,
        ):
            result = self.run_monitor(
                "check",
                now=1_000,
                port=control_port,
                router_port=health_port,
                PROA_ARCHIVE_ROUTER_PORT=str(traffic_port),
            )

        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("route_checked=true", result.stdout)
        self.assertIn("routes_checked=1", result.stdout)

    def test_monitor_probes_every_exact_route(self) -> None:
        (self.state / "last-success").write_text("900\n")
        (self.state / "last-success").chmod(0o600)
        route_responses: dict[str, tuple[int, str]] = {}
        for marker in ("a", "b", "c"):
            hostname, revision, _deployment = self.write_route(
                deployment=marker * 24,
                revision=marker * 40,
            )
            route_responses[hostname] = (200, revision)

        with (
            health_server() as control_port,
            router_server() as health_port,
            exact_routes_server(route_responses) as (traffic_port, tracker),
        ):
            result = self.run_monitor(
                "check",
                now=1_000,
                port=control_port,
                router_port=health_port,
                PROA_ARCHIVE_ROUTER_PORT=str(traffic_port),
            )

        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(sorted(tracker["attempted"]), sorted(route_responses))
        self.assertIn("routes_checked=3", result.stdout)

    def test_later_unhealthy_route_fails_after_every_route_is_probed(self) -> None:
        (self.state / "last-success").write_text("900\n")
        (self.state / "last-success").chmod(0o600)
        route_responses: dict[str, tuple[int, str]] = {}
        for marker in ("a", "b", "c"):
            hostname, revision, _deployment = self.write_route(
                deployment=marker * 24,
                revision=marker * 40,
            )
            route_responses[hostname] = (200, revision)
        second_hostname = sorted(route_responses)[1]
        route_responses[second_hostname] = (200, "d" * 40)

        with (
            health_server() as control_port,
            router_server() as health_port,
            exact_routes_server(route_responses) as (traffic_port, tracker),
        ):
            result = self.run_monitor(
                "check",
                now=1_000,
                port=control_port,
                router_port=health_port,
                PROA_ARCHIVE_ROUTER_PORT=str(traffic_port),
            )

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("event=route_unhealthy", result.stderr)
        self.assertEqual(sorted(tracker["attempted"]), sorted(route_responses))

    def test_later_malformed_route_is_not_masked_by_valid_first_route(self) -> None:
        (self.state / "last-success").write_text("900\n")
        (self.state / "last-success").chmod(0o600)
        self.write_route(deployment="a" * 24, revision="a" * 40)
        self.write_route(deployment="b" * 24, revision="b" * 40)
        malformed = self.routes / f"{'b' * 24}.yml"
        malformed.write_text('{"http": {}}\n')
        malformed.chmod(0o644)

        with health_server() as control_port, router_server() as health_port:
            result = self.run_monitor(
                "check",
                now=1_000,
                port=control_port,
                router_port=health_port,
            )

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("event=route_state_malformed", result.stderr)

    def test_route_directory_rejects_every_unexpected_entry(self) -> None:
        (self.state / "last-success").write_text("900\n")
        (self.state / "last-success").chmod(0o600)

        def create_file(name: str) -> None:
            (self.routes / name).write_text("unexpected\n")

        def create_directory(name: str) -> None:
            (self.routes / name).mkdir()

        def create_symlink(name: str) -> None:
            (self.routes / name).symlink_to(self.state / "last-success")

        cases = (
            ("yaml", "route.yaml", create_file),
            ("toml", "route.toml", create_file),
            ("dotfile", ".route.json", create_file),
            ("legacy-json", f"{'d' * 24}.json", create_file),
            ("directory", "route-directory", create_directory),
            ("symlink", f"{'c' * 24}.yml", create_symlink),
        )
        for label, name, create in cases:
            with self.subTest(entry=label):
                entry = self.routes / name
                create(name)
                try:
                    with (
                        health_server() as control_port,
                        router_server() as health_port,
                    ):
                        result = self.run_monitor(
                            "check",
                            now=1_000,
                            port=control_port,
                            router_port=health_port,
                        )

                    self.assertNotEqual(result.returncode, 0)
                    self.assertIn("event=route_state_unsafe", result.stderr)
                finally:
                    if entry.is_dir() and not entry.is_symlink():
                        entry.rmdir()
                    else:
                        entry.unlink(missing_ok=True)

    def test_route_count_is_hard_capped_at_thirty(self) -> None:
        (self.state / "last-success").write_text("900\n")
        (self.state / "last-success").chmod(0o600)
        for index in range(31):
            self.write_route(
                deployment=f"{index:012x}{'a' * 12}",
                revision=f"{index:040x}",
            )

        with health_server() as control_port, router_server() as health_port:
            result = self.run_monitor(
                "check",
                now=1_000,
                port=control_port,
                router_port=health_port,
            )

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("event=route_limit_exceeded", result.stderr)

    def test_route_probe_pool_is_bounded_and_fits_monitor_deadline(self) -> None:
        (self.state / "last-success").write_text("900\n")
        (self.state / "last-success").chmod(0o600)
        route_responses: dict[str, tuple[int, str]] = {}
        for index in range(30):
            hostname, revision, _deployment = self.write_route(
                deployment=f"{index:012x}{'a' * 12}",
                revision=f"{index:040x}",
            )
            route_responses[hostname] = (200, revision)

        with (
            health_server() as control_port,
            router_server() as health_port,
            exact_routes_server(route_responses, delay_seconds=0.15) as (
                traffic_port,
                tracker,
            ),
        ):
            started = time.monotonic()
            result = self.run_monitor(
                "check",
                now=1_000,
                port=control_port,
                router_port=health_port,
                PROA_ARCHIVE_ROUTER_PORT=str(traffic_port),
            )
            elapsed = time.monotonic() - started

        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(len(tracker["attempted"]), 30)
        self.assertGreater(tracker["max_active"], 1)
        self.assertLessEqual(tracker["max_active"], 8)
        self.assertLess(elapsed, 4.0)

    def test_ping_cannot_mask_a_missing_or_wrong_exact_route(self) -> None:
        (self.state / "last-success").write_text("900\n")
        (self.state / "last-success").chmod(0o600)
        hostname, revision, _deployment = self.write_route()
        with (
            health_server() as control_port,
            router_server() as health_port,
            exact_route_server(
                hostname=hostname, revision="d" * 40, status=200
            ) as traffic_port,
        ):
            result = self.run_monitor(
                "check",
                now=1_000,
                port=control_port,
                router_port=health_port,
                PROA_ARCHIVE_ROUTER_PORT=str(traffic_port),
            )

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("event=route_unhealthy", result.stderr)
        self.assertNotIn(revision, result.stderr)

    def test_route_config_must_be_root_owned_and_nonwritable(self) -> None:
        (self.state / "last-success").write_text("900\n")
        (self.state / "last-success").chmod(0o600)
        self.write_route()
        next(self.routes.glob("*.yml")).chmod(0o666)
        with health_server() as port, router_server() as health_port:
            result = self.run_monitor(
                "check", now=1_000, port=port, router_port=health_port
            )

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("event=route_state_unsafe", result.stderr)

    def test_failure_report_accepts_only_archive_unit_identities(self) -> None:
        accepted = self.run_monitor(
            "unit-failure", "proa-preview-archive-sync.service"
        )
        self.assertEqual(accepted.returncode, 0, accepted.stderr)
        self.assertIn(
            "event=unit_failed unit=proa-preview-archive-sync.service",
            accepted.stderr,
        )

        rejected = self.run_monitor("unit-failure", "ssh.service")
        self.assertNotEqual(rejected.returncode, 0)
        self.assertIn("event=unit_identity_invalid", rejected.stderr)
        self.assertNotIn("ssh.service", rejected.stderr)

    def test_units_record_success_alert_failures_and_check_staleness(self) -> None:
        sync = (SERVER / "proa-preview-archive-sync.service").read_text()
        control = (SERVER / "proa-preview-archive-control.service").read_text()
        router = (SERVER / "proa-preview-archive-router.service").read_text()
        monitor = (SERVER / "proa-preview-archive-monitor.service").read_text()
        timer = (SERVER / "proa-preview-archive-monitor.timer").read_text()
        alert = (SERVER / "proa-preview-archive-alert@.service").read_text()

        self.assertIn("OnFailure=proa-preview-archive-alert@%n.service", sync)
        self.assertIn(
            "Requires=docker.service proa-preview-archive-router.service", sync
        )
        self.assertIn(
            "After=docker.service network-online.target proa-preview-archive-router.service",
            sync,
        )
        self.assertIn("OnFailure=proa-preview-archive-alert@%n.service", control)
        self.assertIn("OnFailure=proa-preview-archive-alert@%n.service", router)
        self.assertIn("ExecStart=/usr/local/sbin/proa-preview-archive-router run", router)
        self.assertIn(
            "ExecStartPost=/usr/local/sbin/proa-preview-archive-monitor record-success",
            sync,
        )
        self.assertIn("ExecStart=/usr/local/sbin/proa-preview-archive-monitor check", monitor)
        self.assertNotIn("ConditionPathExists=", monitor)
        self.assertIn("OnFailure=proa-preview-archive-alert@%n.service", monitor)
        self.assertEqual(
            [line for line in monitor.splitlines() if line.startswith("IPAddressAllow=")],
            ["IPAddressAllow=localhost", "IPAddressAllow=172.31.252.2/32"],
        )
        self.assertIn("PROA_ARCHIVE_ROUTER_IP=172.31.252.2", monitor)
        self.assertIn("PROA_ARCHIVE_ROUTER_HEALTH_PORT=8083", monitor)
        self.assertIn("PROA_ARCHIVE_ROUTER_PORT=8082", monitor)
        self.assertIn("Unit=proa-preview-archive-monitor.service", timer)
        self.assertIn(
            "ExecStart=/usr/local/sbin/proa-preview-archive-monitor unit-failure %i",
            alert,
        )


if __name__ == "__main__":
    unittest.main()
