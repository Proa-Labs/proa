#!/usr/bin/env python3
# Copyright Proa Labs, Inc.
# SPDX-License-Identifier: Apache-2.0

"""Exercise the public contract of a freshly deployed Proa marketing site."""

from __future__ import annotations

import argparse
import json
import re
import sys
import time
import urllib.error
import urllib.parse
import urllib.request
from dataclasses import dataclass
from typing import Callable


MAX_RESPONSE_BYTES = 8 * 1_048_576
REVISION_RE = re.compile(r"(?:[0-9a-fA-F]{40}|[0-9a-fA-F]{64})\Z")


class SmokeError(RuntimeError):
    """The deployment did not satisfy its public HTTP contract."""


@dataclass(frozen=True)
class Check:
    path: str
    validate: Callable[[bytes, str], None]


class RejectRedirects(urllib.request.HTTPRedirectHandler):
    """A smoke check must validate the configured host, not a redirect target."""

    def redirect_request(self, *_args: object, **_kwargs: object) -> None:
        return None


HTTP_OPENER = urllib.request.build_opener(RejectRedirects)


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser()
    parser.add_argument("--url", required=True)
    parser.add_argument("--expected-revision", default="")
    parser.add_argument("--attempts", type=int, default=18)
    parser.add_argument("--interval-seconds", type=float, default=10.0)
    parser.add_argument("--request-timeout-seconds", type=float, default=15.0)
    parser.add_argument("--timeout-seconds", type=float, default=300.0)
    args = parser.parse_args()
    if not 1 <= args.attempts <= 60:
        parser.error("--attempts must be between 1 and 60")
    if not 0 <= args.interval_seconds <= 60:
        parser.error("--interval-seconds must be between 0 and 60")
    if not 0.1 <= args.request_timeout_seconds <= 60:
        parser.error("--request-timeout-seconds must be between 0.1 and 60")
    if not 1 <= args.timeout_seconds <= 1_200:
        parser.error("--timeout-seconds must be between 1 and 1200")
    return args


def validated_base_url(value: str) -> str:
    try:
        parts = urllib.parse.urlsplit(value.strip())
        hostname = parts.hostname
        parts.port
    except ValueError as error:
        raise SmokeError("smoke URL is malformed") from error
    if parts.scheme not in {"http", "https"} or not hostname:
        raise SmokeError("smoke URL must be an absolute HTTP(S) URL")
    if parts.username or parts.password or parts.query or parts.fragment:
        raise SmokeError("smoke URL must not contain credentials, a query, or a fragment")
    if parts.path.rstrip("/"):
        raise SmokeError("smoke URL must not contain a path")
    if parts.scheme == "http" and hostname not in {"localhost", "127.0.0.1", "::1"}:
        raise SmokeError("remote smoke URL must use HTTPS")
    return urllib.parse.urlunsplit((parts.scheme, parts.netloc, "", "", ""))


def validated_expected_revision(value: str) -> str | None:
    revision = value.strip()
    if not revision:
        return None
    if not REVISION_RE.fullmatch(revision):
        raise SmokeError("expected revision must be a full 40- or 64-character Git commit")
    return revision.lower()


def validate_text(expected: bytes) -> Callable[[bytes, str], None]:
    def validate(body: bytes, _content_type: str) -> None:
        if body.strip() != expected:
            raise SmokeError(f"expected body {expected!r}, received {body[:80]!r}")

    return validate


def validate_html(body: bytes, content_type: str) -> None:
    if "text/html" not in content_type:
        raise SmokeError(f"expected HTML content type, received {content_type!r}")
    lowered = body[:32_768].lower()
    if b"<html" not in lowered or b"proa" not in lowered:
        raise SmokeError("home page did not contain the expected Proa HTML shell")


def validate_css(body: bytes, content_type: str) -> None:
    if "text/css" not in content_type:
        raise SmokeError(f"expected CSS content type, received {content_type!r}")
    if len(body) < 1_000:
        raise SmokeError(f"compiled stylesheet was unexpectedly small ({len(body)} bytes)")


def validate_showcase_html(body: bytes, content_type: str) -> None:
    if "text/html" not in content_type:
        raise SmokeError(f"expected showcase HTML content type, received {content_type!r}")
    lowered = body[:262_144].lower()
    if (
        b"<html" not in lowered
        or b"airbnb" not in lowered
        or b"air-parity-shell" not in lowered
    ):
        raise SmokeError("Airbnb showcase did not contain its expected rendered shell")


def validate_showcase_javascript(body: bytes, content_type: str) -> None:
    if "javascript" not in content_type:
        raise SmokeError(f"expected showcase JavaScript content type, received {content_type!r}")
    if len(body) < 1_000 or b"airbnb" not in body.lower():
        raise SmokeError("Airbnb showcase JavaScript was missing or unexpectedly small")


def validate_showcase_image(body: bytes, content_type: str) -> None:
    if "image/jpeg" not in content_type:
        raise SmokeError(f"expected showcase JPEG content type, received {content_type!r}")
    if len(body) < 1_000 or not body.startswith(b"\xff\xd8\xff"):
        raise SmokeError("Airbnb showcase image was missing or invalid")


def validate_service_metadata(body: bytes, content_type: str) -> None:
    if "json" not in content_type:
        raise SmokeError(f"expected JSON content type, received {content_type!r}")
    payload = json.loads(body)
    if not isinstance(payload, dict) or not isinstance(payload.get("interfaces"), list):
        raise SmokeError("service metadata does not declare interfaces")


def validate_openapi(body: bytes, content_type: str) -> None:
    if "json" not in content_type:
        raise SmokeError(f"expected JSON content type, received {content_type!r}")
    payload = json.loads(body)
    if (
        not isinstance(payload, dict)
        or payload.get("openapi") != "3.1.0"
        or not isinstance(payload.get("paths"), dict)
    ):
        raise SmokeError("OpenAPI document does not match the published 3.1 contract")


CHECKS = (
    Check("/healthz", validate_text(b"ok")),
    Check("/readyz", validate_text(b"ready")),
    Check("/", validate_html),
    Check("/static/styles.css", validate_css),
    Check("/showcase/airbnb/proa", validate_showcase_html),
    Check("/assets/airbnb-showcase.js", validate_showcase_javascript),
    Check("/images/airbnb/bedroom.jpg", validate_showcase_image),
    Check("/.well-known/proa-site.json", validate_service_metadata),
    Check("/openapi.json", validate_openapi),
)


def fetch(
    base_url: str,
    check: Check,
    timeout: float,
    expected_revision: str | None = None,
) -> int:
    url = urllib.parse.urljoin(base_url.rstrip("/") + "/", check.path.lstrip("/"))
    request = urllib.request.Request(
        url,
        headers={"Accept": "*/*", "User-Agent": "proa-marketing-smoke/1"},
    )
    with HTTP_OPENER.open(request, timeout=timeout) as response:
        body = response.read(MAX_RESPONSE_BYTES + 1)
        if len(body) > MAX_RESPONSE_BYTES:
            raise SmokeError(f"{check.path} response exceeded {MAX_RESPONSE_BYTES} bytes")
        if response.status != 200:
            raise SmokeError(f"{check.path} returned unexpected HTTP {response.status}")
        if check.path == "/readyz" and expected_revision is not None:
            cache_directives = {
                directive.strip().lower()
                for directive in response.headers.get("Cache-Control", "").split(",")
            }
            if "no-store" not in cache_directives:
                raise SmokeError("/readyz must return Cache-Control: no-store")
            revision_headers = response.headers.get_all("X-Proa-Revision", [])
            if len(revision_headers) != 1:
                raise SmokeError("/readyz must return exactly one x-proa-revision header")
            deployed_revision = revision_headers[0].strip().lower()
            if deployed_revision != expected_revision:
                raise SmokeError(
                    f"/readyz reported revision {deployed_revision!r}, "
                    f"expected {expected_revision!r}"
                )
        content_type = response.headers.get("Content-Type", "").lower()
        check.validate(body, content_type)
        return response.status


def run(args: argparse.Namespace) -> None:
    base_url = validated_base_url(args.url)
    expected_revision = validated_expected_revision(args.expected_revision)
    deadline = time.monotonic() + args.timeout_seconds
    last_error: Exception | None = None
    for attempt in range(1, args.attempts + 1):
        started = time.monotonic()
        try:
            for check in CHECKS:
                remaining = deadline - time.monotonic()
                if remaining <= 0:
                    raise SmokeError(
                        f"overall smoke deadline of {args.timeout_seconds:g}s expired"
                    )
                request_timeout = min(args.request_timeout_seconds, max(0.1, remaining))
                status = fetch(base_url, check, request_timeout, expected_revision)
                print(
                    f"marketing_smoke attempt={attempt} path={check.path} status={status}",
                    flush=True,
                )
            elapsed = time.monotonic() - started
            print(f"marketing_smoke result=passed elapsed_seconds={elapsed:.2f}", flush=True)
            return
        except (SmokeError, json.JSONDecodeError, urllib.error.URLError, TimeoutError) as error:
            last_error = error
            print(
                f"marketing_smoke attempt={attempt} result=retry error={type(error).__name__}",
                flush=True,
            )
            remaining = deadline - time.monotonic()
            if attempt < args.attempts and remaining > 0:
                time.sleep(min(args.interval_seconds, remaining))
            elif remaining <= 0:
                break
    raise SmokeError(f"site did not become healthy after {args.attempts} attempts: {last_error}")


def main() -> int:
    try:
        run(parse_args())
    except SmokeError as error:
        annotation = str(error).replace("%", "%25").replace("\r", "%0D").replace("\n", "%0A")
        print(f"::error title=Marketing smoke checks failed::{annotation}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
