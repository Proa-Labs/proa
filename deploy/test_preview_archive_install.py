# Copyright Proa Labs, Inc.
# SPDX-License-Identifier: Apache-2.0

from __future__ import annotations

import os
from pathlib import Path
import shutil
import stat
import subprocess
import tempfile
import unittest


ROOT = Path(__file__).resolve().parents[1]
SERVER = ROOT / "deploy" / "server"
INSTALLER_PAYLOADS = (
    "proa-preview-archive-sync",
    "proa-preview-archive-control",
    "proa-preview-archive-monitor",
    "proa-preview-archive-router",
    "proa-preview-archive-router-static.yml",
    "proa-preview-archive-url",
    "proa-preview-archive-alert@.service",
    "proa-preview-archive-control.service",
    "proa-preview-archive-monitor.service",
    "proa-preview-archive-monitor.timer",
    "proa-preview-archive-router.service",
    "proa-preview-archive-sync.service",
    "proa-preview-archive-sync.timer",
)


class PreviewArchiveInstallerTest(unittest.TestCase):
    def setUp(self) -> None:
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.bundle = self.root / "bundle"
        self.bundle.mkdir()
        for payload in INSTALLER_PAYLOADS:
            shutil.copy2(SERVER / payload, self.bundle / payload)

        self.systemd = self.root / "systemd"
        self.state = self.root / "state"
        self.sbin = self.root / "sbin"
        self.etc = self.root / "etc-proa"
        self.fake_bin = self.root / "bin"
        for directory in (self.systemd, self.sbin, self.etc, self.fake_bin):
            directory.mkdir()

        # Simulate an upgrade from an active installation. The installer must
        # quiesce these units before its first filesystem mutation.
        for unit in (
            "proa-preview-archive-sync.timer",
            "proa-preview-archive-sync.service",
            "proa-preview-archive-monitor.timer",
            "proa-preview-archive-monitor.service",
        ):
            (self.systemd / unit).write_text("old installation\n")

        source = (SERVER / "install-proa-preview-archive").read_text()
        root_check = (
            '[[ ${EUID:-$(id -u)} -eq 0 ]] || fail "run as root"'
        )
        self.assertEqual(source.count(root_check), 1)
        source = source.replace(root_check, ": # root check disabled by installer test")
        for original, replacement in (
            ("/etc/systemd/system", str(self.systemd)),
            ("/var/lib/proa-preview-archive", str(self.state)),
            ("/usr/local/sbin", str(self.sbin)),
            ("/etc/proa", str(self.etc)),
        ):
            source = source.replace(original, replacement)
        self.installer = self.bundle / "install-proa-preview-archive"
        self.installer.write_text(source)
        self.installer.chmod(self.installer.stat().st_mode | stat.S_IXUSR)

        self.secret = self.root / "control.secret"
        self.secret.write_text("a" * 64)
        self.secret.chmod(0o600)
        self.events = self.root / "events"
        self.router_image = "sha256:" + "b" * 64
        self.network_state = self.root / "archive-network-created"
        self._write_fake_commands()

    def _write_executable(self, name: str, source: str) -> None:
        path = self.fake_bin / name
        path.write_text(source)
        path.chmod(path.stat().st_mode | stat.S_IXUSR)

    def _write_fake_commands(self) -> None:
        recorder = """#!/usr/bin/env bash
printf '%s %s\\n' "$(basename "$0")" "$*" >> "$FAKE_INSTALL_EVENTS"
if [[ "${FAKE_FAIL_SYSTEMCTL:-}" == "$*" ]]; then
  exit 1
fi
exit 0
"""
        self._write_executable("systemctl", recorder)
        self._write_executable(
            "docker",
            """#!/usr/bin/env bash
printf 'docker %s\\n' "$*" >> "$FAKE_INSTALL_EVENTS"
last=${!#}
case "${1:-} ${2:-} ${3:-}" in
  'network inspect proa-preview') exit 0 ;;
  'inspect --format {{.State.Running}}') printf 'true\\n'; exit 0 ;;
  'inspect --format {{.Config.Image}}') printf 'traefik:v3.3\\n'; exit 0 ;;
  'inspect --format {{.Image}}') printf '%s\\n' "$FAKE_ROUTER_IMAGE"; exit 0 ;;
  'image inspect --format')
    # Docker 29 omits absent optional Config keys. Direct field access makes
    # its Go template fail before the installer can distinguish null from a
    # declared volume map; index returns null for the absent key.
    if [[ "$*" == *'json .Config.Volumes'* || "$*" == *'json .Config.Env'* ]]; then
      exit 42
    fi
    printf '%s|%s|%s\\n' \
      "$FAKE_ROUTER_IMAGE" \
      "$FAKE_ROUTER_IMAGE_VOLUMES" \
      "$FAKE_ROUTER_IMAGE_ENVIRONMENT"
    exit 0
    ;;
esac
if [[ "${1:-} ${2:-}" == 'network inspect' && "$last" == proa-preview-archive ]]; then
  if [[ "${FAKE_ARCHIVE_NETWORK_EXISTS:-}" == true || -e "$FAKE_ARCHIVE_NETWORK_STATE" ]]; then
    if [[ "$*" == *'range .Containers'* ]]; then
      printf '%s' "${FAKE_ARCHIVE_NETWORK_MEMBERS:-}"
      exit 0
    fi
    printf '%s\\n' "${FAKE_ARCHIVE_NETWORK_IDENTITY:-bridge|true|true|irx97itir84sonabt5d0g5c5|172.31.252.0/24|1}"
    exit 0
  fi
  exit 1
fi
if [[ "${1:-} ${2:-}" == 'inspect --format' && "$*" == *'proa.preview.archive'* ]]; then
  printf '%s\n' "${FAKE_ARCHIVE_MEMBER_IDENTITY:-}"
  exit 0
fi
if [[ "${1:-} ${2:-}" == 'network create' ]]; then
  : > "$FAKE_ARCHIVE_NETWORK_STATE"
  printf 'network-id\\n'
  exit 0
fi
if [[ "${1:-} ${2:-} ${3:-}" == 'network rm proa-preview-archive' ]]; then
  rm -f "$FAKE_ARCHIVE_NETWORK_STATE"
  exit 0
fi
exit 1
""",
        )
        self._write_executable(
            "flock",
            """#!/usr/bin/env bash
printf 'flock %s\\n' "$*" >> "$FAKE_INSTALL_EVENTS"
if [[ "${FAKE_FLOCK_FAIL:-}" == "$*" ]]; then
  exit 1
fi
exit 0
""",
        )
        self._write_executable(
            "openssl",
            """#!/usr/bin/env bash
printf 'openssl %s\\n' "$*" >> "$FAKE_INSTALL_EVENTS"
printf '%064d\\n' 0 | tr '0' 'd'
""",
        )
        self._write_executable(
            "python3",
            """#!/usr/bin/env bash
printf 'health\\n' >> "$FAKE_INSTALL_EVENTS"
printf 'health-router-ip=%s\\n' "${PROA_ARCHIVE_HEALTH_ROUTER_IP:-missing}" >> "$FAKE_INSTALL_EVENTS"
cat >/dev/null
if [[ "${FAKE_HEALTH_FAIL:-}" == true ]]; then
  exit 1
fi
exit 0
""",
        )
        self._write_executable("sleep", "#!/usr/bin/env bash\nexit 0\n")
        self._write_executable(
            "stat",
            """#!/usr/bin/env bash
case "${1:-} ${2:-}" in
  '-c %u') printf '0\\n' ;;
  '-c %a') printf '600\\n' ;;
  *) exit 1 ;;
esac
""",
        )
        self._write_executable(
            "install",
            """#!/usr/bin/env bash
printf 'install %s\\n' "$*" >> "$FAKE_INSTALL_EVENTS"
args=()
while (($#)); do
  case "$1" in
    -o|-g)
      shift 2
      ;;
    *)
      args+=("$1")
      shift
      ;;
  esac
done
exec /usr/bin/install "${args[@]}"
""",
        )

    def run_installer(
        self,
        *,
        defer: bool,
        fail_systemctl: str | None = None,
        fail_health: bool = False,
        fail_flock: str | None = None,
        archive_network_exists: bool = True,
        archive_network_members: str = "",
        archive_member_identity: str = "",
        archive_network_identity: str = "",
        router_image_volumes: str = "null",
        router_image_environment: str = '["PATH=/usr/local/bin:/usr/bin"]',
    ) -> tuple[subprocess.CompletedProcess[str], list[str]]:
        arguments = [
            str(self.installer),
            "--application-uuid",
            "irx97itir84sonabt5d0g5c5",
            "--control-secret-file",
            str(self.secret),
        ]
        if defer:
            arguments.append("--defer-sync")
        result = subprocess.run(
            arguments,
            check=False,
            capture_output=True,
            text=True,
            env={
                **os.environ,
                "PATH": f"{self.fake_bin}:{os.environ['PATH']}",
                "FAKE_INSTALL_EVENTS": str(self.events),
                "FAKE_FAIL_SYSTEMCTL": fail_systemctl or "",
                "FAKE_HEALTH_FAIL": "true" if fail_health else "false",
                "FAKE_ROUTER_IMAGE": self.router_image,
                "FAKE_ROUTER_IMAGE_VOLUMES": router_image_volumes,
                "FAKE_ROUTER_IMAGE_ENVIRONMENT": router_image_environment,
                "FAKE_FLOCK_FAIL": fail_flock or "",
                "FAKE_ARCHIVE_NETWORK_EXISTS": (
                    "true" if archive_network_exists else "false"
                ),
                "FAKE_ARCHIVE_NETWORK_STATE": str(self.network_state),
                "FAKE_ARCHIVE_NETWORK_MEMBERS": archive_network_members,
                "FAKE_ARCHIVE_MEMBER_IDENTITY": archive_member_identity,
                "FAKE_ARCHIVE_NETWORK_IDENTITY": archive_network_identity,
            },
        )
        events = self.events.read_text().splitlines() if self.events.exists() else []
        return result, events

    def assert_upgrade_was_quiesced_before_install(self, events: list[str]) -> None:
        first_install = next(
            index for index, event in enumerate(events) if event.startswith("install -m")
        )
        for expected in (
            "systemctl disable --now proa-preview-archive-monitor.timer",
            "systemctl disable --now proa-preview-archive-sync.timer",
            "systemctl stop proa-preview-archive-monitor.service",
            "systemctl stop proa-preview-archive-sync.service",
        ):
            self.assertIn(expected, events[:first_install])

    def test_deferred_upgrade_stays_quiesced_after_controller_and_router_health(self) -> None:
        result, events = self.run_installer(defer=True)

        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("sync activation deferred", result.stdout)
        self.assert_upgrade_was_quiesced_before_install(events)
        self.assertLess(
            events.index("systemctl restart proa-preview-archive-control.service"),
            events.index("health"),
        )
        self.assertLess(
            events.index("systemctl restart proa-preview-archive-router.service"),
            events.index("health"),
        )
        self.assertIn("health-router-ip=172.31.252.2", events)
        self.assertNotIn(
            "systemctl start proa-preview-archive-sync.service", events
        )
        self.assertFalse(
            any(event.startswith("systemctl enable --now ") for event in events)
        )
        self.assertTrue((self.sbin / "proa-preview-archive-monitor").is_file())
        self.assertTrue((self.sbin / "proa-preview-archive-router").is_file())
        self.assertTrue(
            (self.systemd / "proa-preview-archive-monitor.timer").is_file()
        )
        sync_unit = (
            self.systemd / "proa-preview-archive-sync.service"
        ).read_text()
        self.assertIn(
            "Environment=DOCKER_CONFIG=/run/proa-preview-archive-sync",
            sync_unit,
        )
        self.assertIn(
            "RuntimeDirectory=proa-preview-archive-sync",
            sync_unit,
        )
        self.assertIn("RuntimeDirectoryMode=0700", sync_unit)
        router_unit = (
            self.systemd / "proa-preview-archive-router.service"
        ).read_text()
        self.assertIn(
            "ExecStartPost=/usr/local/sbin/proa-preview-archive-router attest",
            router_unit,
        )
        self.assertIn(
            "ExecStopPost=-/usr/local/sbin/proa-preview-archive-router prepare",
            router_unit,
        )
        config = (self.etc / "preview-archive.env").read_text()
        self.assertIn(
            "PROA_ARCHIVE_NATIVE_DOCKER_NETWORK=proa-preview",
            config,
        )
        self.assertIn("PROA_ARCHIVE_DOCKER_NETWORK=proa-preview-archive", config)
        self.assertIn("PROA_ARCHIVE_DOCKER_SUBNET=172.31.252.0/24", config)
        self.assertIn(
            f"PROA_ARCHIVE_ROUTER_CONFIG_DIR={self.state}/routes", config
        )
        self.assertIn(
            f"PROA_ARCHIVE_ROUTER_STATIC_CONFIG_FILE={self.etc}/preview-archive-router-static.yml",
            config,
        )
        self.assertIn("PROA_ARCHIVE_ROUTER_ENTRYPOINT=archive", config)
        self.assertIn("PROA_ARCHIVE_ROUTER_IP=172.31.252.2", config)
        self.assertIn("PROA_ARCHIVE_ROUTER_PORT=8082", config)
        self.assertIn("PROA_ARCHIVE_ROUTER_HEALTH_PORT=8083", config)
        sync_timer = (
            self.systemd / "proa-preview-archive-sync.timer"
        ).read_text()
        self.assertIn("OnActiveSec=30s", sync_timer)
        self.assertIn("OnUnitInactiveSec=30s", sync_timer)
        self.assertNotIn("OnBootSec=", sync_timer)
        self.assertNotIn("OnUnitActiveSec=", sync_timer)
        monitor_timer = (
            self.systemd / "proa-preview-archive-monitor.timer"
        ).read_text()
        self.assertIn("OnActiveSec=2min", monitor_timer)
        self.assertIn("OnUnitInactiveSec=1min", monitor_timer)
        self.assertNotIn("OnBootSec=", monitor_timer)
        self.assertNotIn("OnUnitActiveSec=", monitor_timer)
        self.assertEqual(
            (self.etc / "preview-archive-router.image").read_text(),
            self.router_image + "\n",
        )
        self.assertEqual(
            (self.etc / "preview-archive-router-static.yml").read_text(),
            (SERVER / "proa-preview-archive-router-static.yml").read_text(),
        )
        self.assertFalse((self.etc / "preview-archive-router.token").exists())

    def test_default_upgrade_starts_sync_then_enables_both_timers(self) -> None:
        result, events = self.run_installer(defer=False)

        self.assertEqual(result.returncode, 0, result.stderr)
        self.assert_upgrade_was_quiesced_before_install(events)
        health = events.index("health")
        sync_start = events.index("systemctl start proa-preview-archive-sync.service")
        timers = events.index(
            "systemctl enable --now proa-preview-archive-sync.timer "
            "proa-preview-archive-monitor.timer"
        )
        self.assertLess(health, sync_start)
        self.assertLess(sync_start, timers)
        self.assertTrue(
            any(
                index > timers
                and event
                == "systemctl is-active --quiet proa-preview-archive-sync.timer"
                for index, event in enumerate(events)
            )
        )
        self.assertTrue(
            any(
                index > timers
                and event
                == "systemctl is-active --quiet proa-preview-archive-monitor.timer"
                for index, event in enumerate(events)
            )
        )

    def test_partial_timer_activation_disables_timers_without_unsafe_file_rollback(self) -> None:
        activation = (
            "enable --now proa-preview-archive-sync.timer "
            "proa-preview-archive-monitor.timer"
        )
        result, events = self.run_installer(
            defer=False,
            fail_systemctl=activation,
        )

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("timers could not be activated", result.stderr)
        self.assertNotIn("rolling back failed installation", result.stderr)
        self.assertIn(
            "systemctl disable --now proa-preview-archive-sync.timer "
            "proa-preview-archive-monitor.timer",
            events,
        )
        self.assertTrue((self.sbin / "proa-preview-archive-router").is_file())

    def test_health_failure_restores_files_service_states_and_preserves_ledgers(self) -> None:
        old_monitor = self.sbin / "proa-preview-archive-monitor"
        old_monitor.write_text("old monitor\n")
        old_unit = self.systemd / "proa-preview-archive-router.service"
        old_unit.write_text("old router unit\n")
        old_config = self.etc / "preview-archive.env"
        old_config.write_text("OLD_CONFIG=true\n")
        old_token = self.etc / "preview-archive-router.token"
        old_token.write_text("c" * 64 + "\n")
        old_token.chmod(0o600)
        old_image = self.etc / "preview-archive-router.image"
        old_image.write_text("sha256:" + "e" * 64 + "\n")
        old_image.chmod(0o600)
        old_static_config = self.etc / "preview-archive-router-static.yml"
        old_static_config.write_text("old static router config\n")
        old_static_config.chmod(0o640)
        claim = self.state / "hostname-claims" / "existing"
        claim.parent.mkdir(parents=True)
        claim.write_text("permanent-claim\n")
        retired = self.state / "retired" / "existing"
        retired.parent.mkdir(parents=True)
        retired.write_text("permanent-retired\n")
        route = self.state / "routes" / f"{'f' * 24}.yml"
        route.parent.mkdir(parents=True, exist_ok=True)
        route.write_text('{"preserve":true}\n')
        route.chmod(0o644)

        result, events = self.run_installer(defer=True, fail_health=True)

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("isolated router did not become healthy", result.stderr)
        self.assertIn("rolling back failed installation", result.stderr)
        self.assertEqual(old_monitor.read_text(), "old monitor\n")
        self.assertEqual(old_unit.read_text(), "old router unit\n")
        self.assertEqual(old_config.read_text(), "OLD_CONFIG=true\n")
        self.assertEqual(old_token.read_text(), "c" * 64 + "\n")
        self.assertEqual(old_image.read_text(), "sha256:" + "e" * 64 + "\n")
        self.assertEqual(old_static_config.read_text(), "old static router config\n")
        self.assertEqual(stat.S_IMODE(old_static_config.stat().st_mode), 0o640)
        self.assertEqual(claim.read_text(), "permanent-claim\n")
        self.assertEqual(retired.read_text(), "permanent-retired\n")
        self.assertEqual(route.read_text(), '{"preserve":true}\n')
        self.assertIn("systemctl daemon-reload", events)
        self.assertIn("systemctl start proa-preview-archive-router.service", events)

    def test_first_install_health_failure_removes_static_router_config(self) -> None:
        static_config = self.etc / "preview-archive-router-static.yml"
        self.assertFalse(static_config.exists())

        result, _events = self.run_installer(defer=True, fail_health=True)

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("rolling back failed installation", result.stderr)
        self.assertFalse(static_config.exists())

    def test_held_reconciliation_lock_prevents_managed_file_mutation(self) -> None:
        result, events = self.run_installer(defer=True, fail_flock="-n 8")

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("reconciliation is still running", result.stderr)
        self.assertFalse(any(event.startswith("install -m") for event in events))
        self.assertIn("rollback complete", result.stderr)

    def test_incomplete_rollback_is_reported_and_recovery_backup_is_retained(self) -> None:
        old_router_unit = self.systemd / "proa-preview-archive-router.service"
        old_router_unit.write_text("old router unit\n")

        result, _events = self.run_installer(
            defer=True,
            fail_health=True,
            fail_systemctl="start proa-preview-archive-router.service",
        )

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("rollback encountered errors", result.stderr)
        self.assertNotIn("rollback complete", result.stderr)
        marker = "recovery backup retained at "
        self.assertIn(marker, result.stderr)
        retained = Path(result.stderr.split(marker, 1)[1].splitlines()[0])
        self.assertTrue(retained.is_dir())
        self.assertTrue(any(retained.glob("file-*.state")))

    def test_new_archive_network_is_internal_and_removed_on_precommit_failure(self) -> None:
        result, events = self.run_installer(
            defer=True,
            fail_health=True,
            archive_network_exists=False,
        )

        self.assertNotEqual(result.returncode, 0)
        create = next(event for event in events if event.startswith("docker network create"))
        self.assertIn("--driver bridge", create)
        self.assertIn("--internal", create)
        self.assertIn("--subnet 172.31.252.0/24", create)
        self.assertIn("proa.preview.archive.network=true", create)
        self.assertIn(
            "docker network rm proa-preview-archive",
            events,
        )
        self.assertFalse(self.network_state.exists())

    def test_existing_archive_network_with_wrong_subnet_fails_closed(self) -> None:
        result, events = self.run_installer(
            defer=True,
            archive_network_identity=(
                "bridge|true|true|irx97itir84sonabt5d0g5c5|172.31.253.0/24|1"
            ),
        )

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("archive Docker network has an unexpected identity", result.stderr)
        self.assertFalse(any(event.startswith("install -m") for event in events))

    def test_existing_archive_network_requires_exactly_one_ipam_config(self) -> None:
        for identity in (
            "bridge|true|true|irx97itir84sonabt5d0g5c5||0",
            "bridge|true|true|irx97itir84sonabt5d0g5c5||2",
        ):
            with self.subTest(identity=identity):
                self.events.unlink(missing_ok=True)
                result, events = self.run_installer(
                    defer=True,
                    archive_network_identity=identity,
                )

                self.assertNotEqual(result.returncode, 0)
                self.assertIn(
                    "archive Docker network has an unexpected identity",
                    result.stderr,
                )
                self.assertFalse(
                    any(event.startswith("install -m") for event in events)
                )

    def test_archive_network_rejects_native_or_unknown_members(self) -> None:
        result, events = self.run_installer(
            defer=True,
            archive_network_members="irx97itir84sonabt5d0g5c5-pr-14\n",
        )

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("unexpected container", result.stderr)
        self.assertFalse(any(event.startswith("install -m") for event in events))

    def test_archive_network_rejects_a_foreign_container_with_an_owned_name(self) -> None:
        deployment = "a" * 24
        result, events = self.run_installer(
            defer=True,
            archive_network_members=f"proa-preview-archive-{deployment}\n",
            archive_member_identity=f"true|different-application|{deployment}",
        )

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("foreign archive container", result.stderr)
        self.assertFalse(any(event.startswith("install -m") for event in events))

    def test_missing_or_stopped_source_traefik_fails_before_mutation(self) -> None:
        docker = self.fake_bin / "docker"
        source = docker.read_text().replace("printf 'true\\n'", "printf 'false\\n'")
        docker.write_text(source)

        result, events = self.run_installer(defer=True)

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("is not running", result.stderr)
        self.assertFalse(any(event.startswith("install ") for event in events))

    def test_source_traefik_image_with_declared_volumes_fails_before_mutation(
        self,
    ) -> None:
        result, events = self.run_installer(
            defer=True,
            router_image_volumes='{"/data":{}}',
        )

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("source router image declares volumes", result.stderr)
        self.assertFalse(any(event.startswith("install ") for event in events))

    def test_source_traefik_image_with_traefik_environment_fails_before_mutation(
        self,
    ) -> None:
        result, events = self.run_installer(
            defer=True,
            router_image_environment=(
                '["PATH=/usr/local/bin:/usr/bin",'
                '"TRAEFIK_PROVIDERS_DOCKER=true"]'
            ),
        )

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("declares TRAEFIK_* environment", result.stderr)
        self.assertFalse(any(event.startswith("install ") for event in events))


if __name__ == "__main__":
    unittest.main()
