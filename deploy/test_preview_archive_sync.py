# Copyright Proa Labs, Inc.
# SPDX-License-Identifier: Apache-2.0

from __future__ import annotations

import json
import os
from pathlib import Path
import stat
import subprocess
import tempfile
import unittest


ROOT = Path(__file__).resolve().parents[1]
SYNC = ROOT / "deploy/server/proa-preview-archive-sync"
URL = ROOT / "deploy/server/proa-preview-archive-url"
APPLICATION_UUID = "irx97itir84sonabt5d0g5c5"
IMAGE_ID = "sha256:" + "5" * 64
REVISION = "9f2408f530fd940a86f42115743b7fe031ab276e"
ARCHIVE_SUBNET = "172.31.252.0/24"
ROUTER_IP = "172.31.252.2"
IMAGE_ENV = [
    "PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin",
    "HOST=0.0.0.0",
    "PORT=5555",
    "RUST_LOG=info",
    "STATIC_DIR=/app/static",
    "SHOWCASE_PUBLIC_DIR=/app/showcase",
    "PROA_CONSOLE_LOAD_TESTS=0",
]


FAKE_DOCKER = r"""#!/usr/bin/env python3
import json
import os
from pathlib import Path
import sys

state_path = Path(os.environ["FAKE_DOCKER_STATE"])
state = json.loads(state_path.read_text())
args = sys.argv[1:]
state.setdefault("commands", []).append(args)


def save():
    state_path.write_text(json.dumps(state, sort_keys=True))


def track_peak():
    live = []
    by_pr = {}
    for container in state.get("containers", {}).values():
        labels = container.get("Config", {}).get("Labels", {})
        if (
            labels.get("proa.preview.archive") == "true"
            and container.get("State", {}).get("Running") is True
        ):
            live.append(container)
            pr = labels.get("proa.preview.archive.pr", "")
            by_pr[pr] = by_pr.get(pr, 0) + 1
    state["peak_live_archives"] = max(
        state.get("peak_live_archives", 0), len(live)
    )
    peaks = state.setdefault("peak_live_archives_by_pr", {})
    for pr, count in by_pr.items():
        peaks[pr] = max(peaks.get(pr, 0), count)


def fail(message):
    save()
    print(message, file=sys.stderr)
    raise SystemExit(1)


if args[:2] == ["network", "inspect"]:
    name = args[-1]
    if name in state.get("networks", []):
        if "--format" in args:
            template = args[args.index("--format") + 1]
            if ".Containers" in template:
                for container_name, container in state.get("containers", {}).items():
                    networks = container.get("NetworkSettings", {}).get("Networks", {})
                    if name in networks:
                        print(container_name)
            else:
                print(
                    state.get("network_identities", {}).get(
                        name, "bridge|false||"
                    )
                )
        else:
            print(json.dumps({"Name": name}))
        save()
        raise SystemExit(0)
    fail("network not found")

if args[:2] == ["network", "disconnect"]:
    if len(args) != 5 or args[2] != "--force":
        fail("unsupported network disconnect")
    network, name = args[3], args[4]
    if name not in state.get("containers", {}):
        fail("container not found")
    networks = state["containers"][name].setdefault("NetworkSettings", {}).setdefault(
        "Networks", {}
    )
    if network not in networks:
        fail("container is not connected to network")
    del networks[network]
    save()
    raise SystemExit(0)

if args[:2] == ["image", "inspect"]:
    images = state.get("images", {})
    if len(args) == 3 and args[2] in images:
        print(json.dumps([images[args[2]]]))
        save()
        raise SystemExit(0)
    fail("image not found")

if args and args[0] == "inspect":
    if len(args) != 2 or args[1] not in state.get("containers", {}):
        if state.get("missing_inspect_writes_empty_array"):
            print("[]")
        fail("container not found")
    print(json.dumps([state["containers"][args[1]]]))
    save()
    raise SystemExit(0)

if args and args[0] == "ps":
    include_all = "-a" in args
    label_filters = []
    for index, value in enumerate(args):
        if value == "--filter" and index + 1 < len(args):
            candidate = args[index + 1]
            if candidate.startswith("label="):
                label_filters.append(candidate.removeprefix("label="))
    for name, container in state.get("containers", {}).items():
        if not include_all and not container.get("State", {}).get("Running", False):
            continue
        labels = container.get("Config", {}).get("Labels", {})
        matched = True
        for label_filter in label_filters:
            if "=" in label_filter:
                key, expected = label_filter.split("=", 1)
                matched = matched and labels.get(key) == expected
            else:
                matched = matched and label_filter in labels
        if matched:
            print(name)
    save()
    raise SystemExit(0)

if args and args[0] == "run":
    value_options = {
        "--name", "--restart", "--network", "--network-alias", "--tmpfs",
        "--cap-drop", "--security-opt", "--pids-limit", "--memory",
        "--memory-swap", "--memory-reservation", "--cpus", "--cpu-shares",
        "--log-driver", "--log-opt",
        "--stop-timeout", "--health-cmd", "--health-interval",
        "--health-timeout", "--health-retries", "--health-start-period",
        "--env", "--label", "--user",
    }
    boolean_options = {"--detach", "--read-only"}
    name = None
    network = None
    restart = "no"
    labels = {}
    explicit_environment = []
    options = []
    readonly_rootfs = False
    tmpfs = {}
    user = ""
    cap_drop = []
    security_opt = []
    pids_limit = 0
    memory = 0
    memory_swap = 0
    memory_reservation = 0
    nano_cpus = 0
    cpu_shares = 0
    log_driver = ""
    log_options = {}
    stop_timeout = 0
    healthcheck = {}
    index = 1
    while index < len(args) - 1:
        option = args[index]
        options.append(option)
        if option in boolean_options:
            if option == "--read-only":
                readonly_rootfs = True
            index += 1
            continue
        if option not in value_options or index + 1 >= len(args):
            fail(f"unsupported run option: {option}")
        value = args[index + 1]
        options.append(value)
        if option == "--name":
            name = value
        elif option == "--network":
            network = value
        elif option == "--restart":
            restart = value
        elif option == "--user":
            user = value
        elif option == "--tmpfs":
            target, tmpfs_options = value.split(":", 1)
            tmpfs[target] = tmpfs_options
        elif option == "--cap-drop":
            cap_drop.append(value)
        elif option == "--security-opt":
            security_opt.append(value)
        elif option == "--pids-limit":
            pids_limit = int(value)
        elif option == "--memory":
            memory = {"1g": 1024**3}[value]
        elif option == "--memory-swap":
            memory_swap = {"1g": 1024**3}[value]
        elif option == "--memory-reservation":
            memory_reservation = {"256m": 256 * 1024**2}[value]
        elif option == "--cpus":
            nano_cpus = int(float(value) * 1_000_000_000)
        elif option == "--cpu-shares":
            cpu_shares = int(value)
        elif option == "--log-driver":
            log_driver = value
        elif option == "--log-opt":
            key, log_value = value.split("=", 1)
            log_options[key] = log_value
        elif option == "--stop-timeout":
            stop_timeout = int(value)
        elif option == "--health-cmd":
            healthcheck["Test"] = ["CMD-SHELL", value]
        elif option == "--health-interval":
            healthcheck["Interval"] = {"10s": 10_000_000_000}[value]
        elif option == "--health-timeout":
            healthcheck["Timeout"] = {"5s": 5_000_000_000}[value]
        elif option == "--health-retries":
            healthcheck["Retries"] = int(value)
        elif option == "--health-start-period":
            healthcheck["StartPeriod"] = {"30s": 30_000_000_000}[value]
        elif option == "--label":
            key, label_value = value.split("=", 1)
            labels[key] = label_value
        elif option == "--env":
            explicit_environment.append(value)
        index += 2
    image = args[-1]
    if not name or name in state.get("containers", {}):
        fail("container name conflict")
    images = state.get("images", {})
    if image not in images:
        fail("image not found")
    image_config = images[image].get("Config", {})
    inherited_labels = image_config.get("Labels") or {}
    labels = {**inherited_labels, **labels}
    environment_by_key = {}
    for item in image_config.get("Env") or []:
        key = item.split("=", 1)[0]
        environment_by_key[key] = item
    for item in explicit_environment:
        key = item.split("=", 1)[0]
        environment_by_key[key] = item
    environment = list(environment_by_key.values())
    mounts = [
        {
            "Type": "volume",
            "Name": f"anonymous-{index}",
            "Source": f"/var/lib/docker/volumes/anonymous-{index}/_data",
            "Destination": destination,
            "Driver": "local",
            "Mode": "",
            "RW": True,
            "Propagation": "",
        }
        for index, destination in enumerate(
            sorted((image_config.get("Volumes") or {}).keys()), start=1
        )
    ]
    state.setdefault("containers", {})[name] = {
        "Name": name,
        "Image": image,
        "Config": {
            "Labels": labels,
            "Env": environment,
            "Volumes": image_config.get("Volumes"),
            "User": user,
            "StopTimeout": stop_timeout,
            "Healthcheck": healthcheck,
        },
        "HostConfig": {
            "RestartPolicy": {"Name": restart},
            "Privileged": False,
            "PublishAllPorts": False,
            "ReadonlyRootfs": readonly_rootfs,
            "CapAdd": None,
            "CapDrop": cap_drop,
            "SecurityOpt": security_opt,
            "PidsLimit": pids_limit,
            "Memory": memory,
            "MemorySwap": memory_swap,
            "MemoryReservation": memory_reservation,
            "NanoCpus": nano_cpus,
            "CpuShares": cpu_shares,
            "LogConfig": {"Type": log_driver, "Config": log_options},
            "PortBindings": {},
            "NetworkMode": network,
            "Tmpfs": tmpfs,
        },
        "Mounts": mounts,
        "NetworkSettings": {
            "Networks": {network: {"Aliases": [name]}} if network else {}
        },
        "State": {
            "Running": True,
            "Health": {"Status": state.get("new_archive_health", "healthy")},
        },
        "FakeRunOptions": options,
    }
    injection = state.get("inject_archive_alias_after_run")
    if isinstance(injection, dict):
        target = state["containers"].get(injection.get("container"))
        if target is not None:
            aliases = target.setdefault("NetworkSettings", {}).setdefault(
                "Networks", {}
            ).setdefault(network, {}).setdefault("Aliases", [])
            aliases.append(injection.get("alias"))
    track_peak()
    print("f" * 64)
    save()
    raise SystemExit(0)

if args and args[0] == "update":
    if len(args) != 3 or args[1] != "--restart=no":
        fail("unsupported update")
    name = args[2]
    if name not in state.get("containers", {}):
        fail("container not found")
    state["containers"][name].setdefault("HostConfig", {}).setdefault(
        "RestartPolicy", {}
    )["Name"] = "no"
    save()
    print(name)
    raise SystemExit(0)

if args and args[0] == "rm":
    name = args[-1]
    if name not in state.get("containers", {}):
        fail("container not found")
    if state.get("fail_rm", False):
        fail("injected container removal failure")
    del state["containers"][name]
    track_peak()
    save()
    print(name)
    raise SystemExit(0)

if args and args[0] == "stop":
    name = args[-1]
    if name not in state.get("containers", {}):
        fail("container not found")
    state["containers"][name].setdefault("State", {})["Running"] = False
    state["containers"][name]["State"].setdefault("Health", {})["Status"] = "unhealthy"
    track_peak()
    save()
    print(name)
    raise SystemExit(0)

fail("unsupported docker command: " + " ".join(args))
"""


def source_container(pr: int, deployment: str, revision: str = REVISION) -> dict:
    return {
        "Name": f"{APPLICATION_UUID}-pr-{pr}",
        "Image": IMAGE_ID,
        "Config": {
            "Labels": {
                "coolify.managed": "true",
                "coolify.pullRequestId": str(pr),
                "com.docker.compose.project.config_files": (
                    f"/artifacts/{deployment}/docker-compose.yaml"
                ),
            },
            "Env": [
                f"SOURCE_COMMIT={revision}",
                f"PROA_BUILD_REVISION={revision}",
                "RUST_LOG=info,proa=debug",
                "STATIC_DIR=/app/static",
                "SHOWCASE_PUBLIC_DIR=/app/showcase",
                "PROA_CONSOLE_LOAD_TESTS=0",
                "COOLIFY_URL=http://pr.example.invalid",
                "SECRET_TOKEN=must-not-cross-the-archive-boundary",
            ],
        },
        "State": {"Running": True, "Health": {"Status": "healthy"}},
        "NetworkSettings": {"Networks": {"proa-preview": {}}},
    }


def archive_container(
    pr: int,
    deployment: str,
    created: int,
    *,
    healthy: bool = True,
) -> tuple[str, dict]:
    name = f"proa-preview-archive-{deployment}"
    hostname = f"proa-marketing-{deployment[:12]}.proa.cool"
    return name, {
        "Name": name,
        "Image": IMAGE_ID,
        "Config": {
            "Labels": {
                "proa.preview.archive": "true",
                "proa.preview.archive.application": APPLICATION_UUID,
                "proa.preview.archive.pr": str(pr),
                "proa.preview.archive.deployment": deployment,
                "proa.preview.archive.revision": REVISION,
                "proa.preview.archive.hostname": hostname,
                "proa.preview.archive.created": str(created),
            },
            "Env": [
                *IMAGE_ENV,
                f"SITE_URL=https://{hostname}",
                f"SOURCE_COMMIT={REVISION}",
                f"PROA_BUILD_REVISION={REVISION}",
            ],
            "User": "10001:10001",
            "StopTimeout": 10,
            "Healthcheck": {
                "Test": [
                    "CMD-SHELL",
                    "curl -fsS http://127.0.0.1:5555/healthz || exit 1",
                ],
                "Interval": 10_000_000_000,
                "Timeout": 5_000_000_000,
                "Retries": 10,
                "StartPeriod": 30_000_000_000,
            },
        },
        "HostConfig": {
            "RestartPolicy": {"Name": "unless-stopped"},
            "Privileged": False,
            "PublishAllPorts": False,
            "ReadonlyRootfs": True,
            "CapAdd": None,
            "CapDrop": ["ALL"],
            "SecurityOpt": ["no-new-privileges:true"],
            "PidsLimit": 256,
            "Memory": 1024**3,
            "MemorySwap": 1024**3,
            "MemoryReservation": 256 * 1024**2,
            "NanoCpus": 2_000_000_000,
            "CpuShares": 1024,
            "LogConfig": {
                "Type": "local",
                "Config": {"max-size": "10m", "max-file": "3"},
            },
            "PortBindings": {},
            "NetworkMode": "proa-preview-archive",
            "Tmpfs": {"/tmp": "rw,noexec,nosuid,nodev,size=64m"},
        },
        "Mounts": [],
        "NetworkSettings": {"Networks": {"proa-preview-archive": {}}},
        "State": {
            "Running": healthy,
            "Health": {"Status": "healthy" if healthy else "unhealthy"},
        },
    }


def archive_router_container(*, aliases: list[str] | None = None) -> tuple[str, dict]:
    name = "proa-preview-archive-router"
    return name, {
        "Name": name,
        "Image": IMAGE_ID,
        "Config": {
            "Labels": {
                "proa.preview.archive.router": "true",
                "proa.preview.archive.router.application": APPLICATION_UUID,
            },
            "Env": [],
        },
        "HostConfig": {},
        "NetworkSettings": {
            "Networks": {
                "proa-preview-archive": {
                    "Aliases": aliases or [name],
                    "IPAddress": ROUTER_IP,
                    "IPAMConfig": {"IPv4Address": ROUTER_IP},
                }
            }
        },
        "State": {"Running": True, "Health": {"Status": "healthy"}},
    }


class PreviewArchiveTest(unittest.TestCase):
    def setUp(self) -> None:
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.state_path = self.root / "docker-state.json"
        self.archive_state = self.root / "archive-state"
        self.routes = self.root / "archive-routes"
        self.fake_docker = self.root / "docker"
        self.fake_docker.write_text(FAKE_DOCKER)
        self.fake_docker.chmod(self.fake_docker.stat().st_mode | stat.S_IXUSR)
        self.fail_fsync = self.root / "fail-fsync"
        self.fail_fsync.write_text(
            "#!/bin/sh\n"
            "case \"$2\" in */.write.*) exit 1 ;; esac\n"
            "exec python3 \"$@\"\n"
        )
        self.fail_fsync.chmod(self.fail_fsync.stat().st_mode | stat.S_IXUSR)

    def write_state(
        self,
        containers: dict[str, dict],
        *,
        seed_archive_state: bool = True,
    ) -> None:
        live_by_pr: dict[str, int] = {}
        for container in containers.values():
            labels = container.get("Config", {}).get("Labels", {})
            if (
                labels.get("proa.preview.archive") == "true"
                and container.get("State", {}).get("Running") is True
            ):
                pr = labels.get("proa.preview.archive.pr", "")
                live_by_pr[pr] = live_by_pr.get(pr, 0) + 1
        self.state_path.write_text(
            json.dumps(
                {
                    "commands": [],
                    "containers": containers,
                    "peak_live_archives": sum(live_by_pr.values()),
                    "peak_live_archives_by_pr": live_by_pr,
                    "images": {
                        IMAGE_ID: {
                            "Id": IMAGE_ID,
                            "Config": {
                                "Labels": {},
                                "Volumes": None,
                                "Env": IMAGE_ENV,
                            },
                        }
                    },
                    "networks": ["proa-preview", "proa-preview-archive"],
                    "network_identities": {
                        "proa-preview-archive": (
                            "bridge|true|true|"
                            + APPLICATION_UUID
                            + f"|{ARCHIVE_SUBNET}|1"
                        )
                    },
                }
            )
        )
        if not seed_archive_state:
            return
        deployments = self.archive_state / "deployments"
        claims = self.archive_state / "hostname-claims"
        deployments.mkdir(parents=True, exist_ok=True)
        claims.mkdir(parents=True, exist_ok=True)
        for container in containers.values():
            labels = container.get("Config", {}).get("Labels", {})
            if labels.get("proa.preview.archive") != "true":
                continue
            deployment = labels.get("proa.preview.archive.deployment", "")
            pr = labels.get("proa.preview.archive.pr", "")
            revision = labels.get("proa.preview.archive.revision", "")
            created = labels.get("proa.preview.archive.created", "")
            image = container.get("Image", "")
            if len(deployment) != 24:
                continue
            metadata = deployments / deployment
            if not metadata.exists():
                metadata.write_text(f"{pr} {revision} {created} {image}\n")
            claim = claims / deployment[:12]
            if not claim.exists():
                claim.write_text(deployment + "\n")

    def state(self) -> dict:
        return json.loads(self.state_path.read_text())

    def set_image_config(
        self,
        *,
        labels: dict[str, str] | None = None,
        volumes: dict[str, dict] | None = None,
        environment: list[str] | None = None,
    ) -> None:
        state = self.state()
        state["images"][IMAGE_ID]["Config"] = {
            "Labels": labels or {},
            "Volumes": volumes,
            "Env": IMAGE_ENV if environment is None else environment,
        }
        self.state_path.write_text(json.dumps(state))

    def approve(self, pr: int, deployment: str, approved_at: int = 1) -> None:
        approved = self.archive_state / "approved"
        approved.mkdir(parents=True, exist_ok=True)
        (approved / deployment).write_text(
            f"{pr} {REVISION} {approved_at} {IMAGE_ID}\n"
        )

    def write_metadata(self, pr: int, deployment: str, created: int) -> None:
        metadata = self.archive_state / "deployments"
        metadata.mkdir(parents=True, exist_ok=True)
        (metadata / deployment).write_text(
            f"{pr} {REVISION} {created} {IMAGE_ID}\n"
        )

    def route_payload(self, deployment: str) -> dict:
        return json.loads((self.routes / f"{deployment}.yml").read_text())

    def run_sync(self, now: int, **overrides: str) -> subprocess.CompletedProcess[str]:
        env = os.environ.copy()
        env.update(
            {
                "FAKE_DOCKER_STATE": str(self.state_path),
                "PROA_ARCHIVE_APPLICATION_UUID": APPLICATION_UUID,
                "PROA_ARCHIVE_APPLICATION_NAME": "proa-marketing",
                "PROA_ARCHIVE_PREVIEW_DOMAIN": "proa.cool",
                "PROA_ARCHIVE_NATIVE_DOCKER_NETWORK": "proa-preview",
                "PROA_ARCHIVE_DOCKER_NETWORK": "proa-preview-archive",
                "PROA_ARCHIVE_ROUTER_IP": ROUTER_IP,
                "PROA_ARCHIVE_STATE_DIR": str(self.archive_state),
                "PROA_ARCHIVE_ROUTER_CONFIG_DIR": str(self.routes),
                "PROA_ARCHIVE_DOCKER_BIN": str(self.fake_docker),
                "PROA_ARCHIVE_FLOCK_BIN": "/usr/bin/true",
                "PROA_ARCHIVE_URL_HELPER": str(URL),
                "PROA_ARCHIVE_NOW_EPOCH": str(now),
                "PROA_ARCHIVE_HEALTH_ATTEMPTS": "1",
                "PROA_ARCHIVE_HEALTH_INTERVAL_SECONDS": "1",
            }
        )
        env.update(overrides)
        return subprocess.run(
            [str(SYNC)],
            cwd=ROOT,
            env=env,
            check=False,
            text=True,
            capture_output=True,
        )

    def test_lock_contention_is_a_failed_reconciliation(self) -> None:
        self.write_state({})
        busy_flock = self.root / "busy-flock"
        busy_flock.write_text("#!/bin/sh\nexit 1\n")
        busy_flock.chmod(busy_flock.stat().st_mode | stat.S_IXUSR)

        result = self.run_sync(
            1_100,
            PROA_ARCHIVE_FLOCK_BIN=str(busy_flock),
        )

        self.assertEqual(result.returncode, 75)
        self.assertIn("reason=already_running", result.stdout)
        self.assertEqual(self.state()["commands"], [])
        service = (ROOT / "deploy/server/proa-preview-archive-sync.service").read_text()
        self.assertIn("ExecStartPost=/usr/local/sbin/proa-preview-archive-monitor record-success", service)
        self.assertIn(
            "Environment=DOCKER_CONFIG=/run/proa-preview-archive-sync",
            service,
        )
        self.assertIn(
            "RuntimeDirectory=proa-preview-archive-sync",
            service,
        )
        self.assertIn("RuntimeDirectoryMode=0700", service)
        self.assertNotIn("DOCKER_CONFIG=/root/", service)
        self.assertNotIn("SuccessExitStatus=", service)

    def test_url_helper_is_deterministic_and_writes_github_output(self) -> None:
        deployment = "dy2vxce7udvt9q2otsiwb6o1"
        output_file = self.root / "github-output"
        result = subprocess.run(
            [
                str(URL),
                "--deployment-uuid",
                deployment,
                "--domain",
                "proa.cool",
                "--format",
                "github-output",
            ],
            env={**os.environ, "GITHUB_OUTPUT": str(output_file)},
            check=False,
            text=True,
            capture_output=True,
        )
        self.assertEqual(result.returncode, 0, result.stderr)
        expected = "https://proa-marketing-dy2vxce7udvt.proa.cool"
        self.assertEqual(result.stdout.strip(), expected)
        self.assertEqual(
            output_file.read_text().splitlines(),
            [
                f"url={expected}",
                "hostname=proa-marketing-dy2vxce7udvt.proa.cool",
            ],
        )

    def test_url_helper_rejects_non_coolify_identifier(self) -> None:
        result = subprocess.run(
            [
                str(URL),
                "--deployment-uuid",
                "../../not-a-deployment",
                "--domain",
                "proa.cool",
            ],
            check=False,
            text=True,
            capture_output=True,
        )
        self.assertEqual(result.returncode, 2)
        self.assertIn("deployment UUID", result.stderr)

    def test_creates_exact_read_only_archive_without_unselected_environment(self) -> None:
        deployment = "dy2vxce7udvt9q2otsiwb6o1"
        source_name = f"{APPLICATION_UUID}-pr-14"
        self.write_state({source_name: source_container(14, deployment)})

        first = self.run_sync(2_000_000)
        self.assertEqual(first.returncode, 0, first.stderr)
        self.assertIn("event=created", first.stdout)
        state = self.state()
        archive_name = f"proa-preview-archive-{deployment}"
        archive = state["containers"][archive_name]
        labels = archive["Config"]["Labels"]
        environment = archive["Config"]["Env"]
        options = archive["FakeRunOptions"]

        self.assertEqual(archive["Image"], IMAGE_ID)
        self.assertEqual(labels["proa.preview.archive.deployment"], deployment)
        self.assertEqual(labels["proa.preview.archive.created"], "2000000")
        self.assertFalse(any(key.startswith("traefik.") for key in labels))
        self.assertFalse(any(key.startswith("coolify.") for key in labels))
        self.assertEqual(
            archive["NetworkSettings"]["Networks"],
            {"proa-preview-archive": {"Aliases": [archive_name]}},
        )
        self.assertEqual(archive["HostConfig"]["PortBindings"], {})
        self.assertIn("--read-only", options)
        self.assertIn("--user", options)
        self.assertIn("10001:10001", options)
        self.assertIn("--cap-drop", options)
        self.assertIn("ALL", options)
        self.assertIn("/tmp:rw,noexec,nosuid,nodev,size=64m", options)
        self.assertTrue(archive["HostConfig"]["ReadonlyRootfs"])
        self.assertEqual(
            archive["HostConfig"]["Tmpfs"],
            {"/tmp": "rw,noexec,nosuid,nodev,size=64m"},
        )
        self.assertEqual(archive["Mounts"], [])
        self.assertEqual(
            archive["HostConfig"]["LogConfig"],
            {
                "Type": "local",
                "Config": {"max-size": "10m", "max-file": "3"},
            },
        )
        self.assertIn(f"SOURCE_COMMIT={REVISION}", environment)
        self.assertIn("SITE_URL=https://proa-marketing-dy2vxce7udvt.proa.cool", environment)
        self.assertFalse(any(item.startswith("SECRET_TOKEN=") for item in environment))
        self.assertFalse(any(item.startswith("COOLIFY_") for item in environment))
        self.assertEqual(len(environment), 10)
        self.assertEqual(len({item.split("=", 1)[0] for item in environment}), 10)
        self.assertEqual(
            {item.split("=", 1)[0] for item in environment},
            {
                "PATH",
                "HOST",
                "PORT",
                "RUST_LOG",
                "STATIC_DIR",
                "SHOWCASE_PUBLIC_DIR",
                "PROA_CONSOLE_LOAD_TESTS",
                "SITE_URL",
                "SOURCE_COMMIT",
                "PROA_BUILD_REVISION",
            },
        )

        router = "proa-preview-archive-dy2vxce7udvt"
        route_path = self.routes / f"{deployment}.yml"
        self.assertEqual(stat.S_IMODE(self.routes.stat().st_mode), 0o755)
        self.assertEqual(stat.S_IMODE(route_path.stat().st_mode), 0o644)
        self.assertEqual(route_path.stat().st_uid, os.geteuid())
        self.assertEqual(
            self.route_payload(deployment),
            {
                "http": {
                    "middlewares": {f"{router}-gzip": {"compress": {}}},
                    "routers": {
                        router: {
                            "entryPoints": ["archive"],
                            "middlewares": [f"{router}-gzip"],
                            "rule": (
                                "Host(`proa-marketing-dy2vxce7udvt.proa.cool`) "
                                "&& PathPrefix(`/`)"
                            ),
                            "service": router,
                        }
                    },
                    "services": {
                        router: {
                            "loadBalancer": {
                                "servers": [
                                    {
                                        "url": (
                                            "http://proa-preview-archive-"
                                            f"{deployment}:5555"
                                        )
                                    }
                                ]
                            }
                        }
                    },
                }
            },
        )

        run_count = sum(command[:1] == ["run"] for command in state["commands"])
        second = self.run_sync(2_000_030)
        self.assertEqual(second.returncode, 0, second.stderr)
        self.assertIn("event=present", second.stdout)
        second_state = self.state()
        self.assertEqual(
            sum(command[:1] == ["run"] for command in second_state["commands"]),
            run_count,
        )

    def test_missing_archive_inspect_output_is_not_treated_as_a_container(self) -> None:
        deployment = "missinginspect0000000000"
        source_name = f"{APPLICATION_UUID}-pr-14"
        self.write_state({source_name: source_container(14, deployment)})
        state = self.state()
        state["missing_inspect_writes_empty_array"] = True
        self.state_path.write_text(json.dumps(state))

        result = self.run_sync(2_000_000)

        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("event=created", result.stdout)
        self.assertIn(
            f"proa-preview-archive-{deployment}",
            self.state()["containers"],
        )

    def test_rejects_reserved_labels_in_candidate_image(self) -> None:
        cases = (
            {"traefik.http.routers.rogue.rule": "HostRegexp(`{host:.+}`)"},
            {
                "TRAEFIK.ENABLE": "true",
                "Traefik.HTTP.Routers.rogue.Rule": "Host(`proa.so`)",
            },
            {"coolify.managed": "true"},
            {"Coolify.Managed": "true"},
            {"proa.preview.archive": "true"},
            {"Proa.Preview.Archive.Deployment": "spoofed"},
            {"proa.preview.archive.deployment": "spoofed"},
        )
        for index, labels in enumerate(cases):
            with self.subTest(labels=labels):
                deployment = f"{index + 1}" * 24
                source_name = f"{APPLICATION_UUID}-pr-14"
                self.write_state({source_name: source_container(14, deployment)})
                self.set_image_config(labels=labels)

                result = self.run_sync(1_100)

                self.assertNotEqual(result.returncode, 0)
                self.assertIn("reason=image_labels", result.stdout)
                self.assertFalse(
                    any(command[:1] == ["run"] for command in self.state()["commands"])
                )

    def test_rejects_candidate_image_declared_volumes(self) -> None:
        deployment = "volumeimage0000000000000"
        source_name = f"{APPLICATION_UUID}-pr-14"
        self.write_state({source_name: source_container(14, deployment)})
        self.set_image_config(volumes={"/var/lib/proa": {}})

        result = self.run_sync(1_100)

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("reason=image_volumes", result.stdout)
        self.assertFalse(
            any(command[:1] == ["run"] for command in self.state()["commands"])
        )

    def test_rejects_extra_duplicate_and_malformed_image_environment(self) -> None:
        cases = {
            "extra": [*IMAGE_ENV, "SECRET_TOKEN=image-secret"],
            "duplicate": [*IMAGE_ENV, "RUST_LOG=trace"],
            "malformed": [*IMAGE_ENV, "not-an-environment-entry"],
        }
        for index, (case, environment) in enumerate(cases.items(), start=1):
            with self.subTest(case=case):
                deployment = str(index) * 24
                source_name = f"{APPLICATION_UUID}-pr-14"
                self.write_state({source_name: source_container(14, deployment)})
                self.set_image_config(environment=environment)

                result = self.run_sync(1_100)

                self.assertNotEqual(result.returncode, 0)
                self.assertIn("reason=image_environment", result.stdout)
                self.assertFalse(
                    any(command[:1] == ["run"] for command in self.state()["commands"])
                )

    def test_managed_archive_with_extra_router_is_quarantined(self) -> None:
        deployment = "extrarouter0000000000000"
        name, archive = archive_container(14, deployment, 1_000)
        archive["Config"]["Labels"][
            "traefik.http.routers.inherited.rule"
        ] = r"HostRegexp(`{subdomain:[a-z0-9-]+}\.proa\.cool`)"
        self.write_state({name: archive})

        result = self.run_sync(1_100)

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("reason=runtime_contract action=removed", result.stdout)
        self.assertNotIn(name, self.state()["containers"])

    def test_archive_mount_contract_rejects_volume_and_extra_tmpfs(self) -> None:
        cases = ("volume", "tmpfs")
        for index, mutation in enumerate(cases, start=6):
            with self.subTest(mutation=mutation):
                deployment = str(index) * 24
                name, archive = archive_container(14, deployment, 1_000)
                if mutation == "volume":
                    archive["Mounts"] = [
                        {
                            "Type": "volume",
                            "Destination": "/data",
                            "RW": True,
                        }
                    ]
                    archive["Config"]["Volumes"] = {"/data": {}}
                else:
                    archive["HostConfig"]["Tmpfs"]["/run"] = "rw,size=1m"
                self.write_state({name: archive})

                result = self.run_sync(1_100)

                self.assertNotEqual(result.returncode, 0)
                self.assertIn("reason=runtime_contract action=removed", result.stdout)
                self.assertNotIn(name, self.state()["containers"])

    def test_archive_runtime_hardening_drift_is_quarantined(self) -> None:
        def replace_env(archive: dict, key: str, value: str) -> None:
            environment = archive["Config"]["Env"]
            environment[:] = [
                f"{key}={value}" if item.startswith(f"{key}=") else item
                for item in environment
            ]

        cases = {
            "site_url": lambda archive: replace_env(
                archive, "SITE_URL", "https://different.example"
            ),
            "user": lambda archive: archive["Config"].__setitem__("User", "0:0"),
            "healthcheck": lambda archive: archive["Config"]["Healthcheck"].__setitem__(
                "Retries", 1
            ),
            "privileged": lambda archive: archive["HostConfig"].__setitem__(
                "Privileged", True
            ),
            "capabilities": lambda archive: archive["HostConfig"].__setitem__(
                "CapDrop", []
            ),
            "resource_limit": lambda archive: archive["HostConfig"].__setitem__(
                "Memory", 0
            ),
            "log_policy": lambda archive: archive["HostConfig"].__setitem__(
                "LogConfig", {"Type": "json-file", "Config": {}}
            ),
            "published_port": lambda archive: archive["HostConfig"].__setitem__(
                "PortBindings", {"5555/tcp": [{"HostPort": "5555"}]}
            ),
            "extra_environment": lambda archive: archive["Config"]["Env"].append(
                "SECRET_TOKEN=must-not-be-retained"
            ),
            "duplicate_environment": lambda archive: archive["Config"]["Env"].append(
                "RUST_LOG=trace"
            ),
            "malformed_environment": lambda archive: archive["Config"]["Env"].append(
                "not-an-environment-entry"
            ),
            "mixed_case_router_label": lambda archive: archive["Config"][
                "Labels"
            ].update(
                {
                    "TRAEFIK.ENABLE": "true",
                    "Traefik.HTTP.Routers.rogue.Rule": "Host(`proa.so`)",
                }
            ),
        }
        for index, (case, mutate) in enumerate(cases.items(), start=1):
            with self.subTest(case=case):
                deployment = f"hardening{index:02d}" + "a" * 13
                name, archive = archive_container(14, deployment, 1_000)
                mutate(archive)
                self.write_state({name: archive})

                result = self.run_sync(1_100)

                self.assertNotEqual(result.returncode, 0)
                self.assertIn("reason=runtime_contract", result.stdout)
                self.assertNotIn(name, self.state()["containers"])
                self.assertFalse((self.routes / f"{deployment}.yml").exists())

    def test_recreation_preserves_original_ttl_epoch(self) -> None:
        deployment = "dy2vxce7udvt9q2otsiwb6o1"
        source_name = f"{APPLICATION_UUID}-pr-14"
        archive_name, archive = archive_container(14, deployment, 1_234, healthy=False)
        self.write_state(
            {
                source_name: source_container(14, deployment),
                archive_name: archive,
            }
        )
        result = self.run_sync(2_000)
        self.assertEqual(result.returncode, 0, result.stderr)
        recreated = self.state()["containers"][archive_name]
        self.assertEqual(
            recreated["Config"]["Labels"]["proa.preview.archive.created"],
            "1234",
        )

    def test_recreation_never_removes_backend_when_route_removal_fails(self) -> None:
        deployment = "recreateroutefail0000000"
        source_name = f"{APPLICATION_UUID}-pr-14"
        archive_name, archive = archive_container(14, deployment, 1_000)
        self.write_state(
            {
                source_name: source_container(14, deployment),
                archive_name: archive,
            }
        )
        first = self.run_sync(1_050)
        self.assertEqual(first.returncode, 0, first.stderr)
        route = self.routes / f"{deployment}.yml"
        self.assertTrue(route.exists())

        state = self.state()
        state["containers"][archive_name]["State"] = {
            "Running": False,
            "Health": {"Status": "unhealthy"},
        }
        commands_before = len(state["commands"])
        self.state_path.write_text(json.dumps(state))
        fail_route_remove = self.root / "fail-recreate-route-remove"
        fail_route_remove.write_text(
            "#!/bin/sh\n"
            'if [ "$2" = "$FAIL_ROUTE_DIR" ] && '
            '[ "$3" = "$FAIL_ROUTE_NAME" ]; then exit 1; fi\n'
            'exec python3 "$@"\n'
        )
        fail_route_remove.chmod(
            fail_route_remove.stat().st_mode | stat.S_IXUSR
        )

        failed = self.run_sync(
            1_100,
            PROA_ARCHIVE_PYTHON_BIN=str(fail_route_remove),
            FAIL_ROUTE_DIR=str(self.routes),
            FAIL_ROUTE_NAME=f"{deployment}.yml",
        )

        self.assertNotEqual(failed.returncode, 0)
        self.assertIn("event=recreate_error reason=route_removal", failed.stdout)
        state = self.state()
        self.assertIn(archive_name, state["containers"])
        later_commands = state["commands"][commands_before:]
        self.assertFalse(any(command[:1] == ["rm"] for command in later_commands))
        self.assertFalse(any(command[:1] == ["run"] for command in later_commands))

    def test_recreation_after_container_loss_uses_persisted_creation_epoch(self) -> None:
        deployment = "ffffffffffffffffffffffff"
        source_name = f"{APPLICATION_UUID}-pr-14"
        self.write_state({source_name: source_container(14, deployment)})

        first = self.run_sync(1_000)
        self.assertEqual(first.returncode, 0, first.stderr)
        archive_name = f"proa-preview-archive-{deployment}"
        state = self.state()
        del state["containers"][archive_name]
        self.state_path.write_text(json.dumps(state))

        recreated = self.run_sync(1_200)
        self.assertEqual(recreated.returncode, 0, recreated.stderr)
        labels = self.state()["containers"][archive_name]["Config"]["Labels"]
        self.assertEqual(labels["proa.preview.archive.created"], "1000")

    def test_failed_replacement_after_container_loss_leaves_no_route(self) -> None:
        deployment = "lostreplacement000000000"
        source_name = f"{APPLICATION_UUID}-pr-14"
        self.write_state({source_name: source_container(14, deployment)})
        first = self.run_sync(1_000)
        self.assertEqual(first.returncode, 0, first.stderr)
        archive_name = f"proa-preview-archive-{deployment}"
        route = self.routes / f"{deployment}.yml"
        self.assertTrue(route.exists())

        state = self.state()
        del state["containers"][archive_name]
        state["new_archive_health"] = "unhealthy"
        self.state_path.write_text(json.dumps(state))

        failed = self.run_sync(1_100)

        self.assertNotEqual(failed.returncode, 0)
        self.assertIn("event=health_error", failed.stdout)
        self.assertFalse(route.exists())
        self.assertNotIn(archive_name, self.state()["containers"])

    def test_expired_provisional_metadata_is_retired_before_recreation(self) -> None:
        deployment = "111111111111111111111111"
        source_name = f"{APPLICATION_UUID}-pr-14"
        self.write_state({source_name: source_container(14, deployment)})
        self.write_metadata(14, deployment, 1_000)

        result = self.run_sync(1_900)

        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("event=retired reason=provisional_ttl", result.stdout)
        state = self.state()
        self.assertNotIn(f"proa-preview-archive-{deployment}", state["containers"])
        self.assertFalse(any(command[:1] == ["run"] for command in state["commands"]))
        self.assertEqual(
            (self.archive_state / "retired" / deployment).read_text().split(),
            ["14", REVISION, "1900"],
        )
        self.assertEqual(
            (self.archive_state / "hostname-claims" / deployment[:12])
            .read_text()
            .strip(),
            deployment,
        )

    def test_expired_approved_metadata_is_retired_before_recreation(self) -> None:
        deployment = "222222222222222222222222"
        source_name = f"{APPLICATION_UUID}-pr-14"
        self.write_state({source_name: source_container(14, deployment)})
        self.write_metadata(14, deployment, 1_000)
        self.approve(14, deployment, approved_at=1_100)

        result = self.run_sync(1_000 + 604_800)

        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("event=retired reason=ttl", result.stdout)
        state = self.state()
        self.assertNotIn(f"proa-preview-archive-{deployment}", state["containers"])
        self.assertFalse(any(command[:1] == ["run"] for command in state["commands"]))
        self.assertFalse((self.archive_state / "approved" / deployment).exists())
        self.assertEqual(
            (self.archive_state / "retired" / deployment).read_text().split(),
            ["14", REVISION, str(1_000 + 604_800)],
        )

    def test_future_persisted_epoch_is_rejected_before_recreation(self) -> None:
        deployment = "444444444444444444444444"
        source_name = f"{APPLICATION_UUID}-pr-14"
        self.write_state({source_name: source_container(14, deployment)})
        self.write_metadata(14, deployment, 1_101)

        result = self.run_sync(1_100)

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("reason=metadata_identity", result.stdout)
        state = self.state()
        self.assertNotIn(f"proa-preview-archive-{deployment}", state["containers"])
        self.assertFalse(any(command[:1] == ["run"] for command in state["commands"]))

    def test_source_absence_keeps_permalink_until_ttl(self) -> None:
        deployment = "aaaaaaaaaaaaaaaaaaaaaaaa"
        name, archive = archive_container(14, deployment, 1_000)
        self.write_state({name: archive})
        self.approve(14, deployment)

        before_ttl = self.run_sync(1_000 + 604_799)
        self.assertEqual(before_ttl.returncode, 0, before_ttl.stderr)
        self.assertIn(name, self.state()["containers"])
        route = self.routes / f"{deployment}.yml"
        self.assertTrue(route.exists())

        expired = self.run_sync(1_000 + 604_800)
        self.assertEqual(expired.returncode, 0, expired.stderr)
        self.assertNotIn(name, self.state()["containers"])
        self.assertIn("reason=ttl", expired.stdout)
        self.assertNotIn(
            f"event=route_staged pr=14 deployment={deployment}",
            expired.stdout,
        )
        self.assertFalse(route.exists())
        self.assertIn(["rm", "-fv", name], self.state()["commands"])

    def test_fsync_failure_prevents_retirement_and_container_removal(self) -> None:
        deployment = "fsyncfailure000000000000"
        name, archive = archive_container(14, deployment, 1_000)
        self.write_state({name: archive})

        result = self.run_sync(
            1_060,
            PROA_ARCHIVE_TTL_SECONDS="60",
            PROA_ARCHIVE_PROVISIONAL_TTL_SECONDS="60",
            PROA_ARCHIVE_PYTHON_BIN=str(self.fail_fsync),
        )

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("reason=retired_state", result.stdout)
        self.assertIn(name, self.state()["containers"])
        self.assertFalse((self.archive_state / "retired" / deployment).exists())
        self.assertFalse(
            any(command[:1] == ["rm"] for command in self.state()["commands"])
        )

    def test_tombstone_is_durable_before_failed_container_removal(self) -> None:
        deployment = "removefailure00000000000"
        name, archive = archive_container(14, deployment, 1_000)
        self.write_state({name: archive})
        state = self.state()
        state["fail_rm"] = True
        self.state_path.write_text(json.dumps(state))
        self.routes.mkdir(mode=0o755)
        route = self.routes / f"{deployment}.yml"
        route.write_text("{}\n")
        route.chmod(0o644)

        result = self.run_sync(
            1_060,
            PROA_ARCHIVE_TTL_SECONDS="60",
            PROA_ARCHIVE_PROVISIONAL_TTL_SECONDS="60",
        )

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("event=gc_error reason=provisional_ttl", result.stdout)
        self.assertIn(name, self.state()["containers"])
        self.assertEqual(
            (self.archive_state / "retired" / deployment).read_text().split(),
            ["14", REVISION, "1060"],
        )
        self.assertIn(["rm", "-fv", name], self.state()["commands"])
        self.assertFalse(route.exists())
        self.assertIn("event=route_skipped reason=retired", result.stdout)
        self.assertNotIn(f"event=route_published pr=14 deployment={deployment}", result.stdout)

    def test_expired_hostname_prefix_can_never_point_at_another_deployment(self) -> None:
        first_deployment = "dy2vxce7udvt9q2otsiwb6o1"
        colliding_deployment = "dy2vxce7udvt000000000000"
        source_name = f"{APPLICATION_UUID}-pr-14"
        self.write_state(
            {source_name: source_container(14, first_deployment)}
        )
        created = self.run_sync(1_000)
        self.assertEqual(created.returncode, 0, created.stderr)

        archives_only = {
            name: container
            for name, container in self.state()["containers"].items()
            if name != source_name
        }
        self.write_state(archives_only)
        expired = self.run_sync(
            1_061,
            PROA_ARCHIVE_TTL_SECONDS="60",
            PROA_ARCHIVE_PROVISIONAL_TTL_SECONDS="60",
        )
        self.assertEqual(expired.returncode, 0, expired.stderr)
        self.assertFalse(self.state()["containers"])

        self.write_state(
            {source_name: source_container(14, colliding_deployment)}
        )
        collision = self.run_sync(
            1_062,
            PROA_ARCHIVE_TTL_SECONDS="60",
            PROA_ARCHIVE_PROVISIONAL_TTL_SECONDS="60",
        )
        self.assertNotEqual(collision.returncode, 0)
        self.assertIn("reason=hostname_tombstone", collision.stdout)
        self.assertNotIn(
            f"proa-preview-archive-{colliding_deployment}",
            self.state()["containers"],
        )

    def test_per_pr_and_global_caps_keep_newest_deployments(self) -> None:
        containers: dict[str, dict] = {}
        deployments: list[tuple[int, str, int]] = []
        for pr, prefix, created_values in (
            (14, "a", [100, 300, 500, 700]),
            (15, "b", [200, 400, 600]),
        ):
            for index, created in enumerate(created_values):
                deployment = (prefix + str(index)) * 12
                deployment = deployment[:24]
                name, archive = archive_container(pr, deployment, created)
                containers[name] = archive
                deployments.append((pr, deployment, created))
                self.approve(pr, deployment)
        self.write_state(containers)

        result = self.run_sync(
            1_000,
            PROA_ARCHIVE_TTL_SECONDS="10000",
            PROA_ARCHIVE_MAX_PER_PR="3",
            PROA_ARCHIVE_MAX_GLOBAL="4",
        )
        self.assertEqual(result.returncode, 0, result.stderr)
        retained = self.state()["containers"]
        retained_deployments = {
            container["Config"]["Labels"]["proa.preview.archive.deployment"]
            for container in retained.values()
        }
        newest = {
            deployment
            for _, deployment, _ in sorted(
                deployments, key=lambda item: item[2], reverse=True
            )[:4]
        }
        self.assertEqual(retained_deployments, newest)
        self.assertIn("reason=per_pr_cap", result.stdout)
        self.assertIn("reason=global_cap", result.stdout)

    def test_creation_uses_only_one_global_peak_slot(self) -> None:
        old_deployment = "oldglobal000000000000000"
        new_deployment = "newglobal000000000000000"
        old_name, old_archive = archive_container(15, old_deployment, 1_000)
        source_name = f"{APPLICATION_UUID}-pr-14"
        self.write_state(
            {
                old_name: old_archive,
                source_name: source_container(14, new_deployment),
            }
        )
        self.approve(15, old_deployment)

        result = self.run_sync(
            1_100,
            PROA_ARCHIVE_MAX_PER_PR="1",
            PROA_ARCHIVE_MAX_GLOBAL="1",
        )

        self.assertEqual(result.returncode, 0, result.stderr)
        state = self.state()
        self.assertEqual(state["peak_live_archives"], 2)
        self.assertNotIn(old_name, state["containers"])
        self.assertIn(
            f"proa-preview-archive-{new_deployment}", state["containers"]
        )
        self.assertIn("reason=admission_global_cap", result.stdout)

    def test_creation_uses_only_one_per_pr_peak_slot(self) -> None:
        old_deployment = "oldperpr0000000000000000"
        new_deployment = "newperpr0000000000000000"
        old_name, old_archive = archive_container(14, old_deployment, 1_000)
        source_name = f"{APPLICATION_UUID}-pr-14"
        self.write_state(
            {
                old_name: old_archive,
                source_name: source_container(14, new_deployment),
            }
        )
        self.approve(14, old_deployment)

        result = self.run_sync(
            1_100,
            PROA_ARCHIVE_MAX_PER_PR="1",
            PROA_ARCHIVE_MAX_GLOBAL="2",
        )

        self.assertEqual(result.returncode, 0, result.stderr)
        state = self.state()
        self.assertEqual(state["peak_live_archives_by_pr"]["14"], 2)
        self.assertNotIn(old_name, state["containers"])
        self.assertIn(
            f"proa-preview-archive-{new_deployment}", state["containers"]
        )
        self.assertIn("reason=admission_per_pr_cap", result.stdout)

    def test_admission_rejects_a_different_pr_already_over_cap(self) -> None:
        first_deployment = "overcapfirst000000000000"
        second_deployment = "overcapsecond00000000000"
        candidate_deployment = "capcandidate000000000000"
        first_name, first_archive = archive_container(15, first_deployment, 1_000)
        second_name, second_archive = archive_container(15, second_deployment, 1_001)
        source_name = f"{APPLICATION_UUID}-pr-14"
        self.write_state(
            {
                first_name: first_archive,
                second_name: second_archive,
                source_name: source_container(14, candidate_deployment),
            }
        )

        result = self.run_sync(
            1_100,
            PROA_ARCHIVE_MAX_PER_PR="1",
            PROA_ARCHIVE_MAX_GLOBAL="4",
        )

        self.assertNotEqual(result.returncode, 0)
        self.assertIn(
            "reason=preexisting_capacity_violation pr=14 offending_pr=15",
            result.stdout,
        )
        self.assertFalse(
            any(command[:1] == ["run"] for command in self.state()["commands"])
        )
        self.assertNotIn(
            f"proa-preview-archive-{candidate_deployment}",
            self.state()["containers"],
        )

    def test_failed_candidate_at_capacity_preserves_incumbent_and_route(self) -> None:
        old_deployment = "keepoldroute000000000000"
        new_deployment = "failcandidate00000000000"
        old_name, old_archive = archive_container(14, old_deployment, 1_000)
        self.write_state({old_name: old_archive})
        self.approve(14, old_deployment)
        initial = self.run_sync(
            1_050,
            PROA_ARCHIVE_MAX_PER_PR="1",
            PROA_ARCHIVE_MAX_GLOBAL="1",
        )
        self.assertEqual(initial.returncode, 0, initial.stderr)
        route = self.routes / f"{old_deployment}.yml"
        self.assertTrue(route.exists())

        state = self.state()
        source_name = f"{APPLICATION_UUID}-pr-14"
        state["containers"][source_name] = source_container(14, new_deployment)
        state["new_archive_health"] = "unhealthy"
        self.state_path.write_text(json.dumps(state))

        failed = self.run_sync(
            1_100,
            PROA_ARCHIVE_MAX_PER_PR="1",
            PROA_ARCHIVE_MAX_GLOBAL="1",
        )

        self.assertNotEqual(failed.returncode, 0)
        self.assertIn("event=health_error", failed.stdout)
        state = self.state()
        self.assertIn(old_name, state["containers"])
        self.assertNotIn(
            f"proa-preview-archive-{new_deployment}", state["containers"]
        )
        self.assertTrue(route.exists())
        self.assertEqual(state["peak_live_archives"], 2)
        self.assertEqual(state["peak_live_archives_by_pr"]["14"], 2)

    def test_route_commit_failure_at_capacity_preserves_incumbent(self) -> None:
        old_deployment = "keepoldroute000000000000"
        new_deployment = "routecommitfail000000000"
        old_name, old_archive = archive_container(14, old_deployment, 1_000)
        self.write_state({old_name: old_archive})
        self.approve(14, old_deployment)
        initial = self.run_sync(
            1_050,
            PROA_ARCHIVE_MAX_PER_PR="1",
            PROA_ARCHIVE_MAX_GLOBAL="1",
        )
        self.assertEqual(initial.returncode, 0, initial.stderr)
        old_route = self.routes / f"{old_deployment}.yml"
        previous_route = old_route.read_bytes()

        state = self.state()
        source_name = f"{APPLICATION_UUID}-pr-14"
        state["containers"][source_name] = source_container(14, new_deployment)
        self.state_path.write_text(json.dumps(state))
        fail_route_fsync = self.root / "fail-candidate-route-fsync"
        fail_route_fsync.write_text(
            "#!/bin/sh\n"
            'case "$2" in "$FAIL_ROUTE_DIR"/.write.*) exit 1 ;; esac\n'
            'exec python3 "$@"\n'
        )
        fail_route_fsync.chmod(
            fail_route_fsync.stat().st_mode | stat.S_IXUSR
        )

        failed = self.run_sync(
            1_100,
            PROA_ARCHIVE_MAX_PER_PR="1",
            PROA_ARCHIVE_MAX_GLOBAL="1",
            PROA_ARCHIVE_PYTHON_BIN=str(fail_route_fsync),
            FAIL_ROUTE_DIR=str(self.routes),
        )

        self.assertNotEqual(failed.returncode, 0)
        self.assertIn("event=candidate_route_error", failed.stdout)
        state = self.state()
        self.assertIn(old_name, state["containers"])
        self.assertNotIn(
            f"proa-preview-archive-{new_deployment}", state["containers"]
        )
        self.assertEqual(old_route.read_bytes(), previous_route)
        self.assertFalse((self.routes / f"{new_deployment}.yml").exists())
        self.assertFalse(
            (self.archive_state / "retired" / old_deployment).exists()
        )
        self.assertNotIn(["rm", "-fv", old_name], state["commands"])
        self.assertEqual(state["peak_live_archives"], 2)
        self.assertEqual(state["peak_live_archives_by_pr"]["14"], 2)

    def test_route_staging_precedes_destructive_cap_gc(self) -> None:
        old_deployment = "a1" * 12
        candidate_deployment = "b2" * 12
        old_name, old_archive = archive_container(14, old_deployment, 1_000)
        self.write_state({old_name: old_archive})
        self.approve(14, old_deployment)
        initial = self.run_sync(
            1_050,
            PROA_ARCHIVE_MAX_PER_PR="1",
            PROA_ARCHIVE_MAX_GLOBAL="1",
        )
        self.assertEqual(initial.returncode, 0, initial.stderr)
        old_route = self.routes / f"{old_deployment}.yml"
        previous_route = old_route.read_bytes()

        # Model recovery after a previous route-rollback fsync ambiguity: both
        # healthy backends survived, but only the incumbent still has a route.
        candidate_name, candidate = archive_container(
            14, candidate_deployment, 1_040
        )
        containers = self.state()["containers"]
        containers[candidate_name] = candidate
        containers[f"{APPLICATION_UUID}-pr-14"] = source_container(
            14, candidate_deployment
        )
        self.write_state(containers)
        self.assertFalse(
            (self.routes / f"{candidate_deployment}.yml").exists()
        )

        fail_candidate_route = self.root / "fail-candidate-stage-route"
        fail_candidate_route.write_text(
            "#!/bin/sh\n"
            'if [ "${3:-}" = "$FAIL_ROUTE_PATH" ]; then exit 1; fi\n'
            'exec python3 "$@"\n'
        )
        fail_candidate_route.chmod(
            fail_candidate_route.stat().st_mode | stat.S_IXUSR
        )

        failed = self.run_sync(
            1_100,
            PROA_ARCHIVE_MAX_PER_PR="1",
            PROA_ARCHIVE_MAX_GLOBAL="1",
            PROA_ARCHIVE_PYTHON_BIN=str(fail_candidate_route),
            FAIL_ROUTE_PATH=str(
                self.routes / f"{candidate_deployment}.yml"
            ),
        )

        self.assertNotEqual(failed.returncode, 0)
        self.assertIn(
            "archive route staging failed before capacity garbage collection",
            failed.stdout,
        )
        state = self.state()
        self.assertIn(old_name, state["containers"])
        self.assertIn(candidate_name, state["containers"])
        self.assertEqual(old_route.read_bytes(), previous_route)
        self.assertFalse(
            (self.routes / f"{candidate_deployment}.yml").exists()
        )
        self.assertFalse(
            (self.archive_state / "retired" / old_deployment).exists()
        )
        self.assertNotIn(["rm", "-fv", old_name], state["commands"])

    def test_route_rollback_failure_keeps_candidate_backend(self) -> None:
        old_deployment = "rollbackold0000000000000"
        new_deployment = "rollbacknew0000000000000"
        old_name, old_archive = archive_container(14, old_deployment, 1_000)
        self.write_state({old_name: old_archive})
        self.approve(14, old_deployment)
        initial = self.run_sync(
            1_050,
            PROA_ARCHIVE_MAX_PER_PR="1",
            PROA_ARCHIVE_MAX_GLOBAL="1",
        )
        self.assertEqual(initial.returncode, 0, initial.stderr)

        state = self.state()
        source_name = f"{APPLICATION_UUID}-pr-14"
        state["containers"][source_name] = source_container(14, new_deployment)
        self.state_path.write_text(json.dumps(state))
        fail_rollback = self.root / "fail-route-rollback"
        fail_rollback.write_text(
            "#!/bin/sh\n"
            'if [ "${3:-}" = "$FAIL_WRITE_PATH" ]; then\n'
            '  python3 "$@" || exit $?\n'
            '  exit 1\n'
            'fi\n'
            'if [ "$2" = "$FAIL_ROUTE_DIR" ] && '
            '[ "$3" = "$FAIL_ROUTE_NAME" ]; then exit 1; fi\n'
            'exec python3 "$@"\n'
        )
        fail_rollback.chmod(fail_rollback.stat().st_mode | stat.S_IXUSR)

        failed = self.run_sync(
            1_100,
            PROA_ARCHIVE_MAX_PER_PR="1",
            PROA_ARCHIVE_MAX_GLOBAL="1",
            PROA_ARCHIVE_PYTHON_BIN=str(fail_rollback),
            FAIL_WRITE_PATH=str(self.routes / f"{new_deployment}.yml"),
            FAIL_ROUTE_DIR=str(self.routes),
            FAIL_ROUTE_NAME=f"{new_deployment}.yml",
        )

        self.assertNotEqual(failed.returncode, 0)
        self.assertIn("candidate route rollback failed", failed.stdout)
        state = self.state()
        self.assertIn(old_name, state["containers"])
        self.assertIn(
            f"proa-preview-archive-{new_deployment}", state["containers"]
        )
        self.assertTrue((self.routes / f"{old_deployment}.yml").exists())
        self.assertTrue((self.routes / f"{new_deployment}.yml").exists())

    def test_precommit_rollback_route_failure_keeps_candidate_backend(self) -> None:
        old_deployment = "oldadmitroll000000000000"
        new_deployment = "newadmitroll000000000000"
        old_name, old_archive = archive_container(14, old_deployment, 1_000)
        self.write_state({old_name: old_archive})
        self.approve(14, old_deployment)
        initial = self.run_sync(
            1_050,
            PROA_ARCHIVE_MAX_PER_PR="1",
            PROA_ARCHIVE_MAX_GLOBAL="1",
        )
        self.assertEqual(initial.returncode, 0, initial.stderr)

        state = self.state()
        source_name = f"{APPLICATION_UUID}-pr-14"
        state["containers"][source_name] = source_container(14, new_deployment)
        self.state_path.write_text(json.dumps(state))
        fail_rollback = self.root / "fail-admission-rollback"
        fail_rollback.write_text(
            "#!/bin/sh\n"
            'case "$2" in "$FAIL_RETIRED_DIR"/.write.*) exit 1 ;; esac\n'
            'if [ "$2" = "$FAIL_ROUTE_DIR" ] && '
            '[ "$3" = "$FAIL_ROUTE_NAME" ]; then exit 1; fi\n'
            'exec python3 "$@"\n'
        )
        fail_rollback.chmod(fail_rollback.stat().st_mode | stat.S_IXUSR)

        failed = self.run_sync(
            1_100,
            PROA_ARCHIVE_MAX_PER_PR="1",
            PROA_ARCHIVE_MAX_GLOBAL="1",
            PROA_ARCHIVE_PYTHON_BIN=str(fail_rollback),
            FAIL_RETIRED_DIR=str(self.archive_state / "retired"),
            FAIL_ROUTE_DIR=str(self.routes),
            FAIL_ROUTE_NAME=f"{new_deployment}.yml",
        )

        self.assertNotEqual(failed.returncode, 0)
        self.assertIn("candidate admission rollback failed", failed.stdout)
        state = self.state()
        self.assertIn(old_name, state["containers"])
        self.assertIn(
            f"proa-preview-archive-{new_deployment}", state["containers"]
        )
        self.assertTrue((self.routes / f"{old_deployment}.yml").exists())
        self.assertTrue((self.routes / f"{new_deployment}.yml").exists())
        self.assertFalse(
            (self.archive_state / "retired" / old_deployment).exists()
        )

    def test_precommit_eviction_failure_rolls_back_candidate(self) -> None:
        old_deployment = "precommitold000000000000"
        new_deployment = "precommitnew000000000000"
        old_name, old_archive = archive_container(14, old_deployment, 1_000)
        self.write_state({old_name: old_archive})
        self.approve(14, old_deployment)
        initial = self.run_sync(
            1_050,
            PROA_ARCHIVE_MAX_PER_PR="1",
            PROA_ARCHIVE_MAX_GLOBAL="1",
        )
        self.assertEqual(initial.returncode, 0, initial.stderr)

        state = self.state()
        source_name = f"{APPLICATION_UUID}-pr-14"
        state["containers"][source_name] = source_container(14, new_deployment)
        self.state_path.write_text(json.dumps(state))
        fail_retirement = self.root / "fail-incumbent-retirement"
        fail_retirement.write_text(
            "#!/bin/sh\n"
            'case "$2" in "$FAIL_RETIRED_DIR"/.write.*) exit 1 ;; esac\n'
            'exec python3 "$@"\n'
        )
        fail_retirement.chmod(
            fail_retirement.stat().st_mode | stat.S_IXUSR
        )

        failed = self.run_sync(
            1_100,
            PROA_ARCHIVE_MAX_PER_PR="1",
            PROA_ARCHIVE_MAX_GLOBAL="1",
            PROA_ARCHIVE_PYTHON_BIN=str(fail_retirement),
            FAIL_RETIRED_DIR=str(self.archive_state / "retired"),
        )

        self.assertNotEqual(failed.returncode, 0)
        self.assertIn("event=admission_error reason=commit_capacity", failed.stdout)
        state = self.state()
        self.assertIn(old_name, state["containers"])
        self.assertNotIn(
            f"proa-preview-archive-{new_deployment}", state["containers"]
        )
        self.assertTrue((self.routes / f"{old_deployment}.yml").exists())
        self.assertFalse((self.routes / f"{new_deployment}.yml").exists())
        self.assertFalse(
            (self.archive_state / "retired" / old_deployment).exists()
        )

    def test_postcommit_eviction_failure_keeps_candidate_route(self) -> None:
        old_deployment = "postcommitold00000000000"
        new_deployment = "postcommitnew00000000000"
        old_name, old_archive = archive_container(14, old_deployment, 1_000)
        self.write_state({old_name: old_archive})
        self.approve(14, old_deployment)
        initial = self.run_sync(
            1_050,
            PROA_ARCHIVE_MAX_PER_PR="1",
            PROA_ARCHIVE_MAX_GLOBAL="1",
        )
        self.assertEqual(initial.returncode, 0, initial.stderr)

        state = self.state()
        source_name = f"{APPLICATION_UUID}-pr-14"
        state["containers"][source_name] = source_container(14, new_deployment)
        state["fail_rm"] = True
        self.state_path.write_text(json.dumps(state))

        failed = self.run_sync(
            1_100,
            PROA_ARCHIVE_MAX_PER_PR="1",
            PROA_ARCHIVE_MAX_GLOBAL="1",
        )

        self.assertNotEqual(failed.returncode, 0)
        self.assertIn("event=admission_error reason=commit_capacity", failed.stdout)
        state = self.state()
        self.assertIn(old_name, state["containers"])
        self.assertIn(
            f"proa-preview-archive-{new_deployment}", state["containers"]
        )
        self.assertFalse((self.routes / f"{old_deployment}.yml").exists())
        self.assertTrue((self.routes / f"{new_deployment}.yml").exists())
        self.assertEqual(
            (self.archive_state / "retired" / old_deployment).read_text().split()[:2],
            ["14", REVISION],
        )

    def test_postcommit_route_removal_retry_keeps_candidate(self) -> None:
        old_deployment = "routeretryold00000000000"
        new_deployment = "routeretrynew00000000000"
        old_name, old_archive = archive_container(14, old_deployment, 1_000)
        self.write_state({old_name: old_archive})
        self.approve(14, old_deployment)
        initial = self.run_sync(
            1_050,
            PROA_ARCHIVE_MAX_PER_PR="1",
            PROA_ARCHIVE_MAX_GLOBAL="1",
        )
        self.assertEqual(initial.returncode, 0, initial.stderr)

        state = self.state()
        source_name = f"{APPLICATION_UUID}-pr-14"
        state["containers"][source_name] = source_container(14, new_deployment)
        self.state_path.write_text(json.dumps(state))
        failed_once = self.root / "route-removal-failed-once"
        fail_first_remove = self.root / "fail-first-incumbent-route-remove"
        fail_first_remove.write_text(
            "#!/bin/sh\n"
            'if [ "$2" = "$FAIL_ROUTE_DIR" ] && '
            '[ "$3" = "$FAIL_ROUTE_NAME" ] && '
            '[ ! -e "$FAIL_ONCE_MARKER" ]; then\n'
            '  : > "$FAIL_ONCE_MARKER"\n'
            '  exit 1\n'
            'fi\n'
            'exec python3 "$@"\n'
        )
        fail_first_remove.chmod(
            fail_first_remove.stat().st_mode | stat.S_IXUSR
        )

        failed = self.run_sync(
            1_100,
            PROA_ARCHIVE_MAX_PER_PR="1",
            PROA_ARCHIVE_MAX_GLOBAL="1",
            PROA_ARCHIVE_PYTHON_BIN=str(fail_first_remove),
            FAIL_ROUTE_DIR=str(self.routes),
            FAIL_ROUTE_NAME=f"{old_deployment}.yml",
            FAIL_ONCE_MARKER=str(failed_once),
        )

        self.assertNotEqual(failed.returncode, 0)
        self.assertIn("event=admission_error reason=commit_capacity", failed.stdout)
        state = self.state()
        self.assertNotIn(old_name, state["containers"])
        self.assertIn(
            f"proa-preview-archive-{new_deployment}", state["containers"]
        )
        self.assertFalse((self.routes / f"{old_deployment}.yml").exists())
        self.assertTrue((self.routes / f"{new_deployment}.yml").exists())
        self.assertTrue(
            (self.archive_state / "retired" / old_deployment).exists()
        )

    def test_post_replace_retirement_error_keeps_candidate(self) -> None:
        old_deployment = "postreplaceold0000000000"
        new_deployment = "postreplacenew0000000000"
        old_name, old_archive = archive_container(14, old_deployment, 1_000)
        self.write_state({old_name: old_archive})
        self.approve(14, old_deployment)
        initial = self.run_sync(
            1_050,
            PROA_ARCHIVE_MAX_PER_PR="1",
            PROA_ARCHIVE_MAX_GLOBAL="1",
        )
        self.assertEqual(initial.returncode, 0, initial.stderr)

        state = self.state()
        source_name = f"{APPLICATION_UUID}-pr-14"
        state["containers"][source_name] = source_container(14, new_deployment)
        self.state_path.write_text(json.dumps(state))
        fail_after_replace = self.root / "fail-after-retirement-replace"
        fail_after_replace.write_text(
            "#!/bin/sh\n"
            'if [ "${3:-}" = "$FAIL_RETIRED_PATH" ]; then\n'
            '  python3 "$@" || exit $?\n'
            '  exit 1\n'
            'fi\n'
            'exec python3 "$@"\n'
        )
        fail_after_replace.chmod(
            fail_after_replace.stat().st_mode | stat.S_IXUSR
        )

        failed = self.run_sync(
            1_100,
            PROA_ARCHIVE_MAX_PER_PR="1",
            PROA_ARCHIVE_MAX_GLOBAL="1",
            PROA_ARCHIVE_PYTHON_BIN=str(fail_after_replace),
            FAIL_RETIRED_PATH=str(
                self.archive_state / "retired" / old_deployment
            ),
        )

        self.assertNotEqual(failed.returncode, 0)
        self.assertIn("event=gc_error reason=retired_state", failed.stdout)
        state = self.state()
        self.assertNotIn(old_name, state["containers"])
        self.assertIn(
            f"proa-preview-archive-{new_deployment}", state["containers"]
        )
        self.assertFalse((self.routes / f"{old_deployment}.yml").exists())
        self.assertTrue((self.routes / f"{new_deployment}.yml").exists())
        self.assertTrue(
            (self.archive_state / "retired" / old_deployment).exists()
        )

    def test_failed_health_attempt_persists_first_provisional_epoch(self) -> None:
        deployment = "failedhealth000000000000"
        source_name = f"{APPLICATION_UUID}-pr-14"
        self.write_state({source_name: source_container(14, deployment)})
        state = self.state()
        state["new_archive_health"] = "unhealthy"
        self.state_path.write_text(json.dumps(state))

        first = self.run_sync(1_000)
        self.assertNotEqual(first.returncode, 0)
        metadata = self.archive_state / "deployments" / deployment
        self.assertEqual(
            metadata.read_text().split(), ["14", REVISION, "1000", IMAGE_ID]
        )
        run_count = sum(
            command[:1] == ["run"] for command in self.state()["commands"]
        )

        expired = self.run_sync(1_900)

        self.assertEqual(expired.returncode, 0, expired.stderr)
        self.assertIn("reason=provisional_ttl", expired.stdout)
        self.assertEqual(
            sum(command[:1] == ["run"] for command in self.state()["commands"]),
            run_count,
        )
        self.assertTrue((self.archive_state / "retired" / deployment).exists())

    def test_foreign_labeled_container_blocks_physical_admission(self) -> None:
        foreign_deployment = "foreignowned000000000000"
        candidate = "physicalcap0000000000000"
        foreign_name, foreign = archive_container(99, foreign_deployment, 1_000)
        foreign["Config"]["Labels"][
            "proa.preview.archive.application"
        ] = "foreignapplication"
        foreign["NetworkSettings"]["Networks"] = {"other-network": {}}
        source_name = f"{APPLICATION_UUID}-pr-14"
        self.write_state(
            {
                foreign_name: foreign,
                source_name: source_container(14, candidate),
            }
        )

        result = self.run_sync(1_100)

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("reason=foreign_identity", result.stdout)
        self.assertIn("reason=unvalidated_physical_archive", result.stdout)
        self.assertIn(foreign_name, self.state()["containers"])
        self.assertNotIn(
            f"proa-preview-archive-{candidate}", self.state()["containers"]
        )

    def test_unapproved_archive_expires_and_cannot_recreate_from_live_source(self) -> None:
        deployment = "cccccccccccccccccccccccc"
        source_name = f"{APPLICATION_UUID}-pr-14"
        self.write_state({source_name: source_container(14, deployment)})

        created = self.run_sync(1_000)
        self.assertEqual(created.returncode, 0, created.stderr)
        archive_name = f"proa-preview-archive-{deployment}"

        before_expiry = self.run_sync(1_899)
        self.assertEqual(before_expiry.returncode, 0, before_expiry.stderr)
        self.assertIn(archive_name, self.state()["containers"])

        expired = self.run_sync(1_900)
        self.assertEqual(expired.returncode, 0, expired.stderr)
        self.assertIn("reason=provisional_ttl", expired.stdout)
        self.assertNotIn(archive_name, self.state()["containers"])
        retired = self.archive_state / "retired" / deployment
        self.assertEqual(retired.read_text().split(), ["14", REVISION, "1900"])

        commands_before = len(self.state()["commands"])
        reconciled = self.run_sync(1_901)
        self.assertEqual(reconciled.returncode, 0, reconciled.stderr)
        self.assertIn("reason=retired", reconciled.stdout)
        self.assertNotIn(archive_name, self.state()["containers"])
        new_commands = self.state()["commands"][commands_before:]
        self.assertFalse(any(command[:1] == ["run"] for command in new_commands))

    def test_approved_archive_uses_long_retention_window(self) -> None:
        deployment = "dddddddddddddddddddddddd"
        name, archive = archive_container(14, deployment, 1_000)
        self.write_state({name: archive})
        self.approve(14, deployment, approved_at=1_100)

        after_provisional_ttl = self.run_sync(1_901)
        self.assertEqual(after_provisional_ttl.returncode, 0, after_provisional_ttl.stderr)
        self.assertIn(name, self.state()["containers"])

        after_approved_ttl = self.run_sync(1_000 + 604_800)
        self.assertEqual(after_approved_ttl.returncode, 0, after_approved_ttl.stderr)
        self.assertNotIn(name, self.state()["containers"])
        self.assertIn("reason=ttl", after_approved_ttl.stdout)

    def test_mismatched_approval_quarantines_archive(self) -> None:
        deployment = "eeeeeeeeeeeeeeeeeeeeeeee"
        name, archive = archive_container(14, deployment, 1_000)
        self.write_state({name: archive})
        approved = self.archive_state / "approved"
        approved.mkdir(parents=True, exist_ok=True)
        (approved / deployment).write_text(
            f"14 {'a' * 40} 1100 {IMAGE_ID}\n"
        )

        result = self.run_sync(1_900)
        self.assertNotEqual(result.returncode, 0)
        self.assertNotIn(name, self.state()["containers"])
        self.assertIn("reason=approved_identity", result.stdout)

    def test_retired_marker_eventually_removes_a_container_left_running(self) -> None:
        deployment = "gggggggggggggggggggggggg"
        name, archive = archive_container(14, deployment, 1_000)
        self.write_state({name: archive})
        retired = self.archive_state / "retired"
        retired.mkdir(parents=True, exist_ok=True)
        (retired / deployment).write_text(f"14 {REVISION} 1200\n")
        claim = self.archive_state / "hostname-claims" / deployment[:12]
        claim.unlink()

        result = self.run_sync(1_300)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertNotIn(name, self.state()["containers"])
        self.assertIn("reason=revoked", result.stdout)
        self.assertEqual(claim.read_text().strip(), deployment)

    def test_legacy_retired_marker_reserves_prefix_before_skip(self) -> None:
        deployment = "legacyclaim1aaaaaaaaaaaa"
        colliding = deployment[:12] + "bbbbbbbbbbbb"
        source_name = f"{APPLICATION_UUID}-pr-14"
        self.write_state({source_name: source_container(14, deployment)})
        retired = self.archive_state / "retired"
        retired.mkdir(parents=True, exist_ok=True)
        (retired / deployment).write_text(f"14 {REVISION} 1000\n")

        backfill = self.run_sync(1_100)

        self.assertEqual(backfill.returncode, 0, backfill.stderr)
        claim = self.archive_state / "hostname-claims" / deployment[:12]
        self.assertEqual(claim.read_text().strip(), deployment)
        self.write_state({source_name: source_container(14, colliding)})

        collision = self.run_sync(1_101)

        self.assertNotEqual(collision.returncode, 0)
        self.assertIn("reason=hostname_tombstone", collision.stdout)
        self.assertNotIn(
            f"proa-preview-archive-{colliding}", self.state()["containers"]
        )

    def test_provisional_window_has_a_hard_one_hour_ceiling(self) -> None:
        self.write_state({})
        result = self.run_sync(
            1_000,
            PROA_ARCHIVE_PROVISIONAL_TTL_SECONDS="3601",
        )
        self.assertEqual(result.returncode, 2)
        self.assertIn("provisional retention window", result.stdout)

    def test_invalid_owned_archive_metadata_is_removed_and_reported(self) -> None:
        deployment = "333333333333333333333333"
        name, archive = archive_container(14, deployment, 1_000)
        archive["Config"]["Labels"]["proa.preview.archive.created"] = "corrupt"
        self.write_state({name: archive})

        result = self.run_sync(1_100)

        self.assertNotEqual(result.returncode, 0)
        self.assertIn(
            "event=quarantined reason=invalid_metadata action=removed",
            result.stdout,
        )
        state = self.state()
        self.assertNotIn(name, state["containers"])
        self.assertTrue(any(command[:1] == ["rm"] for command in state["commands"]))

    def test_future_and_oversized_archive_epochs_are_quarantined(self) -> None:
        for index, created in enumerate(("1101", "08", "9" * 30), start=5):
            with self.subTest(created=created):
                deployment = str(index) * 24
                name, archive = archive_container(14, deployment, 1_000)
                archive["Config"]["Labels"][
                    "proa.preview.archive.created"
                ] = created
                self.write_state({name: archive})

                result = self.run_sync(1_100)

                self.assertNotEqual(result.returncode, 0)
                self.assertIn("event=quarantined", result.stdout)
                self.assertNotIn(name, self.state()["containers"])

    def test_corrupt_state_markers_quarantine_running_archives(self) -> None:
        cases = ("deployment_metadata", "retired_identity", "hostname_claim")
        for deployment_character, reason in zip(("8", "9", "a"), cases):
            with self.subTest(reason=reason):
                deployment = deployment_character * 24
                name, archive = archive_container(14, deployment, 1_000)
                self.write_state({name: archive})
                if reason == "deployment_metadata":
                    (self.archive_state / "deployments" / deployment).write_text(
                        f"14 {'a' * 40} 1000 {IMAGE_ID}\n"
                    )
                elif reason == "retired_identity":
                    retired = self.archive_state / "retired"
                    retired.mkdir(parents=True, exist_ok=True)
                    (retired / deployment).write_text(f"15 {REVISION} 1000\n")
                else:
                    (self.archive_state / "hostname-claims" / deployment[:12]).write_text(
                        deployment + "\nextra-line"
                    )

                result = self.run_sync(1_100)

                self.assertNotEqual(result.returncode, 0)
                self.assertIn(f"reason={reason}", result.stdout)
                self.assertNotIn(name, self.state()["containers"])

    def test_invalid_approval_and_retirement_timestamps_quarantine_archives(self) -> None:
        cases = (
            ("b", "approved", "0100", "approved_identity"),
            ("c", "approved", "1101", "approved_identity"),
            ("d", "retired", "9" * 30, "retired_identity"),
        )
        for deployment_character, ledger, timestamp, reason in cases:
            with self.subTest(ledger=ledger, timestamp=timestamp):
                deployment = deployment_character * 24
                name, archive = archive_container(14, deployment, 1_000)
                self.write_state({name: archive})
                marker = self.archive_state / ledger / deployment
                marker.parent.mkdir(parents=True, exist_ok=True)
                if ledger == "approved":
                    marker.write_text(
                        f"14 {REVISION} {timestamp} {IMAGE_ID}\n"
                    )
                else:
                    marker.write_text(f"14 {REVISION} {timestamp}\n")

                result = self.run_sync(1_100)

                self.assertNotEqual(result.returncode, 0)
                self.assertIn(f"reason={reason}", result.stdout)
                self.assertNotIn(name, self.state()["containers"])

    def test_routes_use_supported_yml_suffix_and_remove_legacy_json(self) -> None:
        deployment = "routefixture000000000000"
        name, archive = archive_container(14, deployment, 1_000)
        self.write_state({name: archive})
        self.routes.mkdir()
        legacy_route = self.routes / f"{deployment}.json"
        legacy_route.write_text('{"http":{}}')
        (self.routes / ".orphan").write_text("untrusted")

        first = self.run_sync(1_100)

        self.assertEqual(first.returncode, 0, first.stderr)
        self.assertEqual(
            {path.name for path in self.routes.iterdir()},
            {f"{deployment}.yml"},
        )
        self.assertFalse(legacy_route.exists())
        first_bytes = (self.routes / f"{deployment}.yml").read_bytes()
        self.assertTrue(first_bytes.startswith(b'{"http":'))

        second = self.run_sync(1_101)

        self.assertEqual(second.returncode, 0, second.stderr)
        self.assertEqual(
            (self.routes / f"{deployment}.yml").read_bytes(), first_bytes
        )

    def test_routes_fail_closed_before_a_network_failure(self) -> None:
        self.write_state({})
        self.routes.mkdir()
        stale = self.routes / "aaaaaaaaaaaaaaaaaaaaaaaa.yml"
        stale.write_text('{"http":{"routers":{"unsafe":{}}}}')
        state = self.state()
        state["networks"] = []
        self.state_path.write_text(json.dumps(state))

        result = self.run_sync(1_100)

        self.assertNotEqual(result.returncode, 0)
        self.assertFalse(stale.exists())
        self.assertEqual(list(self.routes.iterdir()), [])

    def test_routes_fail_closed_when_archive_network_identity_drifts(self) -> None:
        self.write_state({})
        self.routes.mkdir()
        stale = self.routes / "aaaaaaaaaaaaaaaaaaaaaaaa.yml"
        stale.write_text('{"http":{"routers":{"unsafe":{}}}}')
        state = self.state()
        state["network_identities"]["proa-preview-archive"] = (
            "bridge|false|true|" + APPLICATION_UUID
        )
        self.state_path.write_text(json.dumps(state))

        result = self.run_sync(1_100)

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("unexpected identity", result.stdout)
        self.assertEqual(list(self.routes.iterdir()), [])

    def test_routes_fail_closed_when_archive_network_ipam_drifts(self) -> None:
        cases = {
            "wrong_subnet": (
                "bridge|true|true|"
                + APPLICATION_UUID
                + "|172.31.253.0/24|1"
            ),
            "extra_subnet": (
                "bridge|true|true|"
                + APPLICATION_UUID
                + f"|{ARCHIVE_SUBNET}|2"
            ),
        }
        for case, identity in cases.items():
            with self.subTest(case=case):
                self.write_state({})
                self.routes.mkdir(exist_ok=True)
                stale = self.routes / "aaaaaaaaaaaaaaaaaaaaaaaa.yml"
                stale.write_text('{"http":{"routers":{"unsafe":{}}}}')
                state = self.state()
                state["network_identities"]["proa-preview-archive"] = identity
                self.state_path.write_text(json.dumps(state))

                result = self.run_sync(1_100)

                self.assertNotEqual(result.returncode, 0)
                self.assertIn("unexpected identity", result.stdout)
                self.assertEqual(list(self.routes.iterdir()), [])

    def test_routes_fail_closed_when_router_network_identity_drifts(self) -> None:
        def realized_ip(router: dict) -> None:
            router["NetworkSettings"]["Networks"]["proa-preview-archive"][
                "IPAddress"
            ] = "172.31.252.3"

        def requested_ip(router: dict) -> None:
            router["NetworkSettings"]["Networks"]["proa-preview-archive"][
                "IPAMConfig"
            ]["IPv4Address"] = "172.31.252.3"

        def missing_ipam(router: dict) -> None:
            router["NetworkSettings"]["Networks"]["proa-preview-archive"].pop(
                "IPAMConfig"
            )

        def extra_network(router: dict) -> None:
            router["NetworkSettings"]["Networks"]["other-network"] = {
                "IPAddress": "192.0.2.2",
                "IPAMConfig": None,
            }

        cases = {
            "realized_ip": realized_ip,
            "requested_ip": requested_ip,
            "missing_ipam": missing_ipam,
            "extra_network": extra_network,
        }
        for case, mutate in cases.items():
            with self.subTest(case=case):
                name, router = archive_router_container()
                mutate(router)
                self.write_state({name: router})
                self.routes.mkdir(exist_ok=True)
                stale = self.routes / "aaaaaaaaaaaaaaaaaaaaaaaa.yml"
                stale.write_text('{"http":{"routers":{"unsafe":{}}}}')

                result = self.run_sync(1_100)

                self.assertNotEqual(result.returncode, 0)
                self.assertIn("reason=router_network_identity", result.stdout)
                self.assertEqual(list(self.routes.iterdir()), [])
                self.assertIn(name, self.state()["containers"])

    def test_rejects_noncanonical_router_ip_configuration(self) -> None:
        self.write_state({})

        result = self.run_sync(
            1_100,
            PROA_ARCHIVE_ROUTER_IP="172.31.252.3",
        )

        self.assertEqual(result.returncode, 2)
        self.assertIn("PROA_ARCHIVE_ROUTER_IP must be 172.31.252.2", result.stdout)
        self.assertEqual(self.state()["commands"], [])

    def test_routes_fail_closed_for_foreign_archive_network_member(self) -> None:
        self.write_state(
            {
                "foreign-service": {
                    "Name": "foreign-service",
                    "Image": IMAGE_ID,
                    "Config": {"Labels": {}, "Env": []},
                    "HostConfig": {},
                    "NetworkSettings": {
                        "Networks": {
                            "proa-preview-archive": {"Aliases": ["foreign-service"]}
                        }
                    },
                    "State": {"Running": True, "Health": {"Status": "healthy"}},
                }
            }
        )
        self.routes.mkdir()
        stale = self.routes / "aaaaaaaaaaaaaaaaaaaaaaaa.yml"
        stale.write_text('{"http":{"routers":{"unsafe":{}}}}')

        result = self.run_sync(1_100)

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("reason=foreign_member", result.stdout)
        self.assertEqual(list(self.routes.iterdir()), [])
        self.assertIn("foreign-service", self.state()["containers"])

    def test_prepublication_network_attestation_catches_alias_hijack(self) -> None:
        deployment = "aliastarget0000000000000"
        source_name = f"{APPLICATION_UUID}-pr-14"
        router_name, router = archive_router_container()
        self.write_state(
            {
                router_name: router,
                source_name: source_container(14, deployment),
            }
        )
        self.routes.mkdir()
        stale = self.routes / "aaaaaaaaaaaaaaaaaaaaaaaa.yml"
        stale.write_text('{"http":{"routers":{"unsafe":{}}}}')
        state = self.state()
        state["inject_archive_alias_after_run"] = {
            "container": router_name,
            "alias": f"proa-preview-archive-{deployment}",
        }
        self.state_path.write_text(json.dumps(state))

        result = self.run_sync(1_100)

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("reason=archive_alias_hijack", result.stdout)
        self.assertIn("changed before candidate route publication", result.stdout)
        self.assertNotIn("event=candidate_route_ready", result.stdout)
        self.assertNotIn(f"event=route_published pr=14 deployment={deployment}", result.stdout)
        self.assertEqual(list(self.routes.iterdir()), [])

    def test_prepublication_attestation_rejects_case_variant_alias(self) -> None:
        deployment = "casealiastarget000000000"
        source_name = f"{APPLICATION_UUID}-pr-14"
        router_name, router = archive_router_container()
        self.write_state(
            {
                router_name: router,
                source_name: source_container(14, deployment),
            }
        )
        state = self.state()
        state["inject_archive_alias_after_run"] = {
            "container": router_name,
            "alias": f"proa-preview-archive-{deployment}".upper(),
        }
        self.state_path.write_text(json.dumps(state))

        result = self.run_sync(1_100)

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("reason=archive_alias_hijack", result.stdout)
        self.assertNotIn("event=candidate_route_ready", result.stdout)
        self.assertFalse((self.routes / f"{deployment}.yml").exists())

    def test_prepublication_alias_extraction_failure_is_fail_closed(self) -> None:
        deployment = "aliasextract000000000000"
        hijacked_deployment = "aliasvictim0000000000000"
        source_name = f"{APPLICATION_UUID}-pr-14"
        candidate_name = f"proa-preview-archive-{deployment}"
        self.write_state({source_name: source_container(14, deployment)})
        state = self.state()
        state["inject_archive_alias_after_run"] = {
            "container": candidate_name,
            "alias": f"proa-preview-archive-{hijacked_deployment}".upper(),
        }
        self.state_path.write_text(json.dumps(state))
        fail_alias_jq = self.root / "fail-alias-extraction-jq"
        fail_alias_jq.write_text(
            "#!/bin/sh\n"
            "for argument do\n"
            "  case \"$argument\" in *Aliases*ascii_downcase*) exit 1 ;; esac\n"
            "done\n"
            'exec jq "$@"\n'
        )
        fail_alias_jq.chmod(fail_alias_jq.stat().st_mode | stat.S_IXUSR)

        result = self.run_sync(
            1_100,
            PROA_ARCHIVE_JQ_BIN=str(fail_alias_jq),
        )

        self.assertNotEqual(result.returncode, 0)
        self.assertIn(
            "reason=alias_extraction container=" + candidate_name,
            result.stdout,
        )
        self.assertIn(
            "changed before candidate route publication",
            result.stdout,
        )
        self.assertNotIn("event=candidate_route_ready", result.stdout)
        self.assertFalse((self.routes / f"{deployment}.yml").exists())

    def test_source_must_come_from_the_native_preview_network(self) -> None:
        deployment = "sourcenetwork00000000000"
        source_name = f"{APPLICATION_UUID}-pr-14"
        source = source_container(14, deployment)
        source["NetworkSettings"]["Networks"] = {"other-network": {}}
        self.write_state({source_name: source})

        result = self.run_sync(1_100)

        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("reason=network_identity", result.stdout)
        self.assertNotIn(
            f"proa-preview-archive-{deployment}", self.state()["containers"]
        )

    def test_route_file_fsync_failure_leaves_no_published_route(self) -> None:
        deployment = "routefsync00000000000000"
        name, archive = archive_container(14, deployment, 1_000)
        self.write_state({name: archive})
        fail_route_fsync = self.root / "fail-route-fsync"
        fail_route_fsync.write_text(
            "#!/bin/sh\n"
            "case \"$2\" in \"$FAIL_ROUTE_DIR\"/.write.*) exit 1 ;; esac\n"
            "exec python3 \"$@\"\n"
        )
        fail_route_fsync.chmod(
            fail_route_fsync.stat().st_mode | stat.S_IXUSR
        )

        result = self.run_sync(
            1_100,
            PROA_ARCHIVE_PYTHON_BIN=str(fail_route_fsync),
            FAIL_ROUTE_DIR=str(self.routes),
        )

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("event=route_write_error", result.stdout)
        self.assertFalse((self.routes / f"{deployment}.yml").exists())
        self.assertTrue(self.state()["containers"][name]["State"]["Running"])

    def test_route_write_failure_preserves_previous_valid_route(self) -> None:
        deployment = "preserveroute00000000000"
        name, archive = archive_container(14, deployment, 1_000)
        self.write_state({name: archive})
        first = self.run_sync(1_050)
        self.assertEqual(first.returncode, 0, first.stderr)
        route = self.routes / f"{deployment}.yml"
        previous = route.read_bytes()
        fail_route_fsync = self.root / "fail-existing-route-fsync"
        fail_route_fsync.write_text(
            "#!/bin/sh\n"
            'case "$2" in "$FAIL_ROUTE_DIR"/.write.*) exit 1 ;; esac\n'
            'exec python3 "$@"\n'
        )
        fail_route_fsync.chmod(
            fail_route_fsync.stat().st_mode | stat.S_IXUSR
        )

        failed = self.run_sync(
            1_100,
            PROA_ARCHIVE_PYTHON_BIN=str(fail_route_fsync),
            FAIL_ROUTE_DIR=str(self.routes),
        )

        self.assertNotEqual(failed.returncode, 0)
        self.assertIn("event=route_write_error", failed.stdout)
        self.assertEqual(route.read_bytes(), previous)

    def test_route_destination_directory_is_never_certified(self) -> None:
        deployment = "badroutepath000000000000"
        name, archive = archive_container(14, deployment, 1_000)
        self.write_state({name: archive})
        self.routes.mkdir()
        corrupt = self.routes / f"{deployment}.yml"
        corrupt.mkdir()
        (corrupt / "left-behind").write_text("not a route")

        result = self.run_sync(1_100)

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("event=route_write_error", result.stdout)
        self.assertNotIn(
            f"event=route_published pr=14 deployment={deployment}", result.stdout
        )
        self.assertFalse(corrupt.exists())

    def test_route_destination_symlink_is_never_certified(self) -> None:
        deployment = "badsymlink00000000000000"
        name, archive = archive_container(14, deployment, 1_000)
        self.write_state({name: archive})
        self.routes.mkdir()
        target = self.root / "outside-route-directory"
        target.mkdir()
        corrupt = self.routes / f"{deployment}.yml"
        corrupt.symlink_to(target, target_is_directory=True)

        result = self.run_sync(1_100)

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("event=route_write_error", result.stdout)
        self.assertNotIn(
            f"event=route_published pr=14 deployment={deployment}", result.stdout
        )
        self.assertFalse(corrupt.exists())
        self.assertTrue(target.exists())

    def test_invalid_archive_is_never_republished(self) -> None:
        deployment = "invalidroute000000000000"
        name, archive = archive_container(14, deployment, 1_000)
        archive["Config"]["Labels"]["proa.preview.archive.created"] = "invalid"
        self.write_state({name: archive})
        self.routes.mkdir()
        route = self.routes / f"{deployment}.yml"
        route.write_text('{"http":{"routers":{"unsafe":{}}}}')

        result = self.run_sync(1_100)

        self.assertNotEqual(result.returncode, 0)
        self.assertFalse(route.exists())
        self.assertNotIn(name, self.state()["containers"])

    def test_route_directory_must_be_owned_and_not_group_writable(self) -> None:
        self.write_state({})
        self.routes.mkdir(mode=0o777)
        self.routes.chmod(0o777)

        result = self.run_sync(1_100)

        self.assertEqual(result.returncode, 2)
        self.assertIn("route directory is unavailable or unsafe", result.stdout)
        self.assertEqual(self.state()["commands"], [])

    def test_unrelated_docker_routers_are_not_scanned_or_mutated(self) -> None:
        name = "unrelated-router"
        self.write_state(
            {
                name: {
                    "Name": name,
                    "Image": IMAGE_ID,
                    "Config": {
                        "Labels": {
                            "traefik.enable": "true",
                            "traefik.http.routers.rogue.rule": (
                                r"HostRegexp(`{subdomain:.+}\.proa\.cool`)"
                            ),
                        },
                        "Env": [],
                    },
                    "HostConfig": {"RestartPolicy": {"Name": "always"}},
                    "NetworkSettings": {"Networks": {"proa-preview": {}}},
                    "State": {"Running": True, "Health": {"Status": "healthy"}},
                }
            }
        )

        result = self.run_sync(1_100)

        self.assertEqual(result.returncode, 0, result.stderr)
        container = self.state()["containers"][name]
        self.assertTrue(container["State"]["Running"])
        self.assertEqual(container["HostConfig"]["RestartPolicy"]["Name"], "always")
        self.assertFalse(
            any(
                command[:1] in (["update"], ["stop"])
                or command[:2] == ["network", "disconnect"]
                for command in self.state()["commands"]
            )
        )


if __name__ == "__main__":
    unittest.main()
