# Copyright Proa Labs, Inc.
# SPDX-License-Identifier: Apache-2.0

from __future__ import annotations

import json
import math
from collections.abc import Iterator
from typing import TypedDict, cast
from urllib.error import HTTPError
from urllib.parse import urlencode, urlsplit, urlunsplit
from urllib.request import Request, urlopen


class DocumentSummary(TypedDict):
    title: str
    path: str
    markdown_url: str
    description: str


class Document(DocumentSummary):
    markdown: str


class DocumentList(TypedDict):
    results: list[DocumentSummary]
    total: int
    next_cursor: str | None


class ProaApiError(Exception):
    """An HTTP failure, including the problem document and Retry-After seconds."""

    def __init__(self, status: int, problem: object, retry_after: int | None) -> None:
        detail = problem.get("detail") if isinstance(problem, dict) else None
        super().__init__(detail if isinstance(detail, str) else f"Proa API returned HTTP {status}")
        self.status = status
        self.problem = problem
        self.retry_after = retry_after


class ProaDocs:
    def __init__(self, base_url: str = "https://proa.so", *, timeout: float = 15) -> None:
        parts = urlsplit(base_url)
        if (
            parts.scheme not in {"http", "https"}
            or not parts.netloc
            or parts.username is not None
            or parts.password is not None
            or parts.query
            or parts.fragment
        ):
            raise ValueError("base_url must be HTTP(S) without credentials, query, or fragment")
        if not math.isfinite(timeout) or timeout <= 0:
            raise ValueError("timeout must be finite and positive")
        self.base_url = urlunsplit(parts).rstrip("/")
        self.timeout = timeout

    def _request(self, endpoint: str, **parameters: str | int | None) -> object:
        query = urlencode({key: value for key, value in parameters.items() if value is not None})
        request = Request(
            f"{self.base_url}/{endpoint}?{query}",
            headers={"Accept": "application/json", "User-Agent": "proa-docs-python/0.1.0"},
            method="GET",
        )
        try:
            with urlopen(request, timeout=self.timeout) as response:
                return json.load(response)
        except HTTPError as error:
            with error:
                try:
                    problem = json.load(error)
                except (ValueError, UnicodeError):
                    problem = None
                delay = error.headers.get("Retry-After", "")
                retry_after = int(delay) if delay.isascii() and delay.isdigit() else None
                raise ProaApiError(error.code, problem, retry_after) from error

    def search(
        self, q: str, *, limit: int | None = None, cursor: str | None = None
    ) -> DocumentList:
        return cast(DocumentList, self._request("api/v1/search", q=q, limit=limit, cursor=cursor))

    def list(self, *, limit: int | None = None, cursor: str | None = None) -> DocumentList:
        return cast(DocumentList, self._request("api/v1/docs", limit=limit, cursor=cursor))

    def get(self, path: str) -> Document:
        return cast(Document, self._request("api/v1/docs/page", path=path))

    def documents(self, *, limit: int = 50) -> Iterator[DocumentSummary]:
        """Read all pages; reject a repeated cursor rather than looping forever."""
        seen: set[str] = set()
        cursor = None
        while True:
            page = self.list(limit=limit, cursor=cursor)
            yield from page["results"]
            cursor = page["next_cursor"]
            if cursor is None:
                return
            if not isinstance(cursor, str) or cursor in seen:
                raise ValueError("Invalid or repeating Proa pagination cursor")
            seen.add(cursor)
