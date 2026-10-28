# Copyright Proa Labs, Inc.
# SPDX-License-Identifier: Apache-2.0

import json
import subprocess
import sys
import threading
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from urllib.parse import parse_qs, urlsplit

import pytest

from proa_docs import ProaApiError, ProaDocs

SUMMARY = {
    "title": "Example",
    "path": "/docs/example",
    "markdown_url": "/docs/example.md",
    "description": "Example",
}


@pytest.fixture
def api():
    requests = []

    class Handler(BaseHTTPRequestHandler):
        def do_GET(self):
            parsed = urlsplit(self.path)
            query = parse_qs(parsed.query)
            requests.append((parsed.path, query, self.headers))
            if query.get("q") == ["truncated"]:
                self.send_response(200)
                self.send_header("Content-Type", "application/json")
                self.send_header("Content-Length", "100")
                self.end_headers()
                self.wfile.write(b'{"results":')
                self.close_connection = True
                return
            if parsed.path == "/api/v1/search" and query.get("q") == ["limited"]:
                self.send_response(429)
                self.send_header("Content-Type", "application/problem+json")
                self.send_header("Retry-After", "12")
                body = {
                    "status": 429,
                    "code": "rate_limited",
                    "detail": "Wait for the shared budget",
                }
            elif parsed.path == "/api/v1/docs/page":
                self.send_response(200)
                body = {**SUMMARY, "markdown": "# Sample\n\n<literal>"}
            else:
                self.send_response(200)
                cursor = query.get("cursor")
                body = {
                    "results": [{**SUMMARY, "path": "/first" if cursor is None else "/second"}],
                    "total": 2,
                    "next_cursor": "next +/&" if cursor is None else None,
                }
            self.end_headers()
            self.wfile.write(json.dumps(body).encode())

        def log_message(self, *_args):
            pass

    server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
    worker = threading.Thread(target=server.serve_forever, daemon=True)
    worker.start()
    try:
        yield f"http://127.0.0.1:{server.server_port}", requests
    finally:
        server.shutdown()
        server.server_close()
        worker.join()


def test_query_encoding_and_document_reads(api):
    base, requests = api
    client = ProaDocs(base)
    client.search("A & B/C?", limit=5, cursor="0")
    assert requests[0][0] == "/api/v1/search"
    assert requests[0][1] == {"q": ["A & B/C?"], "limit": ["5"], "cursor": ["0"]}
    assert requests[0][2].get("Authorization") is None
    assert client.get("/docs/a b&c.md")["markdown"] == "# Sample\n\n<literal>"
    assert requests[1][1]["path"] == ["/docs/a b&c.md"]


def test_pagination_terminates_and_encodes_opaque_cursors(api):
    base, requests = api
    assert [page["path"] for page in ProaDocs(base).documents()] == ["/first", "/second"]
    assert requests[1][1]["cursor"] == ["next +/&"]
    assert len(requests) == 2


def test_repeating_cursor_fails(monkeypatch):
    client = ProaDocs()
    monkeypatch.setattr(
        client, "list", lambda **_: {"results": [], "total": 0, "next_cursor": "repeat"}
    )
    with pytest.raises(ValueError, match="repeating"):
        list(client.documents())


def test_error_details_and_retry_after_without_automatic_retry(api):
    base, requests = api
    with pytest.raises(ProaApiError) as raised:
        ProaDocs(base).search("limited")
    assert raised.value.status == 429
    assert raised.value.retry_after == 12
    assert raised.value.problem["code"] == "rate_limited"
    assert len(requests) == 1


def test_cli_json_markdown_and_errors(api):
    base, _ = api
    command = [sys.executable, "-m", "proa_docs.cli", "--base-url", base]
    result = subprocess.run(
        [*command, "get", "/docs/example", "--markdown"], capture_output=True, text=True, check=True
    )
    assert result.stdout == "# Sample\n\n<literal>\n"
    assert result.stderr == ""
    result = subprocess.run([*command, "list"], capture_output=True, text=True, check=True)
    assert json.loads(result.stdout)["total"] == 2
    result = subprocess.run(
        [*command, "search", "limited"], capture_output=True, text=True, check=False
    )
    assert result.returncode == 1
    assert result.stdout == ""
    assert json.loads(result.stderr)["retry_after"] == 12


def test_cli_truncated_response_is_a_json_error(api):
    base, _ = api
    result = subprocess.run(
        [sys.executable, "-m", "proa_docs.cli", "--base-url", base, "search", "truncated"],
        capture_output=True,
        text=True,
        check=False,
    )
    assert result.returncode == 1
    assert result.stdout == ""
    assert isinstance(json.loads(result.stderr)["error"], str)


@pytest.mark.parametrize(
    "base", ["file:///etc/passwd", "https://user:secret@example.com", "https://example.com/?q=x"]
)
def test_invalid_origins(base):
    with pytest.raises(ValueError):
        ProaDocs(base)


@pytest.mark.parametrize("timeout", [0, -1, float("nan"), float("inf")])
def test_invalid_timeouts(timeout):
    with pytest.raises(ValueError):
        ProaDocs(timeout=timeout)
