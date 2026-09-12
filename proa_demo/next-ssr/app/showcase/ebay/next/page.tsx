// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

import type { Metadata } from "next";
import { EBAY_STYLE_CHIPS, EBAY_STYLE_MAIN, buildEbayListings } from "@lib/ebay";
import { DEFAULT_QUERY, RealHeadAssets, RealSrpChrome } from "@react-ssr/chrome.generated";
import { fmtCount } from "../../../../../marko-server/helpers_srp_real.mjs";
import { EbayClient } from "./ebay-client";

export const dynamic = "force-dynamic";
export const runtime = "nodejs";

type SearchParams = Promise<Record<string, string | string[] | undefined>>;

export async function generateMetadata({ searchParams }: { searchParams: SearchParams }): Promise<Metadata> {
  const params = await searchParams;
  const q = typeof params.q === "string" ? params.q : DEFAULT_QUERY;
  return { title: `${q} for sale | eBay` };
}

export default async function EbayNextPage({ searchParams }: { searchParams: SearchParams }) {
  const params = await searchParams;
  const n = Math.max(1, Math.min(1000, Number(params.n) || 60));
  const q = typeof params.q === "string" ? params.q : DEFAULT_QUERY;
  const listings = await buildEbayListings(n);
  const totalFmt = fmtCount(117_406 + listings.length * 7);

  return (
    <>
      <RealHeadAssets styleMain={EBAY_STYLE_MAIN} styleChips={EBAY_STYLE_CHIPS} />
      <RealSrpChrome
        query={q}
        totalFmt={totalFmt}
        styleMain={EBAY_STYLE_MAIN}
        styleChips={EBAY_STYLE_CHIPS}
        river={<EbayClient listings={listings} />}
      />
    </>
  );
}
