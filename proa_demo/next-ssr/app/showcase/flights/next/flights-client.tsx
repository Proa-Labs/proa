// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

"use client";

import { useMemo, useState } from "react";
import type { Flight } from "@lib/flights";

const fmtPrice = new Intl.NumberFormat("en-US", { style: "currency", currency: "USD", maximumFractionDigits: 0 });
const fmtTime = new Intl.DateTimeFormat("en-US", { hour: "numeric", minute: "2-digit", timeZone: "UTC" });

function dur(min: number): string {
  return `${(min / 60) | 0}h ${min % 60}m`;
}

function AmenityIcon({ name }: { name: string }) {
  return (
    <svg className={`h-4 w-4 icon-${name}`} viewBox="0 0 24 24" aria-hidden="true">
      <path d="M4 12h16M12 4v16" stroke="currentColor" />
    </svg>
  );
}

function Leg({ f, back }: { f: Flight; back: boolean }) {
  const from = back ? f.to : f.from;
  const to = back ? f.from : f.to;
  return (
    <div className="flex items-center gap-3 py-2">
      <div className="w-20 text-right">
        <div className="text-lg font-semibold">{fmtTime.format(f.depMs)}</div>
        <div className="text-xs text-gray-500">{from}</div>
      </div>
      <div className="relative flex-1">
        <div className="border-t border-gray-300" />
        {f.stops > 0 ? (
          <div className="absolute inset-x-0 -top-1 flex justify-center gap-2">
            {Array.from({ length: f.stops }, (_, i) => (
              <span key={i} className="h-2 w-2 rounded-full bg-rose-400" />
            ))}
          </div>
        ) : null}
        <div className="mt-1 text-center text-xs text-gray-500">
          {dur(f.durMin)} · {f.stops === 0 ? "Nonstop" : `${f.stops} stop${f.stops > 1 ? "s" : ""}`}
        </div>
      </div>
      <div className="w-20">
        <div className="text-lg font-semibold">{fmtTime.format(f.arrMs)}</div>
        <div className="text-xs text-gray-500">{to}</div>
      </div>
    </div>
  );
}

const FARES = ["Basic", "Main", "Comfort+", "First"];

function FlightCard({ f }: { f: Flight }) {
  return (
    <article className="card rounded-lg border bg-white shadow-sm" data-flight={f.id}>
      <header className="flex items-center gap-2 border-b px-4 py-2">
        <svg className="h-6 w-6" viewBox="0 0 24 24">
          <circle cx={12} cy={12} r={10} />
          <path d="M2 12h20" />
        </svg>
        <span className="font-medium">{f.name}</span>
        <span className="text-xs text-gray-500">
          {f.code} {f.num}
          {f.stops > 0 ? ` · ${f.code} ${f.num + 1}` : ""}
        </span>
        {f.seats < 4 ? (
          <span className="rounded bg-amber-100 px-1.5 text-xs text-amber-800">Only {f.seats} left</span>
        ) : null}
        <span className="ml-auto rounded bg-emerald-50 px-1.5 text-xs text-emerald-700">{f.rating.toFixed(1)} ★</span>
      </header>
      <div className="flex">
        <div className="flex-1 px-4">
          <Leg f={f} back={false} />
          <Leg f={f} back={true} />
          <div className="flex items-center gap-1.5 pb-2">
            {f.amenities.map((a) => (
              <AmenityIcon key={a} name={a} />
            ))}
            <span className="rounded bg-gray-100 px-1.5 text-xs text-gray-600">{f.co2} kg CO₂e</span>
          </div>
        </div>
        <div className="flex w-36 flex-col items-end justify-center gap-1 border-l px-4 py-3">
          <div className="text-xs text-gray-400 line-through">{fmtPrice.format(f.price + 40)}</div>
          <div className="text-2xl font-bold">{fmtPrice.format(f.price)}</div>
          <div className="text-xs text-gray-500">round trip / person</div>
          <button className="mt-1 rounded-md bg-blue-600 px-3 py-1.5 text-sm text-white">
            <span>Select</span>
          </button>
        </div>
      </div>
      <div className="grid grid-cols-4 gap-px border-t bg-gray-100 text-xs">
        {FARES.map((fare, j) => (
          <div key={fare} className="bg-white px-3 py-2">
            <div className="font-medium">{fare}</div>
            <div className="text-gray-500">{fmtPrice.format(f.price + j * 75)}</div>
            <div className={j < 1 ? "text-rose-600" : "text-emerald-600"}>{j < 1 ? "No changes" : "Free changes"}</div>
          </div>
        ))}
      </div>
    </article>
  );
}

type SortKey = "best" | "cheapest" | "fastest";

export function FlightsClient({ flights }: { flights: Flight[] }) {
  const [sort, setSort] = useState<SortKey>("best");
  const sorted = useMemo(() => {
    if (sort === "cheapest") return [...flights].sort((a, b) => a.price - b.price);
    if (sort === "fastest") return [...flights].sort((a, b) => a.durMin - b.durMin);
    return flights;
  }, [flights, sort]);

  return (
    <main className="flex flex-1 flex-col gap-3">
      <div className="flex items-center gap-2 text-sm text-gray-600">
        <span className="font-semibold">{flights.length} results</span>
        {(["best", "cheapest", "fastest"] as const).map((key) => (
          <button
            key={key}
            onClick={() => setSort(key)}
            className={key === sort ? "rounded bg-blue-600 px-2 py-1 text-white" : "rounded border px-2 py-1"}
          >
            {key[0].toUpperCase() + key.slice(1)}
          </button>
        ))}
      </div>
      {sorted.map((f) => (
        <FlightCard key={f.id} f={f} />
      ))}
    </main>
  );
}
