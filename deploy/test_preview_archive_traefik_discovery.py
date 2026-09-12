# Copyright Proa Labs, Inc.
# SPDX-License-Identifier: Apache-2.0

from __future__ import annotations

from contextlib import redirect_stderr, redirect_stdout
import io
import json
from pathlib import Path
import socket
import tempfile
import unittest
from unittest import mock

import check_preview_archive_traefik_discovery as discovery


class PreviewArchiveTraefikDiscoveryTest(unittest.TestCase):
    def test_reserved_loopback_port_stays_exclusive_until_released(self) -> None:
        reservation, port = discovery.reserve_loopback_port(set())
        contender = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
        try:
            with self.assertRaises(OSError):
                contender.bind(("127.0.0.1", port))
        finally:
            contender.close()
            reservation.close()

        rebound = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
        try:
            rebound.bind(("127.0.0.1", port))
        finally:
            rebound.close()

    def test_route_uses_supported_yml_suffix_with_canonical_json(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            routes = Path(temporary)
            route = discovery.write_route(routes, "http://127.0.0.1:12345")

            self.assertEqual(route.suffix, ".yml")
            self.assertEqual(list(routes.glob("*.json")), [])
            payload = json.loads(route.read_text(encoding="utf-8"))
            expected = (
                json.dumps(payload, separators=(",", ":"), sort_keys=True) + "\n"
            )
            self.assertEqual(route.read_text(encoding="utf-8"), expected)

    def test_native_static_config_changes_only_environment_specific_fields(
        self,
    ) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            routes = root / "routes"
            routes.mkdir()
            generated = root / "traefik.yml"

            discovery.write_native_static_config(generated, routes, 32101, 32102)

            content = generated.read_text(encoding="utf-8")
            self.assertIn('address: "127.0.0.1:32101"', content)
            self.assertIn('address: "127.0.0.1:32102"', content)
            self.assertIn(f"directory: {json.dumps(str(routes))}", content)
            self.assertIn("watch: true", content)
            self.assertIn("entryPoint: archive-health", content)
            self.assertNotIn("/etc/traefik/dynamic", content)

    def test_native_static_config_rejects_shipped_shape_drift(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            drifted = root / "drifted.yml"
            drifted.write_text("entryPoints: {}\n", encoding="utf-8")
            routes = root / "routes"
            routes.mkdir()

            with (
                mock.patch.object(discovery, "STATIC_CONFIG", drifted),
                self.assertRaises(RuntimeError),
            ):
                discovery.write_native_static_config(
                    root / "generated.yml",
                    routes,
                    32101,
                    32102,
                )

    def test_native_mode_does_not_require_docker(self) -> None:
        output = io.StringIO()
        with (
            mock.patch.object(discovery, "native_integration") as native,
            mock.patch.object(discovery, "docker_available", return_value=False),
            redirect_stdout(output),
        ):
            result = discovery.main(["--traefik-binary", "/tmp/traefik"])

        self.assertEqual(result, 0)
        native.assert_called_once_with("/tmp/traefik")

    def test_required_integration_fails_without_a_runtime(self) -> None:
        error = io.StringIO()
        with (
            mock.patch.dict(
                discovery.os.environ,
                {"PROA_REQUIRE_TRAEFIK_INTEGRATION": "1"},
                clear=True,
            ),
            mock.patch.object(discovery, "docker_available", return_value=False),
            redirect_stderr(error),
        ):
            result = discovery.main([])

        self.assertEqual(result, 1)
        self.assertNotEqual(error.getvalue(), "")


if __name__ == "__main__":
    unittest.main()
