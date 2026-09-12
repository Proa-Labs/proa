// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

// Deterministic market watchlist dataset for the dashboard SSR benchmark.
// Models a live quotes table: prices move every second in production, so the
// page is dynamically rendered per request.

export interface Quote {
  sym: string;
  name: string;
  price: number;
  chg: number;
  vol: number;
  avgVol: number;
  mktCap: number;
  pe: number;
  lo52: number;
  hi52: number;
  spark: number[];
}

function mulberry32(a: number): () => number {
  return function () {
    a |= 0;
    a = (a + 0x6d2b79f5) | 0;
    let t = Math.imul(a ^ (a >>> 15), 1 | a);
    t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

export function buildQuotes(n: number, seed: number): Quote[] {
  const rnd = mulberry32(seed);
  const out: Quote[] = [];
  for (let i = 0; i < n; i++) {
    const price = 5 + rnd() * 800;
    out.push({
      sym: `TKR${i}`,
      name: `Company ${i} Holdings Inc.`,
      price,
      chg: (rnd() - 0.45) * 12,
      vol: (rnd() * 90e6) | 0,
      avgVol: (rnd() * 70e6) | 0,
      mktCap: Math.round(rnd() * 900e9),
      pe: 4 + rnd() * 80,
      lo52: price * (0.5 + rnd() * 0.3),
      hi52: price * (1.1 + rnd() * 0.5),
      spark: Array.from({ length: 24 }, () => Math.round(rnd() * 200) / 10),
    });
  }
  return out;
}
