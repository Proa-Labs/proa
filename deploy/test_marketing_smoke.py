# Copyright Proa Labs, Inc.
# SPDX-License-Identifier: Apache-2.0

from __future__ import annotations

import json
import threading
import unittest
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from types import SimpleNamespace

import marketing_smoke


EXPECTED_REVISION = "0123456789abcdef0123456789abcdef01234567"

RESPONSES = {
    "/healthz": ("text/plain; charset=utf-8", b"ok"),
    "/readyz": ("text/plain; charset=utf-8", b"ready"),
    "/": ("text/html; charset=utf-8", b"<!doctype html><html><title>Proa</title></html>"),
    "/static/styles.css": ("text/css; charset=utf-8", b".proa{}" * 200),
    "/showcase/airbnb/proa": (
        "text/html; charset=utf-8",
        b'<!doctype html><html><div class="air-parity-shell">Airbnb</div></html>',
    ),
    "/assets/airbnb-showcase.js": (
        "application/javascript; charset=utf-8",
        b"const airbnbShowcase = true;" * 100,
    ),
    "/images/airbnb/bedroom.jpg": (
        "image/jpeg",
        b"\xff\xd8\xff" + b" " * 2_000,
    ),
    "/.well-known/proa-site.json": (
        "application/json",
        json.dumps({"interfaces": [{"kind": "openapi"}]}).encode(),
    ),
    "/openapi.json": (
        "application/vnd.oai.openapi+json",
        json.dumps({"openapi": "3.1.0", "paths": {"/api/search": {}}}).encode(),
    ),
}


class MarketingHandler(BaseHTTPRequestHandler):
    override: dict[str, tuple[str, bytes]] = {}
    revision = EXPECTED_REVISION
    emit_revision = True
    ready_cache_control = "no-store"

    def log_message(self, _format: str, *_args: object) -> None:
        pass

    def do_GET(self) -> None:
        content_type, body = type(self).override.get(self.path, RESPONSES[self.path])
        self.send_response(200)
        self.send_header("Content-Type", content_type)
        self.send_header("Content-Length", str(len(body)))
        if self.path == "/readyz" and type(self).emit_revision:
            self.send_header("X-Proa-Revision", type(self).revision)
        if self.path == "/readyz" and type(self).ready_cache_control:
            self.send_header("Cache-Control", type(self).ready_cache_control)
        self.end_headers()
        self.wfile.write(body)


class MarketingSmokeTests(unittest.TestCase):
    def setUp(self) -> None:
        MarketingHandler.override = {}
        MarketingHandler.revision = EXPECTED_REVISION
        MarketingHandler.emit_revision = True
        MarketingHandler.ready_cache_control = "no-store"
        self.server = ThreadingHTTPServer(("127.0.0.1", 0), MarketingHandler)
        self.thread = threading.Thread(target=self.server.serve_forever, daemon=True)
        self.thread.start()
        self.url = f"http://127.0.0.1:{self.server.server_port}"

    def tearDown(self) -> None:
        self.server.shutdown()
        self.thread.join()
        self.server.server_close()

    def args(self) -> SimpleNamespace:
        return SimpleNamespace(
            url=self.url,
            expected_revision=EXPECTED_REVISION,
            attempts=1,
            interval_seconds=0,
            request_timeout_seconds=2,
            timeout_seconds=5,
        )

    def test_public_contract_passes(self) -> None:
        marketing_smoke.run(self.args())

    def test_invalid_openapi_fails(self) -> None:
        MarketingHandler.override["/openapi.json"] = (
            "application/json",
            json.dumps({"openapi": "3.0.0", "paths": {}}).encode(),
        )
        with self.assertRaises(marketing_smoke.SmokeError):
            marketing_smoke.run(self.args())

    def test_broken_showcase_contract_fails(self) -> None:
        invalid_responses = {
            "/showcase/airbnb/proa": ("text/html", b"<html>Airbnb</html>"),
            "/assets/airbnb-showcase.js": ("application/javascript", b"tiny"),
            "/images/airbnb/bedroom.jpg": (
                "image/jpeg",
                b"not-a-jpeg" * 200,
            ),
        }
        for path, response in invalid_responses.items():
            with self.subTest(path=path):
                MarketingHandler.override[path] = response
                with self.assertRaises(marketing_smoke.SmokeError):
                    marketing_smoke.run(self.args())
                MarketingHandler.override = {}

    def test_missing_revision_header_fails(self) -> None:
        MarketingHandler.emit_revision = False
        with self.assertRaisesRegex(marketing_smoke.SmokeError, "x-proa-revision"):
            marketing_smoke.run(self.args())

    def test_cacheable_revision_response_fails(self) -> None:
        MarketingHandler.ready_cache_control = "public, max-age=300"
        with self.assertRaisesRegex(marketing_smoke.SmokeError, "Cache-Control: no-store"):
            marketing_smoke.run(self.args())

    def test_mismatched_revision_header_fails(self) -> None:
        MarketingHandler.revision = "f" * 40
        with self.assertRaisesRegex(marketing_smoke.SmokeError, "reported revision"):
            marketing_smoke.run(self.args())

    def test_invalid_expected_revision_fails_before_requests(self) -> None:
        args = self.args()
        args.expected_revision = "short"
        with self.assertRaisesRegex(marketing_smoke.SmokeError, "full 40- or 64-character"):
            marketing_smoke.run(args)

    def test_non_object_json_contracts_fail_cleanly(self) -> None:
        for path in ("/.well-known/proa-site.json", "/openapi.json"):
            with self.subTest(path=path):
                MarketingHandler.override[path] = ("application/json", b"[]")
                with self.assertRaises(marketing_smoke.SmokeError):
                    marketing_smoke.run(self.args())
                MarketingHandler.override = {}

    def test_overall_deadline_is_enforced(self) -> None:
        args = self.args()
        args.timeout_seconds = 0
        with self.assertRaisesRegex(marketing_smoke.SmokeError, "deadline"):
            marketing_smoke.run(args)

    def test_unsafe_base_urls_are_rejected(self) -> None:
        for value in (
            "http://marketing.example",
            "https://user:password@marketing.example",
            "https://marketing.example/path",
            "https://marketing.example?token=secret",
        ):
            with self.subTest(value=value), self.assertRaises(marketing_smoke.SmokeError):
                marketing_smoke.validated_base_url(value)


if __name__ == "__main__":
    unittest.main()
