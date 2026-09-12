// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

import type { Metadata } from "next";
import { AIRLINES, buildFlights, seedFromQuery } from "@lib/flights";
import { FlightsClient } from "./flights-client";

export const dynamic = "force-dynamic";
export const runtime = "nodejs";

export const metadata: Metadata = {
  title: "Flight Results SSR Benchmark",
};

function FilterGroup({ title, opts }: { title: string; opts: string[] }) {
  return (
    <fieldset className="border-b pb-3">
      <legend className="py-2 text-sm font-semibold">{title}</legend>
      {opts.map((o) => (
        <label key={o} className="flex items-center gap-2 py-1 text-sm">
          <input type="checkbox" defaultChecked className="h-4 w-4" />
          <span>{o}</span>
          <span className="ml-auto text-xs text-gray-400">12</span>
        </label>
      ))}
    </fieldset>
  );
}

export default async function FlightsNextPage({
  searchParams,
}: {
  searchParams: Promise<Record<string, string | string[] | undefined>>;
}) {
  const params = await searchParams;
  const n = Math.min(1000, Number(params.n) || 200);
  const from = typeof params.from === "string" ? params.from : "SFO";
  const to = typeof params.to === "string" ? params.to : "JFK";
  const date = typeof params.date === "string" ? params.date : "benchmark-default";
  const flights = buildFlights(n, seedFromQuery(from, to, date));

  return (
    <div className="min-h-screen bg-gray-50">
      <header className="flex items-center gap-4 border-b bg-white px-6 py-3">
        <span className="text-xl font-bold text-blue-700">proatravel</span>
        {["Flights", "Hotels", "Cars", "Packages"].map((x) => (
          <a key={x} href="#" className="text-sm text-gray-600">
            {x}
          </a>
        ))}
        <button className="ml-auto rounded border px-3 py-1 text-sm">Sign in</button>
      </header>
      <div className="mx-auto flex max-w-6xl gap-6 px-4 py-6">
        <aside className="w-56 shrink-0">
          <FilterGroup title="Stops" opts={["Nonstop", "1 stop", "2+ stops"]} />
          <FilterGroup title="Airlines" opts={AIRLINES.slice(0, 6).map(([, name]) => name)} />
          <FilterGroup title="Times" opts={["Morning", "Afternoon", "Evening", "Red-eye"]} />
          <FilterGroup title="Cabin" opts={["Economy", "Premium", "Business", "First"]} />
        </aside>
        <FlightsClient flights={flights} />
      </div>
    </div>
  );
}
