# Copyright Proa Labs, Inc.
# SPDX-License-Identifier: Apache-2.0

import unittest

from indexnow import payload


class IndexNowTests(unittest.TestCase):
    key = "a" * 32

    def sitemap(self, *urls):
        return (
            '<urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">'
            + "".join(f"<url><loc>{url}</loc></url>" for url in urls)
            + "</urlset>"
        )

    def test_only_public_production_urls_are_submitted_once(self):
        result = payload(
            self.key,
            self.key + "\n",
            self.sitemap("https://proa.so/", "https://proa.so/docs/api", "https://proa.so/"),
        )
        self.assertEqual(result["urlList"], ["https://proa.so/", "https://proa.so/docs/api"])

    def test_proof_and_origin_must_match_before_publication(self):
        with self.assertRaises(ValueError):
            payload(self.key, "b" * 32, self.sitemap("https://proa.so/"))
        for url in [
            "http://proa.so/",
            "https://example.org/",
            "https://proa.so.example.org/",
            "https://user@proa.so/",
            "https://proa.so/#fragment",
            "",
        ]:
            with self.subTest(url=url), self.assertRaises(ValueError):
                payload(self.key, self.key, self.sitemap(url))
        with self.assertRaises(ValueError):
            payload(self.key, self.key, self.sitemap())
        with self.assertRaises(ValueError):
            payload(self.key, self.key, "<sitemapindex/>")


if __name__ == "__main__":
    unittest.main()
