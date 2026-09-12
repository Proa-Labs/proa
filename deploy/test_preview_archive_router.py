# Copyright Proa Labs, Inc.
# SPDX-License-Identifier: Apache-2.0

from __future__ import annotations

import copy
import json
import os
from pathlib import Path
import stat
import subprocess
import tempfile
import unittest


ROOT = Path(__file__).resolve().parents[1]
SERVER = ROOT / "deploy" / "server"
ROUTER = SERVER / "proa-preview-archive-router"
IMAGE = "sha256:" + "b" * 64


def router_command() -> list[str]:
    return ["--configFile=/etc/traefik/traefik.yml"]


def parse_static_router_config(source: str) -> dict[str, object]:
    """Parse the mapping-only YAML subset used by the static router config."""
    root: dict[str, object] = {}
    stack: list[tuple[int, dict[str, object]]] = [(-1, root)]

    for line_number, line in enumerate(source.splitlines(), start=1):
        if not line.strip() or line.lstrip().startswith("#"):
            continue
        if "\t" in line:
            raise AssertionError(f"tab indentation on line {line_number}")
        indentation = len(line) - len(line.lstrip(" "))
        if indentation % 2 != 0:
            raise AssertionError(f"odd indentation on line {line_number}")
        key, separator, raw_value = line.strip().partition(":")
        if separator != ":" or not key or any(character.isspace() for character in key):
            raise AssertionError(f"invalid mapping entry on line {line_number}")

        while stack[-1][0] >= indentation:
            stack.pop()
        parent = stack[-1][1]
        if key in parent:
            raise AssertionError(f"duplicate key {key!r} on line {line_number}")

        value_text = raw_value.strip()
        if not value_text:
            child: dict[str, object] = {}
            parent[key] = child
            stack.append((indentation, child))
        elif value_text == "true":
            parent[key] = True
        elif value_text == "false":
            parent[key] = False
        elif value_text.startswith('"'):
            parent[key] = json.loads(value_text)
        else:
            parent[key] = value_text

    return root


def attested_container(routes: Path, static_config: Path) -> dict:
    return {
        "Name": "/proa-preview-archive-router",
        "Image": IMAGE,
        "Config": {
            "Image": IMAGE,
            "User": "65532:65532",
            "Env": ["PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin"],
            "Labels": {
                "org.opencontainers.image.title": "Traefik",
                "traefik.enable": "false",
                "proa.preview.archive.router": "true",
                "proa.preview.archive.router.application": (
                    "irx97itir84sonabt5d0g5c5"
                ),
            },
            "Cmd": router_command(),
        },
        "State": {"Running": True},
        "HostConfig": {
            "ReadonlyRootfs": True,
            "Privileged": False,
            "PublishAllPorts": False,
            "AutoRemove": True,
            "RestartPolicy": {"Name": "no", "MaximumRetryCount": 0},
            "NetworkMode": "proa-preview-archive",
            "Binds": [
                f"{static_config}:/etc/traefik/traefik.yml:ro",
                f"{routes}:/etc/traefik/dynamic:ro",
            ],
            "Mounts": None,
            "Tmpfs": {
                "/tmp": (
                    "rw,noexec,nosuid,nodev,size=8m,uid=65532,gid=65532,"
                    "mode=0700"
                )
            },
            "PortBindings": {},
            "CapAdd": None,
            "CapDrop": ["ALL"],
            "SecurityOpt": ["no-new-privileges:true"],
            "PidsLimit": 64,
            "Memory": 134217728,
            "MemorySwap": 134217728,
            "NanoCpus": 500000000,
            "Ulimits": [{"Name": "nofile", "Hard": 1024, "Soft": 1024}],
            "LogConfig": {
                "Type": "journald",
                "Config": {"tag": "proa-preview-archive-router"},
            },
        },
        "Mounts": [
            {
                "Type": "bind",
                "Source": str(static_config),
                "Destination": "/etc/traefik/traefik.yml",
                "Mode": "ro",
                "RW": False,
                "Propagation": "rprivate",
            },
            {
                "Type": "bind",
                "Source": str(routes),
                "Destination": "/etc/traefik/dynamic",
                "Mode": "ro",
                "RW": False,
                "Propagation": "rprivate",
            }
        ],
        "NetworkSettings": {
            "Networks": {
                "proa-preview-archive": {
                    "IPAddress": "172.31.252.2",
                    "IPAMConfig": {"IPv4Address": "172.31.252.2"},
                }
            }
        },
    }


class PreviewArchiveRouterTest(unittest.TestCase):
    def setUp(self) -> None:
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.fake_bin = self.root / "bin"
        self.fake_bin.mkdir()
        self.events = self.root / "events"
        self.attest_miss_state = self.root / "attest-miss.state"
        self.attest_drift_state = self.root / "attest-drift.state"
        self.config = self.root / "preview-archive.env"
        self.image = self.root / "router.image"
        self.static_config = self.root / "router-static.yml"
        self.routes = self.root / "routes"
        self.routes.mkdir(mode=0o755)

        self.image.write_text(IMAGE + "\n")
        self.image.chmod(0o600)
        self.static_config.write_text(
            (SERVER / "proa-preview-archive-router-static.yml").read_text()
        )
        self.static_config.chmod(0o644)
        self.config.write_text(
            "\n".join(
                (
                    "PROA_ARCHIVE_APPLICATION_UUID=irx97itir84sonabt5d0g5c5",
                    "PROA_ARCHIVE_DOCKER_NETWORK=proa-preview-archive",
                    f"PROA_ARCHIVE_ROUTER_IMAGE_FILE={self.image}",
                    f"PROA_ARCHIVE_ROUTER_STATIC_CONFIG_FILE={self.static_config}",
                    f"PROA_ARCHIVE_ROUTER_CONFIG_DIR={self.routes}",
                    "PROA_ARCHIVE_ROUTER_IP=172.31.252.2",
                    "PROA_ARCHIVE_ROUTER_ENTRYPOINT=archive",
                    "PROA_ARCHIVE_ROUTER_PORT=8082",
                    "PROA_ARCHIVE_ROUTER_HEALTH_PORT=8083",
                    "",
                )
            )
        )
        self.config.chmod(0o600)

        source = ROUTER.read_text()
        source = source.replace(
            '[[ ${EUID:-$(id -u)} -eq 0 ]] || fail root_required 2',
            ": # root check disabled by router test",
        )
        source = source.replace('"$config_owner" == 0', '"$config_owner" == "$(id -u)"')
        source = source.replace('"$owner" == 0', '"$owner" == "$(id -u)"')
        source = source.replace(
            '"$router_static_config_owner" == 0',
            '"$router_static_config_owner" == "$(id -u)"',
        )
        source = source.replace(
            '"$router_config_owner" == 0',
            '"$router_config_owner" == "$(id -u)"',
        )
        source = source.replace("for attempt in {1..50}; do", "for attempt in {1..2}; do")
        source = source.replace("attempt < 50", "attempt < 2")
        source = source.replace("sleep 0.1", ": # retry delay removed by router test")
        self.router = self.root / "router"
        self.router.write_text(source)
        self.router.chmod(self.router.stat().st_mode | stat.S_IXUSR)
        self._write_fake_docker()

    def _write_fake_docker(self) -> None:
        docker = self.fake_bin / "docker"
        docker.write_text(
            """#!/usr/bin/env bash
printf '%s\\0' "$@" >> "$FAKE_ROUTER_EVENTS"
printf '\\n' >> "$FAKE_ROUTER_EVENTS"
case "${1:-} ${2:-}" in
  'network inspect') printf '%s\n' "$FAKE_ROUTER_NETWORK_IDENTITY"; exit 0 ;;
  'image inspect')
    # Docker 29 omits absent optional Config keys. The production command
    # must use index so the template yields null instead of failing.
    if [[ "$*" == *'json .Config.Volumes'* || "$*" == *'json .Config.Env'* ]]; then
      exit 42
    fi
    printf '%s\\n' "$FAKE_ROUTER_IMAGE_CONTRACT"
    exit 0
    ;;
  'inspect --format')
    if [[ -n "${FAKE_EXISTING_IDENTITY:-}" ]]; then
      printf '%s\\n' "$FAKE_EXISTING_IDENTITY"
      exit 0
    fi
    exit 1
    ;;
  'inspect proa-preview-archive-router')
    if [[ "${FAKE_ROUTER_ATTEST_MISS_ONCE:-}" == true \
      && ! -e "$FAKE_ROUTER_ATTEST_MISS_STATE" ]]; then
      : > "$FAKE_ROUTER_ATTEST_MISS_STATE"
      exit 1
    fi
    if [[ "${FAKE_ROUTER_ATTEST_DRIFT_ONCE:-}" == true \
      && ! -e "$FAKE_ROUTER_ATTEST_DRIFT_STATE" ]]; then
      : > "$FAKE_ROUTER_ATTEST_DRIFT_STATE"
      printf '[{}]\n'
      exit 0
    fi
    if [[ -n "${FAKE_ROUTER_ATTEST_JSON:-}" ]]; then
      printf '%s\\n' "$FAKE_ROUTER_ATTEST_JSON"
      exit 0
    fi
    exit 1
    ;;
  'run --rm') exit 0 ;;
  'rm -fv'|'stop --time') exit 0 ;;
esac
exit 1
"""
        )
        docker.chmod(docker.stat().st_mode | stat.S_IXUSR)

    def run_router(
        self,
        command: str,
        *,
        existing: str = "",
        image: str = IMAGE,
        volumes: str = "null",
        image_environment: str = '["PATH=/usr/local/bin:/usr/bin"]',
        attest: dict | None = None,
        attest_raw: str | None = None,
        attest_miss_once: bool = False,
        attest_drift_once: bool = False,
        network_identity: str = (
            "bridge|true|true|irx97itir84sonabt5d0g5c5|172.31.252.0/24|1"
        ),
    ) -> subprocess.CompletedProcess[str]:
        self.attest_miss_state.unlink(missing_ok=True)
        self.attest_drift_state.unlink(missing_ok=True)
        return subprocess.run(
            [str(self.router), command],
            check=False,
            capture_output=True,
            text=True,
            env={
                **os.environ,
                "PATH": f"{self.fake_bin}:{os.environ['PATH']}",
                "PROA_ARCHIVE_CONFIG_FILE": str(self.config),
                "FAKE_ROUTER_EVENTS": str(self.events),
                "FAKE_ROUTER_IMAGE_CONTRACT": (
                    f"{image}|{volumes}|{image_environment}"
                ),
                "FAKE_EXISTING_IDENTITY": existing,
                "FAKE_ROUTER_ATTEST_JSON": (
                    attest_raw
                    if attest_raw is not None
                    else (json.dumps([attest]) if attest is not None else "")
                ),
                "FAKE_ROUTER_ATTEST_MISS_ONCE": str(attest_miss_once).lower(),
                "FAKE_ROUTER_ATTEST_MISS_STATE": str(self.attest_miss_state),
                "FAKE_ROUTER_ATTEST_DRIFT_ONCE": str(attest_drift_once).lower(),
                "FAKE_ROUTER_ATTEST_DRIFT_STATE": str(self.attest_drift_state),
                "FAKE_ROUTER_NETWORK_IDENTITY": network_identity,
            },
        )

    def event_arguments(self, line: int = -1) -> list[str]:
        records = self.events.read_bytes().splitlines()
        return [item.decode() for item in records[line].split(b"\0") if item]

    def test_run_is_internal_only_hardened_and_file_provider_only(self) -> None:
        result = self.run_router("run")

        self.assertEqual(result.returncode, 0, result.stderr)
        arguments = self.event_arguments()
        joined = "\n".join(arguments)
        for expected in (
            "--read-only",
            "--cap-drop",
            "ALL",
            "no-new-privileges:true",
            "--configFile=/etc/traefik/traefik.yml",
            IMAGE,
            "172.31.252.2",
            f"{self.static_config}:/etc/traefik/traefik.yml:ro",
            f"{self.routes}:/etc/traefik/dynamic:ro",
        ):
            self.assertIn(expected, arguments)
        self.assertFalse(any(argument.startswith("--providers.docker") for argument in arguments))
        self.assertNotIn("/var/run/docker.sock", joined)
        self.assertNotIn("router-token", joined)
        self.assertFalse(any(argument.startswith("--api.dashboard") for argument in arguments))
        self.assertNotIn("--restart", arguments)
        self.assertIn("--ip", arguments)
        self.assertNotIn("--publish", arguments)
        self.assertEqual(arguments[-2:], [IMAGE, *router_command()])

        static_config = parse_static_router_config(self.static_config.read_text())
        self.assertEqual(
            static_config,
            {
                "global": {
                    "checkNewVersion": False,
                    "sendAnonymousUsage": False,
                },
                "log": {"level": "INFO"},
                "entryPoints": {
                    "archive": {"address": ":8082"},
                    "archive-health": {"address": ":8083"},
                },
                "ping": {"entryPoint": "archive-health"},
                "providers": {
                    "file": {
                        "directory": "/etc/traefik/dynamic",
                        "watch": True,
                    }
                },
            },
        )
        self.assertNotIn("api", static_config)
        providers = static_config["providers"]
        self.assertIsInstance(providers, dict)
        self.assertEqual(set(providers), {"file"})
        self.assertNotIn("docker", providers)

    def test_prepare_refuses_foreign_container_identity(self) -> None:
        result = self.run_router(
            "prepare",
            existing=f"true|different-application|{IMAGE}",
        )

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("reason=container_identity_conflict", result.stderr)
        self.assertFalse(any(record.startswith(b"rm\0") for record in self.events.read_bytes().splitlines()))

    def test_prepare_removes_only_exact_owned_stale_container(self) -> None:
        identity = f"true|irx97itir84sonabt5d0g5c5|{IMAGE}"
        result = self.run_router("prepare", existing=identity)

        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("event=stale_router_removed", result.stdout)
        self.assertEqual(
            self.event_arguments(),
            ["rm", "-fv", "proa-preview-archive-router"],
        )

    def test_router_image_must_not_declare_volumes(self) -> None:
        result = self.run_router("run", volumes='{"/data":{}}')

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("reason=router_image_volumes", result.stderr)
        self.assertFalse(
            any(
                record.startswith(b"run\0")
                for record in self.events.read_bytes().splitlines()
            )
        )

    def test_router_image_must_not_declare_traefik_environment(self) -> None:
        result = self.run_router(
            "run",
            image_environment=(
                '["PATH=/usr/bin", "TRAEFIK_PROVIDERS_DOCKER=true"]'
            ),
        )

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("reason=router_image_environment", result.stderr)
        self.assertFalse(
            any(
                record.startswith(b"run\0")
                for record in self.events.read_bytes().splitlines()
            )
        )

    def test_attest_accepts_only_the_exact_running_contract(self) -> None:
        result = self.run_router(
            "attest", attest=attested_container(self.routes, self.static_config)
        )

        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("event=router_attested", result.stdout)

    def test_attest_rejects_non_singleton_inspect_payloads(self) -> None:
        for payload in ("{}", "[]", "[{}, {}]"):
            with self.subTest(payload=payload):
                result = self.run_router("attest", attest_raw=payload)
                self.assertNotEqual(result.returncode, 0)
                self.assertIn("reason=container_contract_drift", result.stderr)

    def test_attest_waits_for_the_new_router_container_to_become_visible(self) -> None:
        result = self.run_router(
            "attest",
            attest=attested_container(self.routes, self.static_config),
            attest_miss_once=True,
        )

        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertTrue(self.attest_miss_state.is_file())
        self.assertIn("event=router_attested", result.stdout)

    def test_attest_waits_for_the_runtime_contract_to_stabilize(self) -> None:
        result = self.run_router(
            "attest",
            attest=attested_container(self.routes, self.static_config),
            attest_drift_once=True,
        )

        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertTrue(self.attest_drift_state.is_file())
        self.assertIn("event=router_attested", result.stdout)

    def test_attest_rejects_adversarial_runtime_drift(self) -> None:
        baseline = attested_container(self.routes, self.static_config)
        drifts: list[tuple[str, dict]] = []

        changed = copy.deepcopy(baseline)
        changed["Image"] = "sha256:" + "c" * 64
        drifts.append(("image", changed))

        changed = copy.deepcopy(baseline)
        changed["Config"]["Labels"][
            "proa.preview.archive.router.application"
        ] = "different"
        drifts.append(("labels", changed))

        changed = copy.deepcopy(baseline)
        changed["Config"]["Labels"][
            "traefik.http.routers.archive-drift.rule"
        ] = "Host(`attacker.example`)"
        drifts.append(("extra_owned_label", changed))

        changed = copy.deepcopy(baseline)
        changed["HostConfig"]["ReadonlyRootfs"] = False
        drifts.append(("writable_root", changed))

        changed = copy.deepcopy(baseline)
        changed["HostConfig"]["Binds"].append(
            "/var/run/docker.sock:/var/run/docker.sock:ro"
        )
        changed["Mounts"].append(
            {
                "Type": "bind",
                "Source": "/var/run/docker.sock",
                "Destination": "/var/run/docker.sock",
                "Mode": "ro",
                "RW": False,
                "Propagation": "rprivate",
            }
        )
        drifts.append(("docker_socket", changed))

        changed = copy.deepcopy(baseline)
        changed["HostConfig"]["Tmpfs"]["/run"] = "rw,size=1m"
        drifts.append(("tmpfs", changed))

        changed = copy.deepcopy(baseline)
        changed["NetworkSettings"]["Networks"]["bridge"] = {}
        drifts.append(("network", changed))

        changed = copy.deepcopy(baseline)
        changed["HostConfig"]["PortBindings"] = {
            "8082/tcp": [{"HostIp": "127.0.0.1", "HostPort": "8082"}]
        }
        drifts.append(("published_port", changed))

        changed = copy.deepcopy(baseline)
        changed["NetworkSettings"]["Networks"]["proa-preview-archive"][
            "IPAddress"
        ] = "172.31.252.3"
        drifts.append(("router_ip", changed))

        changed = copy.deepcopy(baseline)
        changed["NetworkSettings"]["Networks"]["proa-preview-archive"][
            "IPAMConfig"
        ]["IPv4Address"] = "172.31.252.3"
        drifts.append(("requested_router_ip", changed))

        changed = copy.deepcopy(baseline)
        changed["Config"]["User"] = "0:0"
        drifts.append(("user", changed))

        for key, value in (
            ("CapDrop", []),
            ("SecurityOpt", []),
            ("PidsLimit", 65),
            ("Memory", 268435456),
            ("MemorySwap", 268435456),
            ("NanoCpus", 1000000000),
            ("Privileged", True),
            ("PublishAllPorts", True),
        ):
            changed = copy.deepcopy(baseline)
            changed["HostConfig"][key] = value
            drifts.append((key, changed))

        changed = copy.deepcopy(baseline)
        changed["HostConfig"]["Ulimits"][0]["Hard"] = 4096
        drifts.append(("ulimit", changed))

        changed = copy.deepcopy(baseline)
        changed["Config"]["Cmd"].append("--providers.docker=true")
        drifts.append(("docker_provider", changed))

        changed = copy.deepcopy(baseline)
        changed["Config"]["Env"].append("TRAEFIK_PROVIDERS_DOCKER=true")
        drifts.append(("traefik_environment", changed))

        for name, container in drifts:
            with self.subTest(drift=name):
                self.events.unlink(missing_ok=True)
                result = self.run_router("attest", attest=container)
                self.assertNotEqual(result.returncode, 0)
                self.assertIn("reason=container_contract_drift", result.stderr)

    def test_systemd_attests_start_and_cleans_up_failed_start(self) -> None:
        service = (SERVER / "proa-preview-archive-router.service").read_text()

        self.assertIn(
            "ExecStartPost=/usr/local/sbin/proa-preview-archive-router attest",
            service,
        )
        self.assertIn(
            "ExecStopPost=-/usr/local/sbin/proa-preview-archive-router prepare",
            service,
        )
        self.assertIn(
            "ReadOnlyPaths=/etc/proa/preview-archive-router-static.yml",
            service,
        )
        self.assertIn(
            "Environment=DOCKER_CONFIG=/run/proa-preview-archive-router",
            service,
        )
        self.assertIn("RuntimeDirectory=proa-preview-archive-router", service)
        self.assertLess(service.index("ExecStart="), service.index("ExecStartPost="))

    def test_writable_route_directory_and_unpinned_image_fail_closed(self) -> None:
        self.routes.chmod(0o777)
        unsafe = self.run_router("run")
        self.assertNotEqual(unsafe.returncode, 0)
        self.assertIn("reason=router_config_unsafe", unsafe.stderr)

        self.routes.chmod(0o755)
        mismatch = self.run_router("run", image="sha256:" + "c" * 64)
        self.assertNotEqual(mismatch.returncode, 0)
        self.assertIn("reason=router_image_mismatch", mismatch.stderr)

    def test_missing_or_writable_static_config_fails_closed(self) -> None:
        self.static_config.chmod(0o666)
        writable = self.run_router("run")
        self.assertNotEqual(writable.returncode, 0)
        self.assertIn("reason=router_static_config_unsafe", writable.stderr)

        self.static_config.unlink()
        missing = self.run_router("run")
        self.assertNotEqual(missing.returncode, 0)
        self.assertIn("reason=router_static_config_unsafe", missing.stderr)

    def test_same_mode_static_config_drift_fails_closed(self) -> None:
        self.static_config.write_text(
            self.static_config.read_text() + "api:\n  dashboard: true\n"
        )
        self.static_config.chmod(0o644)

        drifted = self.run_router("run")

        self.assertNotEqual(drifted.returncode, 0)
        self.assertIn("reason=router_static_config_drift", drifted.stderr)
        self.assertFalse(self.events.exists())

    def test_static_entrypoint_and_ports_reject_environment_drift(self) -> None:
        baseline = self.config.read_text()
        for setting, replacement, reason in (
            (
                "PROA_ARCHIVE_ROUTER_ENTRYPOINT=archive",
                "PROA_ARCHIVE_ROUTER_ENTRYPOINT=other",
                "entrypoint_invalid",
            ),
            (
                "PROA_ARCHIVE_ROUTER_PORT=8082",
                "PROA_ARCHIVE_ROUTER_PORT=8182",
                "port_invalid",
            ),
            (
                "PROA_ARCHIVE_ROUTER_HEALTH_PORT=8083",
                "PROA_ARCHIVE_ROUTER_HEALTH_PORT=8183",
                "health_port_invalid",
            ),
        ):
            with self.subTest(setting=setting):
                self.events.unlink(missing_ok=True)
                self.config.write_text(baseline.replace(setting, replacement))
                result = self.run_router("run")

                self.assertNotEqual(result.returncode, 0)
                self.assertIn(f"reason={reason}", result.stderr)
                self.assertFalse(self.events.exists())

        self.config.write_text(baseline)

    def test_router_refuses_a_replaced_or_noninternal_archive_network(self) -> None:
        result = self.run_router(
            "run",
            network_identity=(
                "bridge|false|true|irx97itir84sonabt5d0g5c5|172.31.252.0/24|1"
            ),
        )

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("reason=network_identity_mismatch", result.stderr)
        self.assertFalse(
            any(record.startswith(b"run\0") for record in self.events.read_bytes().splitlines())
        )

    def test_router_requires_exactly_one_ipam_config(self) -> None:
        for identity in (
            "bridge|true|true|irx97itir84sonabt5d0g5c5||0",
            "bridge|true|true|irx97itir84sonabt5d0g5c5||2",
        ):
            with self.subTest(identity=identity):
                self.events.unlink(missing_ok=True)
                result = self.run_router("run", network_identity=identity)

                self.assertNotEqual(result.returncode, 0)
                self.assertIn("reason=network_identity_mismatch", result.stderr)
                self.assertFalse(
                    any(
                        record.startswith(b"run\0")
                        for record in self.events.read_bytes().splitlines()
                    )
                )


if __name__ == "__main__":
    unittest.main()
