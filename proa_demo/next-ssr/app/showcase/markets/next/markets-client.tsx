// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

"use client";

import { useMemo, useState } from "react";
import type { Quote } from "@lib/markets";

const fmtMoney = new Intl.NumberFormat("en-US", { style: "currency", currency: "USD", minimumFractionDigits: 2 });
const fmtNum = new Intl.NumberFormat("en-US");
const fmtPct = new Intl.NumberFormat("en-US", { style: "percent", minimumFractionDigits: 2 });

const COLUMNS = ["Symbol", "Price", "Chg", "Chg %", "Volume", "Avg vol", "Mkt cap", "P/E", "52w range", "Trend", ""] as const;

function QuoteRow({ q }: { q: Quote }) {
  const up = q.chg >= 0;
  const pos = Math.max(0, Math.min(1, (q.price - q.lo52) / (q.hi52 - q.lo52)));
  return (
    <tr className="border-b hover:bg-gray-50">
      <td className="px-3 py-2">
        <span className="font-semibold">{q.sym}</span>
        <span className="block text-xs text-gray-500">{q.name}</span>
      </td>
      <td className="px-3 text-right tabular-nums">{fmtMoney.format(q.price)}</td>
      <td className={`px-3 text-right tabular-nums ${up ? "text-emerald-600" : "text-rose-600"}`}>
        <span>
          {up ? "+" : ""}
          {fmtMoney.format(q.chg)}
        </span>
      </td>
      <td className={`px-3 text-right tabular-nums ${up ? "text-emerald-600" : "text-rose-600"}`}>
        <span>
          {up ? "▲ " : "▼ "}
          {fmtPct.format(q.chg / q.price)}
        </span>
      </td>
      <td className="px-3 text-right tabular-nums">{fmtNum.format(q.vol)}</td>
      <td className="px-3 text-right tabular-nums">{fmtNum.format(q.avgVol)}</td>
      <td className="px-3 text-right tabular-nums">{fmtMoney.format(q.mktCap)}</td>
      <td className="px-3 text-right tabular-nums">{q.pe.toFixed(1)}</td>
      <td className="px-3">
        <div className="relative h-1.5 w-24 rounded bg-gray-200">
          <div className="absolute h-1.5 rounded bg-blue-500" style={{ width: `${(pos * 100) | 0}%` }} />
          <span className="absolute -top-1 h-3 w-0.5 bg-gray-700" style={{ left: `${(pos * 100) | 0}%` }} />
        </div>
      </td>
      <td className="px-3">
        <svg className="h-6 w-24" viewBox="0 0 96 24" preserveAspectRatio="none">
          <polyline
            points={q.spark.map((v, i) => `${i * 4},${(24 - v).toFixed(1)}`).join(" ")}
            fill="none"
            stroke={up ? "#059669" : "#e11d48"}
          />
        </svg>
      </td>
      <td className="px-3">
        <button className="rounded border px-2 py-0.5 text-xs">Trade</button>
      </td>
    </tr>
  );
}

export function MarketsClient({ quotes }: { quotes: Quote[] }) {
  const [sortCol, setSortCol] = useState<"sym" | "price" | "chg">("sym");
  const sorted = useMemo(() => {
    if (sortCol === "price") return [...quotes].sort((a, b) => b.price - a.price);
    if (sortCol === "chg") return [...quotes].sort((a, b) => b.chg - a.chg);
    return quotes;
  }, [quotes, sortCol]);

  return (
    <table className="w-full text-sm">
      <thead>
        <tr className="border-b text-left text-xs uppercase text-gray-500">
          {COLUMNS.map((c) => (
            <th
              key={c}
              className="cursor-pointer px-3 py-2"
              onClick={() => {
                if (c === "Price") setSortCol("price");
                else if (c === "Chg") setSortCol("chg");
                else setSortCol("sym");
              }}
            >
              {c}
            </th>
          ))}
        </tr>
      </thead>
      <tbody>
        {sorted.map((q) => (
          <QuoteRow key={q.sym} q={q} />
        ))}
      </tbody>
    </table>
  );
}
