'use strict';
// Throwaway sizing benchmark: how long does React 19 renderToString take for
// candidate demo page shapes on this machine? Run with:
//   NODE_ENV=production node ssr_sizing_bench.cjs
// Deleted after use; not part of the demo.

const React = require('react');
const { renderToString } = require('react-dom/server');
const h = React.createElement;

console.log('react', React.version, '| node', process.version, '| NODE_ENV', process.env.NODE_ENV);

// Deterministic PRNG so every run renders identical data.
function mulberry32(a) {
  return function () {
    a |= 0;
    a = (a + 0x6d2b79f5) | 0;
    let t = Math.imul(a ^ (a >>> 15), 1 | a);
    t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

// Cached formatters (best practice; the fair configuration).
const fmtPrice = new Intl.NumberFormat('en-US', { style: 'currency', currency: 'USD', maximumFractionDigits: 0 });
const fmtPrice2 = new Intl.NumberFormat('en-US', { style: 'currency', currency: 'USD', minimumFractionDigits: 2 });
const fmtNum = new Intl.NumberFormat('en-US');
const fmtPct = new Intl.NumberFormat('en-US', { style: 'percent', minimumFractionDigits: 2 });
const fmtTime = new Intl.DateTimeFormat('en-US', { hour: 'numeric', minute: '2-digit' });

// ---------------------------------------------------------------------------
// Workload A: flight search results (Kayak/Expedia archetype)
// ---------------------------------------------------------------------------

const AIRLINES = [['UA', 'United'], ['DL', 'Delta'], ['AA', 'American'], ['B6', 'JetBlue'], ['AS', 'Alaska'], ['WN', 'Southwest'], ['F9', 'Frontier'], ['NK', 'Spirit']];
const AIRPORTS = ['SFO', 'LAX', 'JFK', 'EWR', 'ORD', 'DEN', 'SEA', 'BOS', 'ATL', 'MIA', 'DFW', 'PHX'];
const AMENITIES = ['wifi', 'power', 'tv', 'meal'];

function makeFlights(n, seed) {
  const rnd = mulberry32(seed);
  const out = [];
  for (let i = 0; i < n; i++) {
    const [code, name] = AIRLINES[(rnd() * AIRLINES.length) | 0];
    const stops = (rnd() * 3) | 0;
    const dep = new Date(2026, 6, 4, 5 + ((rnd() * 16) | 0), (rnd() * 60) | 0);
    const durMin = 190 + ((rnd() * 400) | 0);
    const base = 89 + ((rnd() * 1100) | 0);
    out.push({
      id: i, code, name,
      num: 100 + ((rnd() * 4800) | 0),
      stops, dep, durMin,
      arr: new Date(dep.getTime() + durMin * 60000),
      from: AIRPORTS[(rnd() * AIRPORTS.length) | 0],
      to: AIRPORTS[(rnd() * AIRPORTS.length) | 0],
      price: base, taxes: (base * 0.18) | 0,
      seats: 1 + ((rnd() * 9) | 0),
      co2: 180 + ((rnd() * 220) | 0),
      rating: 3 + rnd() * 2,
      amenities: AMENITIES.filter(() => rnd() > 0.4),
    });
  }
  return out;
}

function Icon(name) {
  return h('svg', { key: name, className: 'h-4 w-4 icon-' + name, viewBox: '0 0 24 24', 'aria-hidden': 'true' },
    h('path', { d: 'M4 12h16M12 4v16', stroke: 'currentColor' }));
}

function dur(min) { return ((min / 60) | 0) + 'h ' + (min % 60) + 'm'; }

function Leg({ f, back }) {
  const from = back ? f.to : f.from;
  const to = back ? f.from : f.to;
  return h('div', { className: 'flex items-center gap-3 py-2' },
    h('div', { className: 'w-20 text-right' },
      h('div', { className: 'text-lg font-semibold' }, fmtTime.format(f.dep)),
      h('div', { className: 'text-xs text-gray-500' }, from)),
    h('div', { className: 'relative flex-1' },
      h('div', { className: 'border-t border-gray-300' }),
      f.stops > 0
        ? h('div', { className: 'absolute inset-x-0 -top-1 flex justify-center gap-2' },
            Array.from({ length: f.stops }, (_, i) => h('span', { key: i, className: 'h-2 w-2 rounded-full bg-rose-400' })))
        : null,
      h('div', { className: 'mt-1 text-center text-xs text-gray-500' },
        dur(f.durMin), ' · ', f.stops === 0 ? 'Nonstop' : f.stops + ' stop' + (f.stops > 1 ? 's' : ''))),
    h('div', { className: 'w-20' },
      h('div', { className: 'text-lg font-semibold' }, fmtTime.format(f.arr)),
      h('div', { className: 'text-xs text-gray-500' }, to)));
}

function FlightCard({ f }) {
  return h('article', { className: 'card rounded-lg border bg-white shadow-sm', 'data-flight': f.id },
    h('header', { className: 'flex items-center gap-2 border-b px-4 py-2' },
      h('svg', { className: 'h-6 w-6', viewBox: '0 0 24 24' }, h('circle', { cx: 12, cy: 12, r: 10 }), h('path', { d: 'M2 12h20' })),
      h('span', { className: 'font-medium' }, f.name),
      h('span', { className: 'text-xs text-gray-500' }, f.code + ' ' + f.num + (f.stops > 0 ? ' · ' + f.code + ' ' + (f.num + 1) : '')),
      f.seats < 4 ? h('span', { className: 'rounded bg-amber-100 px-1.5 text-xs text-amber-800' }, 'Only ' + f.seats + ' left') : null,
      h('span', { className: 'ml-auto rounded bg-emerald-50 px-1.5 text-xs text-emerald-700' }, f.rating.toFixed(1) + ' ★')),
    h('div', { className: 'flex' },
      h('div', { className: 'flex-1 px-4' },
        h(Leg, { f, back: false }),
        h(Leg, { f, back: true }),
        h('div', { className: 'flex items-center gap-1.5 pb-2' },
          f.amenities.map((a) => Icon(a)),
          h('span', { className: 'rounded bg-gray-100 px-1.5 text-xs text-gray-600' }, f.co2 + ' kg CO₂e'))),
      h('div', { className: 'flex w-36 flex-col items-end justify-center gap-1 border-l px-4 py-3' },
        h('div', { className: 'text-xs text-gray-400 line-through' }, fmtPrice.format(f.price + 40)),
        h('div', { className: 'text-2xl font-bold' }, fmtPrice.format(f.price)),
        h('div', { className: 'text-xs text-gray-500' }, 'round trip / person'),
        h('button', { className: 'mt-1 rounded-md bg-blue-600 px-3 py-1.5 text-sm text-white' }, h('span', null, 'Select')))),
    h('div', { className: 'grid grid-cols-4 gap-px border-t bg-gray-100 text-xs' },
      ['Basic', 'Main', 'Comfort+', 'First'].map((fare, j) =>
        h('div', { key: fare, className: 'bg-white px-3 py-2' },
          h('div', { className: 'font-medium' }, fare),
          h('div', { className: 'text-gray-500' }, fmtPrice.format(f.price + j * 75)),
          h('div', { className: j < 1 ? 'text-rose-600' : 'text-emerald-600' }, j < 1 ? 'No changes' : 'Free changes')))));
}

function FilterGroup(title, opts) {
  return h('fieldset', { key: title, className: 'border-b pb-3' },
    h('legend', { className: 'py-2 text-sm font-semibold' }, title),
    opts.map((o) =>
      h('label', { key: o, className: 'flex items-center gap-2 py-1 text-sm' },
        h('input', { type: 'checkbox', defaultChecked: true, className: 'h-4 w-4' }),
        h('span', null, o),
        h('span', { className: 'ml-auto text-xs text-gray-400' }, '12'))));
}

function FlightsPage({ flights }) {
  return h('div', { className: 'min-h-screen bg-gray-50' },
    h('header', { className: 'flex items-center gap-4 border-b bg-white px-6 py-3' },
      h('span', { className: 'text-xl font-bold text-blue-700' }, 'proatravel'),
      ['Flights', 'Hotels', 'Cars', 'Packages'].map((x) => h('a', { key: x, href: '#', className: 'text-sm text-gray-600' }, x)),
      h('button', { className: 'ml-auto rounded border px-3 py-1 text-sm' }, 'Sign in')),
    h('div', { className: 'mx-auto flex max-w-6xl gap-6 px-4 py-6' },
      h('aside', { className: 'w-56 shrink-0' },
        FilterGroup('Stops', ['Nonstop', '1 stop', '2+ stops']),
        FilterGroup('Airlines', AIRLINES.map(([, n]) => n).slice(0, 6)),
        FilterGroup('Times', ['Morning', 'Afternoon', 'Evening', 'Red-eye']),
        FilterGroup('Cabin', ['Economy', 'Premium', 'Business', 'First'])),
      h('main', { className: 'flex flex-1 flex-col gap-3' },
        h('div', { className: 'flex items-center gap-2 text-sm text-gray-600' },
          h('span', { className: 'font-semibold' }, flights.length + ' results'),
          ['Best', 'Cheapest', 'Fastest'].map((x, i) =>
            h('button', { key: x, className: i === 0 ? 'rounded bg-blue-600 px-2 py-1 text-white' : 'rounded border px-2 py-1' }, x))),
        flights.map((f) => h(FlightCard, { key: f.id, f })))));
}

// ---------------------------------------------------------------------------
// Workload B: live market watchlist table (finance dashboard archetype)
// ---------------------------------------------------------------------------

function makeQuotes(n, seed) {
  const rnd = mulberry32(seed);
  const out = [];
  for (let i = 0; i < n; i++) {
    const price = 5 + rnd() * 800;
    out.push({
      sym: 'TKR' + i,
      name: 'Company ' + i + ' Holdings Inc.',
      price,
      chg: (rnd() - 0.45) * 12,
      vol: (rnd() * 90e6) | 0,
      avgVol: (rnd() * 70e6) | 0,
      mktCap: (rnd() * 900e9) | 0,
      pe: 4 + rnd() * 80,
      lo52: price * (0.5 + rnd() * 0.3),
      hi52: price * (1.1 + rnd() * 0.5),
      spark: Array.from({ length: 24 }, () => (price * (0.9 + rnd() * 0.2)).toFixed(1)),
    });
  }
  return out;
}

function QuoteRow({ q, naive }) {
  const up = q.chg >= 0;
  const pos = (q.price - q.lo52) / (q.hi52 - q.lo52);
  const money = naive ? (v) => '$' + v.toLocaleString('en-US', { minimumFractionDigits: 2 }) : (v) => fmtPrice2.format(v);
  const num = naive ? (v) => v.toLocaleString('en-US') : (v) => fmtNum.format(v);
  const pct = naive ? (v) => (v * 100).toLocaleString('en-US', { minimumFractionDigits: 2 }) + '%' : (v) => fmtPct.format(v);
  return h('tr', { className: 'border-b hover:bg-gray-50' },
    h('td', { className: 'px-3 py-2' }, h('span', { className: 'font-semibold' }, q.sym), h('span', { className: 'block text-xs text-gray-500' }, q.name)),
    h('td', { className: 'px-3 text-right tabular-nums' }, money(q.price)),
    h('td', { className: 'px-3 text-right tabular-nums ' + (up ? 'text-emerald-600' : 'text-rose-600') },
      h('span', null, (up ? '+' : '') + money(q.chg))),
    h('td', { className: 'px-3 text-right tabular-nums ' + (up ? 'text-emerald-600' : 'text-rose-600') },
      h('span', null, (up ? '▲ ' : '▼ ') + pct(q.chg / q.price))),
    h('td', { className: 'px-3 text-right tabular-nums' }, num(q.vol)),
    h('td', { className: 'px-3 text-right tabular-nums' }, num(q.avgVol)),
    h('td', { className: 'px-3 text-right tabular-nums' }, money(q.mktCap)),
    h('td', { className: 'px-3 text-right tabular-nums' }, q.pe.toFixed(1)),
    h('td', { className: 'px-3' },
      h('div', { className: 'relative h-1.5 w-24 rounded bg-gray-200' },
        h('div', { className: 'absolute h-1.5 rounded bg-blue-500', style: { width: ((pos * 100) | 0) + '%' } }),
        h('span', { className: 'absolute -top-1 h-3 w-0.5 bg-gray-700', style: { left: ((pos * 100) | 0) + '%' } }))),
    h('td', { className: 'px-3' },
      h('svg', { className: 'h-6 w-24', viewBox: '0 0 96 24', preserveAspectRatio: 'none' },
        h('polyline', { points: q.spark.map((v, i) => (i * 4) + ',' + (24 - (v % 24)).toFixed(1)).join(' '), fill: 'none', stroke: up ? '#059669' : '#e11d48' }))),
    h('td', { className: 'px-3' }, h('button', { className: 'rounded border px-2 py-0.5 text-xs' }, 'Trade')));
}

function MarketPage({ quotes, naive }) {
  return h('div', { className: 'min-h-screen bg-white' },
    h('header', { className: 'flex gap-4 border-b px-6 py-3' },
      h('span', { className: 'font-bold' }, 'proabrokerage'),
      ['Markets', 'Watchlists', 'Portfolio', 'News'].map((x) => h('a', { key: x, href: '#', className: 'text-sm text-gray-600' }, x))),
    h('table', { className: 'w-full text-sm' },
      h('thead', null, h('tr', { className: 'border-b text-left text-xs uppercase text-gray-500' },
        ['Symbol', 'Price', 'Chg', 'Chg %', 'Volume', 'Avg vol', 'Mkt cap', 'P/E', '52w range', 'Trend', ''].map((x) => h('th', { key: x, className: 'px-3 py-2' }, x)))),
      h('tbody', null, quotes.map((q) => h(QuoteRow, { key: q.sym, q, naive })))));
}

// ---------------------------------------------------------------------------
// Harness
// ---------------------------------------------------------------------------

function bench(label, makeTree, iters, warmup) {
  for (let i = 0; i < warmup; i++) renderToString(makeTree());
  const times = [];
  let html = '';
  for (let i = 0; i < iters; i++) {
    const t0 = process.hrtime.bigint();
    html = renderToString(makeTree());
    const t1 = process.hrtime.bigint();
    times.push(Number(t1 - t0) / 1e6);
  }
  times.sort((a, b) => a - b);
  const med = times[(times.length / 2) | 0];
  const p95 = times[Math.min(times.length - 1, (times.length * 0.95) | 0)];
  const tags = (html.match(/<[a-zA-Z]/g) || []).length;
  console.log(
    label.padEnd(34),
    ('med ' + med.toFixed(2) + 'ms').padEnd(13),
    ('p95 ' + p95.toFixed(2) + 'ms').padEnd(13),
    ('html ' + (html.length / 1024).toFixed(0) + 'KB').padEnd(12),
    'elements ' + tags,
  );
  return { med, tags, bytes: html.length };
}

console.log('\n--- Workload A: flight search results page ---');
for (const [n, iters] of [[60, 40], [100, 30], [200, 20], [400, 10]]) {
  const flights = makeFlights(n, 42);
  bench('flights x' + n, () => h(FlightsPage, { flights }), iters, Math.min(15, iters));
}

console.log('\n--- Workload B: market watchlist table (cached Intl formatters) ---');
for (const [n, iters] of [[200, 30], [500, 15], [1000, 10]]) {
  const quotes = makeQuotes(n, 7);
  bench('watchlist x' + n, () => h(MarketPage, { quotes, naive: false }), iters, Math.min(10, iters));
}

console.log('\n--- Workload B variant: naive toLocaleString per cell (common in real apps) ---');
{
  const quotes = makeQuotes(500, 7);
  bench('watchlist x500 naive Intl', () => h(MarketPage, { quotes, naive: true }), 5, 2);
}
