// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

import { readFileSync } from "node:fs";
import { join } from "node:path";
import { pathToFileURL } from "node:url";

export type Listing = {
  itemId: number;
  imageHash: string;
  title: string;
  subtitleParts?: string[];
  caption?: string | null;
  starsFull?: number;
  hasHalfStar?: boolean;
  reviewCount?: number;
  priceCents: number;
  originalPriceCents?: number | null;
  format?: string;
  deliveryCents: number;
  location: string;
  watcherCount: number;
  couponPct?: number | null;
  sellerName?: string;
  sellerFeedbackPctX10?: number;
  sellerFeedbackCount?: number;
};

const ebayDataDir = join(process.cwd(), "..", "..", "proa_showcase", "src", "ebay", "data");

const shellPre = readFileSync(join(ebayDataDir, "shell_pre.html"), "utf8");
const shellPost = readFileSync(join(ebayDataDir, "shell_post.html"), "utf8");

// The captured page's own stylesheet (extracted by scripts/gen_srp_chrome.py)
// plus the chips toolbar stylesheet, shared verbatim with all engines.
export const EBAY_STYLE_MAIN = readFileSync(
  join(ebayDataDir, "srp_real_style.css"),
  "utf8",
);
export const EBAY_STYLE_CHIPS = readFileSync(
  join(ebayDataDir, "srp_chips_style.css"),
  "utf8",
);

// Retain the previous helper names for callers outside this demo package.
export const MARKETPLACE_STYLE = EBAY_STYLE_MAIN;
export const MARKETPLACE_CHIPS_STYLE = EBAY_STYLE_CHIPS;

const SHELL_LIST_OPEN_RE =
  /\s*<ul class="(?:srp-river-results srp-list|srp-results srp-list clearfix)">\s*$/;
const SHELL_LIST_CLOSE_RE = /^\s*<\/ul>\s*/;
let helperPromise: Promise<{ buildListings: (n: number) => Listing[] }> | null = null;

function loadEbayHelper(): Promise<{ buildListings: (n: number) => Listing[] }> {
  if (!helperPromise) {
    const helperUrl = pathToFileURL(
      join(process.cwd(), "..", "marko-server", "helpers_srp_real.mjs"),
    ).href;
    const runtimeImport = new Function("specifier", "return import(specifier)") as (
      specifier: string,
    ) => Promise<{ buildListings: (n: number) => Listing[] }>;
    helperPromise = runtimeImport(helperUrl);
  }
  return helperPromise;
}

function removeShellListOpen(html: string): string {
  return html.replace(SHELL_LIST_OPEN_RE, "");
}

function removeShellListClose(html: string): string {
  return html.replace(SHELL_LIST_CLOSE_RE, "");
}

export async function buildEbayListings(count = 60): Promise<Listing[]> {
  const safeCount = Math.max(1, Math.min(1000, Number(count) || 60));
  const { buildListings: buildListingsHelper } = await loadEbayHelper();
  return buildListingsHelper(safeCount) as Listing[];
}

function applyOverrides(listings: Listing[], overrides: any): Listing[] {
  if (!Array.isArray(overrides)) return listings;
  for (let i = 0; i < overrides.length && i < listings.length; i++) {
    const ov = overrides[i] || {};
    const listing = listings[i];
    if (typeof ov.title === "string") listing.title = ov.title;
    if (typeof ov.subtitle === "string") {
      listing.subtitleParts = ov.subtitle
        .split(/[·|]/)
        .map((part: string) => part.trim())
        .filter(Boolean);
    }
    if (typeof ov.imageHash === "string") listing.imageHash = ov.imageHash;
    if (typeof ov.priceCents === "number") listing.priceCents = ov.priceCents;
    if (Object.prototype.hasOwnProperty.call(ov, "originalPriceCents")) {
      listing.originalPriceCents =
        ov.originalPriceCents == null ? null : Number(ov.originalPriceCents);
    }
    if (typeof ov.freeDelivery === "boolean" && ov.freeDelivery) listing.deliveryCents = 0;
    if (typeof ov.deliveryCents === "number") listing.deliveryCents = ov.deliveryCents;
    if (typeof ov.location === "string") listing.location = ov.location;
    if (typeof ov.watcherCount === "number") listing.watcherCount = ov.watcherCount;
    if (Object.prototype.hasOwnProperty.call(ov, "couponPct")) {
      listing.couponPct = ov.couponPct == null ? null : Number(ov.couponPct);
    }
    if (typeof ov.starsFull === "number") listing.starsFull = Math.min(5, ov.starsFull);
    if (typeof ov.hasHalfStar === "boolean") listing.hasHalfStar = ov.hasHalfStar;
    if (typeof ov.reviewCount === "number") listing.reviewCount = ov.reviewCount;
    if (typeof ov.format === "string") listing.format = ov.format;
    if (typeof ov.caption === "string") listing.caption = ov.caption === "None" ? null : ov.caption;
    if (typeof ov.sellerName === "string") listing.sellerName = ov.sellerName;
    if (typeof ov.sellerFeedbackPctX10 === "number") {
      listing.sellerFeedbackPctX10 = ov.sellerFeedbackPctX10;
    }
    if (typeof ov.sellerFeedbackCount === "number") {
      listing.sellerFeedbackCount = ov.sellerFeedbackCount;
    }
  }
  return listings;
}

export async function buildEbayListingsFromProps(props: any): Promise<Listing[]> {
  const listings = await buildEbayListings(props?.count ?? 60);
  return applyOverrides(listings, props?.listings);
}

export function wrapEbayBody(bodyHtml: string, listings: Listing[]): string {
  const seed = `<script type="application/json" id="__app_data__">${JSON.stringify(listings).replace(/</g, "\\u003c")}</script>`;
  return `${removeShellListOpen(shellPre)}<div id="__app_root__">${bodyHtml}</div>${seed}${removeShellListClose(shellPost)}`;
}
