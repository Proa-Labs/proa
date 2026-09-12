// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

// Deterministic per-query flight dataset for the flight-results SSR benchmark.
// Seeded by the search query so every (from, to, date) pair yields a stable
// but unique result set, mirroring a real fare search that cannot be cached.

export interface Flight {
  id: number;
  code: string;
  name: string;
  num: number;
  stops: number;
  depMs: number;
  arrMs: number;
  durMin: number;
  from: string;
  to: string;
  price: number;
  taxes: number;
  seats: number;
  co2: number;
  rating: number;
  amenities: string[];
}

export const AIRLINES: ReadonlyArray<readonly [string, string]> = [
  ["UA", "United"],
  ["DL", "Delta"],
  ["AA", "American"],
  ["B6", "JetBlue"],
  ["AS", "Alaska"],
  ["WN", "Southwest"],
  ["F9", "Frontier"],
  ["NK", "Spirit"],
];

const AIRPORTS = ["SFO", "LAX", "JFK", "EWR", "ORD", "DEN", "SEA", "BOS", "ATL", "MIA", "DFW", "PHX"];
const AMENITIES = ["wifi", "power", "tv", "meal"];

function mulberry32(a: number): () => number {
  return function () {
    a |= 0;
    a = (a + 0x6d2b79f5) | 0;
    let t = Math.imul(a ^ (a >>> 15), 1 | a);
    t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

export function seedFromQuery(from: string, to: string, date: string): number {
  let h = 2166136261;
  for (const ch of `${from}|${to}|${date}`) {
    h ^= ch.charCodeAt(0);
    h = Math.imul(h, 16777619);
  }
  return h >>> 0;
}

export function buildFlights(n: number, seed: number): Flight[] {
  const rnd = mulberry32(seed);
  const out: Flight[] = [];
  for (let i = 0; i < n; i++) {
    const [code, name] = AIRLINES[(rnd() * AIRLINES.length) | 0];
    const stops = (rnd() * 3) | 0;
    const dep = (5 + ((rnd() * 16) | 0)) * 60 * 60_000 + ((rnd() * 60) | 0) * 60_000;
    const durMin = 190 + ((rnd() * 400) | 0);
    const base = 89 + ((rnd() * 1100) | 0);
    out.push({
      id: i,
      code,
      name,
      num: 100 + ((rnd() * 4800) | 0),
      stops,
      depMs: dep,
      arrMs: dep + durMin * 60000,
      durMin,
      from: AIRPORTS[(rnd() * AIRPORTS.length) | 0],
      to: AIRPORTS[(rnd() * AIRPORTS.length) | 0],
      price: base,
      taxes: (base * 0.18) | 0,
      seats: 1 + ((rnd() * 9) | 0),
      co2: 180 + ((rnd() * 220) | 0),
      rating: 3 + rnd() * 2,
      amenities: AMENITIES.filter(() => rnd() > 0.4),
    });
  }
  return out;
}
