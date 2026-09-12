#!/usr/bin/env python3
# Copyright Proa Labs, Inc.
# SPDX-License-Identifier: Apache-2.0

"""Exercise the archive dynamic-route contract against real Traefik 3.6."""

from __future__ import annotations

import argparse
import http.client
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
import os
from pathlib import Path
import secrets
import shutil
import socket
import subprocess
import sys
import tempfile
import threading
import time


ROOT = Path(__file__).resolve().parents[1]
STATIC_CONFIG = ROOT / "deploy/server/proa-preview-archive-router-static.yml"
TRAEFIK_IMAGE = os.environ.get(
    "PROA_TRAEFIK_TEST_IMAGE",
    (
        "traefik@sha256:"
        "31267173a15b4944e797a76ffd9c419707c8d8b32fe5b610f80cd0cfa05f372d"
    ),
)
BACKEND_IMAGE = os.environ.get(
    "PROA_TRAEFIK_TEST_BACKEND_IMAGE",
    (
        "traefik/whoami@sha256:"
        "200689790a0a0ea48ca45992e0450bc26ccab5307375b41c84dfc4f2475937ab"
    ),
)
HOSTNAME = "proa-marketing-abcdefghijkl.proa.cool"
NATIVE_BACKEND_BODY = b"archive-native-backend-ok\n"


def run(
    *arguments: str,
    check: bool = True,
    timeout: float = 30,
) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        arguments,
        check=check,
        capture_output=True,
        text=True,
        timeout=timeout,
    )


def docker_available() -> bool:
    if shutil.which("docker") is None:
        return False
    try:
        return run("docker", "info", timeout=10).returncode == 0
    except (OSError, subprocess.SubprocessError):
        return False


class NativeBackendHandler(BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"
    seen_requests: list[tuple[str, str]] = []

    def do_GET(self) -> None:  # noqa: N802 - BaseHTTPRequestHandler contract
        type(self).seen_requests.append((self.path, self.headers.get("Host", "")))
        if self.path != "/healthz":
            self.send_error(404)
            return
        self.send_response(200)
        self.send_header("Content-Type", "text/plain")
        self.send_header("Content-Length", str(len(NATIVE_BACKEND_BODY)))
        self.end_headers()
        self.wfile.write(NATIVE_BACKEND_BODY)

    def log_message(self, _format: str, *_arguments: object) -> None:
        return


def published_port(container: str, internal_port: int) -> int:
    output = run(
        "docker",
        "port",
        container,
        f"{internal_port}/tcp",
    ).stdout.strip()
    if "\n" in output or not output.startswith("127.0.0.1:"):
        raise RuntimeError(f"unexpected Docker port mapping: {output!r}")
    return int(output.rsplit(":", 1)[1])


def request(port: int, path: str, *, host: str | None = None) -> tuple[int, bytes]:
    connection = http.client.HTTPConnection("127.0.0.1", port, timeout=2)
    headers = {"Connection": "close"}
    if host is not None:
        headers["Host"] = host
    try:
        connection.request("GET", path, headers=headers)
        response = connection.getresponse()
        return response.status, response.read()
    finally:
        connection.close()


def await_contract(traffic_port: int, health_port: int) -> bytes:
    deadline = time.monotonic() + 20
    last: tuple[object, ...] = ()
    while time.monotonic() < deadline:
        try:
            ping = request(health_port, "/ping")
            routed = request(traffic_port, "/healthz", host=HOSTNAME)
            last = (ping, routed)
            if ping == (200, b"OK") and routed[0] == 200:
                return routed[1]
        except (OSError, http.client.HTTPException) as error:
            last = (type(error).__name__, str(error))
        time.sleep(0.2)
    raise RuntimeError(f"Traefik discovery contract did not become ready: {last!r}")


def write_route(routes: Path, service_url: str) -> Path:
    route = {
        "http": {
            "middlewares": {"archive-discovery-gzip": {"compress": {}}},
            "routers": {
                "archive-discovery": {
                    "entryPoints": ["archive"],
                    "middlewares": ["archive-discovery-gzip"],
                    "rule": f"Host(`{HOSTNAME}`) && PathPrefix(`/`)",
                    "service": "archive-discovery",
                }
            },
            "services": {
                "archive-discovery": {
                    "loadBalancer": {"servers": [{"url": service_url}]}
                }
            },
        }
    }
    # The reconciler deliberately keeps canonical JSON bytes. JSON is valid
    # YAML, and `.yml` is the discovery contract Traefik honors.
    route_path = routes / "abcdefghijklmnopqrstuvwx.yml"
    route_path.write_text(
        json.dumps(route, separators=(",", ":"), sort_keys=True) + "\n",
        encoding="utf-8",
    )
    route_path.chmod(0o644)
    return route_path


def traefik_version(binary: str) -> str:
    output = run(binary, "version", timeout=20).stdout
    for line in output.splitlines():
        if line.startswith("Version:"):
            return line.partition(":")[2].strip()
    raise RuntimeError(f"Traefik did not report a version: {output!r}")


def reserve_loopback_port(
    excluded: set[int],
) -> tuple[socket.socket, int]:
    for _attempt in range(20):
        listener = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
        try:
            listener.bind(("127.0.0.1", 0))
            port = listener.getsockname()[1]
            if port not in excluded:
                return listener, port
        except Exception:
            listener.close()
            raise
        listener.close()
    raise RuntimeError("could not reserve distinct loopback ports")


def write_native_static_config(
    destination: Path,
    routes: Path,
    traffic_port: int,
    health_port: int,
) -> None:
    source = STATIC_CONFIG.read_text(encoding="utf-8")
    replacements = {
        '    address: ":8082"': (
            f'    address: "127.0.0.1:{traffic_port}"'
        ),
        '    address: ":8083"': (
            f'    address: "127.0.0.1:{health_port}"'
        ),
        "    directory: /etc/traefik/dynamic": (
            f"    directory: {json.dumps(str(routes))}"
        ),
    }
    for expected, replacement in replacements.items():
        if source.count(expected) != 1:
            raise RuntimeError(
                "shipped Traefik static configuration no longer has its "
                f"single expected field: {expected!r}"
            )
        source = source.replace(expected, replacement)
    destination.write_text(source, encoding="utf-8")
    destination.chmod(0o600)


def native_integration(binary_argument: str) -> None:
    binary_path = Path(binary_argument).expanduser()
    try:
        binary_path = binary_path.resolve(strict=True)
    except OSError as error:
        raise RuntimeError("native Traefik binary is unavailable") from error
    if not binary_path.is_file() or not os.access(binary_path, os.X_OK):
        raise RuntimeError("native Traefik path is not an executable file")
    binary = str(binary_path)
    version = traefik_version(binary)
    if version != "3.6.25":
        raise RuntimeError(
            f"native archive router test requires Traefik 3.6.25, got {version!r}"
        )

    NativeBackendHandler.seen_requests = []
    backend = ThreadingHTTPServer(("127.0.0.1", 0), NativeBackendHandler)
    backend.daemon_threads = True
    backend_thread = threading.Thread(target=backend.serve_forever, daemon=True)
    backend_started = False
    reservations: list[socket.socket] = []
    process: subprocess.Popen[str] | None = None

    try:
        backend_thread.start()
        backend_started = True
        traffic_reservation, traffic_port = reserve_loopback_port(
            {backend.server_port}
        )
        reservations.append(traffic_reservation)
        health_reservation, health_port = reserve_loopback_port(
            {backend.server_port, traffic_port}
        )
        reservations.append(health_reservation)
        with tempfile.TemporaryDirectory(prefix="proa-traefik-native-") as temporary:
            root = Path(temporary)
            routes = root / "routes"
            routes.mkdir(mode=0o755)
            write_route(routes, f"http://127.0.0.1:{backend.server_port}")
            static_config = root / "traefik.yml"
            # Traefik's static configuration sources are mutually exclusive:
            # once --configfile is present, CLI static flags are not merged.
            # Derive the test copy with strict, one-occurrence substitutions so
            # every other byte still comes from the shipped production file.
            write_native_static_config(
                static_config,
                routes,
                traffic_port,
                health_port,
            )
            log_path = root / "traefik.log"
            with log_path.open("w+", encoding="utf-8") as log:
                for reservation in reservations:
                    reservation.close()
                reservations.clear()
                process = subprocess.Popen(
                    (
                        binary,
                        f"--configfile={static_config}",
                    ),
                    stdout=log,
                    stderr=subprocess.STDOUT,
                    text=True,
                )
                try:
                    routed_body = await_contract(traffic_port, health_port)
                except Exception:
                    log.flush()
                    log.seek(0)
                    print(log.read()[-8_000:], file=sys.stderr)
                    raise

            if routed_body != NATIVE_BACKEND_BODY or (
                "/healthz",
                HOSTNAME,
            ) not in NativeBackendHandler.seen_requests:
                raise RuntimeError(
                    "native Traefik did not preserve the routed Host/path contract: "
                    f"{routed_body[-2_000:]!r}"
                )
    finally:
        for reservation in reservations:
            reservation.close()
        if process is not None and process.poll() is None:
            process.terminate()
            try:
                process.wait(timeout=5)
            except subprocess.TimeoutExpired:
                process.kill()
                process.wait(timeout=5)
        if backend_started:
            backend.shutdown()
        backend.server_close()
        if backend_started:
            backend_thread.join(timeout=2)


def docker_integration() -> None:
    for image in (TRAEFIK_IMAGE, BACKEND_IMAGE):
        try:
            run("docker", "image", "inspect", image, timeout=10)
        except subprocess.CalledProcessError:
            run("docker", "pull", image, timeout=180)

    image_id = run(
        "docker",
        "image",
        "inspect",
        "--format",
        "{{.Id}}",
        TRAEFIK_IMAGE,
        timeout=10,
    ).stdout.strip()
    version = run("docker", "run", "--rm", image_id, "version", timeout=20).stdout
    if "Version:      3.6.25" not in version:
        raise RuntimeError(
            f"archive router test image is not Traefik 3.6.25: {version!r}"
        )

    backend_image_id = run(
        "docker",
        "image",
        "inspect",
        "--format",
        "{{.Id}}",
        BACKEND_IMAGE,
        timeout=10,
    ).stdout.strip()
    container = f"proa-traefik-discovery-{os.getpid()}-{secrets.token_hex(4)}"
    backend_container = f"{container}-backend"
    network = f"{container}-network"

    try:
        with tempfile.TemporaryDirectory(prefix="proa-traefik-discovery-") as temporary:
            root = Path(temporary)
            routes = root / "routes"
            routes.mkdir(mode=0o755)
            static_config = root / "traefik.yml"
            static_config.write_bytes(STATIC_CONFIG.read_bytes())
            static_config.chmod(0o644)
            write_route(
                routes,
                "http://archive-discovery-backend:8080",
            )

            run("docker", "network", "create", network, timeout=20)
            run(
                "docker",
                "run",
                "--detach",
                "--name",
                backend_container,
                "--network",
                network,
                "--network-alias",
                "archive-discovery-backend",
                "--read-only",
                "--cap-drop",
                "ALL",
                "--security-opt",
                "no-new-privileges:true",
                backend_image_id,
                "-port=8080",
                timeout=30,
            )
            run(
                "docker",
                "run",
                "--detach",
                "--name",
                container,
                "--network",
                network,
                "--publish",
                "127.0.0.1::8082",
                "--publish",
                "127.0.0.1::8083",
                "--read-only",
                "--cap-drop",
                "ALL",
                "--security-opt",
                "no-new-privileges:true",
                "--tmpfs",
                "/tmp:rw,noexec,nosuid,nodev,size=8m",
                "--volume",
                f"{static_config}:/etc/traefik/traefik.yml:ro",
                "--volume",
                f"{routes}:/etc/traefik/dynamic:ro",
                image_id,
                "--configFile=/etc/traefik/traefik.yml",
                timeout=30,
            )
            traffic_port = published_port(container, 8082)
            health_port = published_port(container, 8083)
            try:
                routed_body = await_contract(traffic_port, health_port)
            except Exception:
                logs = run(
                    "docker", "logs", "--tail", "80", container, check=False
                )
                print(logs.stdout[-8_000:], file=sys.stderr)
                print(logs.stderr[-8_000:], file=sys.stderr)
                raise

            if (
                f"Host: {HOSTNAME}".encode() not in routed_body
                or b"GET /healthz HTTP/1.1" not in routed_body
            ):
                raise RuntimeError(
                    "Traefik did not preserve the routed Host/path contract: "
                    f"{routed_body[-2_000:]!r}"
                )
    finally:
        run("docker", "rm", "--force", container, check=False, timeout=20)
        run("docker", "rm", "--force", backend_container, check=False, timeout=20)
        run("docker", "network", "rm", network, check=False, timeout=20)


def main(arguments: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--traefik-binary",
        default=os.environ.get("PROA_TRAEFIK_TEST_BINARY"),
        help="run the integration against this native Traefik 3.6.25 binary",
    )
    parser.add_argument(
        "--docker",
        action="store_true",
        help="force the pinned Docker-image integration path",
    )
    options = parser.parse_args(arguments)
    if options.docker and options.traefik_binary:
        parser.error("--docker and --traefik-binary are mutually exclusive")

    required = (
        os.environ.get("PROA_REQUIRE_TRAEFIK_INTEGRATION") == "1"
        or os.environ.get("PROA_REQUIRE_DOCKER_INTEGRATION") == "1"
    )
    docker_is_available = (
        False if options.traefik_binary else docker_available()
    )
    if options.traefik_binary:
        native_integration(options.traefik_binary)
        mode = "native"
    elif options.docker or docker_is_available:
        if not docker_is_available:
            raise RuntimeError("Docker was requested but is unavailable")
        docker_integration()
        mode = "docker"
    else:
        message = "archive Traefik discovery: no native binary or Docker runtime"
        if required:
            print(message, file=sys.stderr)
            return 1
        print(f"{message}; skipped")
        return 0

    print(f"archive Traefik discovery ({mode}): ok")
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (OSError, RuntimeError, subprocess.SubprocessError) as error:
        print(f"archive Traefik discovery: {error}", file=sys.stderr)
        raise SystemExit(1) from error
