# Copyright Proa Labs, Inc.
# SPDX-License-Identifier: Apache-2.0

from __future__ import annotations

import argparse
import json
import sys
from http.client import HTTPException
from urllib.error import URLError

from . import __version__
from .client import ProaApiError, ProaDocs


def main() -> None:
    parser = argparse.ArgumentParser(description="Search and read public Proa documentation.")
    parser.add_argument("--base-url", default="https://proa.so")
    parser.add_argument("--timeout", type=float, default=15, help="Request timeout in seconds")
    parser.add_argument("--version", action="version", version=__version__)
    commands = parser.add_subparsers(dest="command", required=True)
    search = commands.add_parser("search", help="Search a documentation phrase")
    search.add_argument("query")
    listing = commands.add_parser("list", help="List public documents")
    for command in (search, listing):
        command.add_argument("--limit", type=int, choices=range(1, 51), metavar="1..50")
        command.add_argument("--cursor")
    get = commands.add_parser("get", help="Read a document by its published path")
    get.add_argument("path")
    get.add_argument("--markdown", action="store_true")
    args = parser.parse_args()
    try:
        client = ProaDocs(args.base_url, timeout=args.timeout)
        if args.command == "get":
            document = client.get(args.path)
            print(document["markdown"] if args.markdown else json.dumps(document, indent=2))
        else:
            page = (
                client.search(args.query, limit=args.limit, cursor=args.cursor)
                if args.command == "search"
                else client.list(limit=args.limit, cursor=args.cursor)
            )
            print(json.dumps(page, indent=2))
    except ProaApiError as error:
        print(
            json.dumps(
                {"error": str(error), "status": error.status, "retry_after": error.retry_after}
            ),
            file=sys.stderr,
        )
        sys.exit(1)
    except (HTTPException, URLError, OSError, ValueError) as error:
        print(json.dumps({"error": str(error)}), file=sys.stderr)
        sys.exit(1)


if __name__ == "__main__":
    main()
