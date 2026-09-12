// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

import type { Metadata } from "next";
import { buildQuotes } from "@lib/markets";
import { MarketsClient } from "./markets-client";

export const dynamic = "force-dynamic";
export const runtime = "nodejs";

export const metadata: Metadata = {
  title: "Market Watchlist SSR Benchmark",
};

export default async function MarketsNextPage({
  searchParams,
}: {
  searchParams: Promise<Record<string, string | string[] | undefined>>;
}) {
  const params = await searchParams;
  const n = Math.min(2000, Number(params.n) || 500);
  // Live quotes change every tick in production (hence force-dynamic); the
  // fixed seed keeps benchmark runs comparable.
  const quotes = buildQuotes(n, 7);

  return (
    <div className="min-h-screen bg-white">
      <header className="flex gap-4 border-b px-6 py-3">
        <span className="font-bold">proabrokerage</span>
        {["Markets", "Watchlists", "Portfolio", "News"].map((x) => (
          <a key={x} href="#" className="text-sm text-gray-600">
            {x}
          </a>
        ))}
      </header>
      <MarketsClient quotes={quotes} />
    </div>
  );
}
