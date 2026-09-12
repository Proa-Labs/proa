// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

import type { Listing } from "./ebay";

function nowUs(): number {
  return Number(process.hrtime.bigint()) / 1000;
}

export type TimedHtml = {
  html: string;
  renderUs: number;
  docUs: number;
};

// The playground preview path. The page route owns the pixel-faithful
// dynamic document (RealSrpChrome + EbayClient); rendering that tree again
// here would require a parallel hook-free component copy, which is exactly
// the kind of duplicate that drifts. Instead the preview self-fetches the
// page route, so preview and full view are byte-identical by construction.
// Per-listing prop overrides are not supported on this engine's preview;
// `count` maps to the page's `?n=`.
export async function renderEbayDocumentWithTimings(listings: Listing[]): Promise<TimedHtml> {
  const port = process.env.PORT || "5402";
  const url = `http://127.0.0.1:${port}/showcase/ebay/next?n=${listings.length}`;
  const docStart = nowUs();
  const res = await fetch(url, { cache: "no-store" });
  const html = await res.text();
  const docUs = nowUs() - docStart;
  return { html, renderUs: docUs, docUs };
}

export async function renderEbayDocument(listings: Listing[]): Promise<string> {
  return (await renderEbayDocumentWithTimings(listings)).html;
}
