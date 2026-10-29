# Copyright Proa Labs, Inc.
# SPDX-License-Identifier: Apache-2.0

"""Notify IndexNow of public sitemap URLs after a successful production deploy.

No account or private credential is involved. The deployed public key must match
the reviewed checkout before notification. Acceptance is not an indexing promise.
"""

import json
import re
import urllib.parse
import urllib.request
import xml.etree.ElementTree as ET
from pathlib import Path

ORIGIN = "https://proa.so"
KEY_PATH = "/indexnow-key.txt"
ENDPOINT = "https://api.indexnow.org/indexnow"
MAX_BODY = 4 * 1024 * 1024


def fetch(url):
    request = urllib.request.Request(url, headers={"User-Agent": "Proa-indexing/1.0"})
    with urllib.request.urlopen(request, timeout=30) as response:
        if urllib.parse.urlsplit(response.url).netloc != "proa.so":
            raise ValueError("Production discovery redirected to another host")
        body = response.read(MAX_BODY + 1)
        if len(body) > MAX_BODY:
            raise ValueError("Production discovery response exceeds limit")
        return body


def payload(key, deployed_key, sitemap):
    if not re.fullmatch(r"[a-f0-9]{32}", key) or deployed_key.strip() != key:
        raise ValueError("The production IndexNow key does not match this checkout")
    root = ET.fromstring(sitemap)
    if root.tag != "{http://www.sitemaps.org/schemas/sitemap/0.9}urlset":
        raise ValueError("Expected a sitemap URL set")
    urls = list(dict.fromkeys(node.text for node in root.findall("{*}url/{*}loc")))
    if not 1 <= len(urls) <= 10000:
        raise ValueError("Expected between 1 and 10000 sitemap URLs")
    for url in urls:
        parsed = urllib.parse.urlsplit(url)
        if parsed.scheme != "https" or parsed.netloc != "proa.so" or parsed.fragment:
            raise ValueError("Sitemap contains a non-production URL")
    return {"host": "proa.so", "key": key, "keyLocation": ORIGIN + KEY_PATH, "urlList": urls}


def main():
    root = Path(__file__).resolve().parents[2]
    key = (root / "proa_marketing/indexnow-key.txt").read_text().strip()
    body = payload(key, fetch(ORIGIN + KEY_PATH).decode(), fetch(ORIGIN + "/sitemap.xml"))
    request = urllib.request.Request(
        ENDPOINT,
        data=json.dumps(body).encode(),
        method="POST",
        headers={
            "Content-Type": "application/json; charset=utf-8",
            "User-Agent": "Proa-indexing/1.0",
        },
    )
    with urllib.request.urlopen(request, timeout=30) as response:
        if response.status not in (200, 202):
            raise RuntimeError(f"Unexpected IndexNow status: {response.status}")
        print(
            f"IndexNow accepted {len(body['urlList'])} URLs (HTTP {response.status}); "
            "indexing is asynchronous."
        )


if __name__ == "__main__":
    main()
