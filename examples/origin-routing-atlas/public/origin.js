// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

// ORIGIN progressive engine. Everything with a typed rsjs surface lives in
// the islands (theme, tooltips, overlay, tape controls, flood controls);
// this file covers what rsjs rejects by design: the RIS Live WebSocket, bulk
// innerHTML row building, performance timings, and wall-clock formatting,
// wired to the islands through document events.
//
// Honesty contract: the feed banner says LIVE only while the RIS Live socket
// is delivering real messages. If the socket cannot connect, the banner says
// REPLAY and the replayed events are deliberately fictional: unnamed
// operators and RFC 5737 / RFC 3849 documentation ranges only.

const CYAN = "var(--cyan, #3DE1E8)";
const RED = "var(--red, #FF3B30)";
const AMBER = "var(--brand, #DD7627)";
const BONE = "var(--ink, #E5E1D6)";
const MID = "var(--ink3, #8C887E)";

// Replay pool: every range is documentation space, every operator unnamed.
const REPLAY_EVENTS = [
  { headline: "A route to 198.51.100.0/24 disappeared", explanation: "One network stopped offering this path. Other routes remained available, so the range stayed reachable.", prefix: "198.51.100.0/24", origin: "64496", kind: "W" },
  { headline: "A new route for 203.0.113.0/16 appeared", explanation: "A transit provider began carrying this range. Traffic from nearby networks may take a shorter path.", prefix: "203.0.113.0/24", origin: "64499", kind: "A" },
  { headline: "A backup path for 192.0.2.0/24 was withdrawn", explanation: "Routine. The path was one of many; the remaining ones still carry traffic normally.", prefix: "192.0.2.0/24", origin: "64500", kind: "W" },
  { headline: "A regional network re-announced its ranges", explanation: "This network went quiet for six minutes and then re-announced all of its address space.", prefix: "198.51.100.0/24", origin: "64502", kind: "A" },
  { headline: "2001:db8::/32 gained a shorter route", explanation: "A carrier began announcing a direct path, one hop shorter than before.", prefix: "2001:db8::/32", origin: "64505", kind: "A" },
];
const REPLAY_COLLECTORS = ["rrc00", "rrc01", "rrc03", "rrc10", "rrc21"];

const query = (selector) => document.querySelector(selector);
const utc = (withMillis) => {
  const d = new Date();
  const p = (n) => String(n).padStart(2, "0");
  const base = p(d.getUTCHours()) + ":" + p(d.getUTCMinutes()) + ":" + p(d.getUTCSeconds());
  return withMillis ? base + "." + String(d.getUTCMilliseconds()).padStart(3, "0") : base;
};

// Lookup navigation is a full page load; carrying the scroll position
// across it keeps chip-hopping from yanking the reader back to the top.
document.addEventListener("click", (event) => {
  const link = event.target.closest && event.target.closest('a[href^="/?q="]');
  if (link) sessionStorage.setItem("origin.scroll", String(window.scrollY));
});
(function restoreScroll() {
  const saved = sessionStorage.getItem("origin.scroll");
  if (saved === null) return;
  sessionStorage.removeItem("origin.scroll");
  const y = Number(saved) || 0;
  const jump = () => window.scrollTo({ top: y, behavior: "instant" });
  jump();
  // Streamed sections grow the page after this script runs; re-apply once
  // everything has landed.
  window.addEventListener("load", jump, { once: true });
})();

const clockEl = query("[data-clock]");
function tickClock() { if (clockEl) clockEl.textContent = utc(false) + " UTC"; }
setInterval(tickClock, 1000);
tickClock();

const eventsEl = query("[data-events]");
const tapeEl = query("[data-tape]");
const sparkEl = query("[data-spark]");
const rateAnnounceEl = query("[data-rate-announce]");
const rateWithdrawEl = query("[data-rate-withdraw]");
const feedStatusEl = query("[data-feed-status]");
const feedDotEl = query("[data-feed-dot]");

let paused = false;
let filter = "all";
let floodRows = 0;
let mode = "connecting"; // connecting | live | replay
const events = [];
const tape = [];

// Rolling 60s window of real message timestamps for the per-minute rates.
// Until a full window exists the counts are extrapolated to a per-minute
// rate, so the first number a visitor sees is not a warming artifact.
const announceTimes = [];
const withdrawTimes = [];
let ratesStartedAt = null;
let secondCount = 0;

function eventRow(entry, first) {
  const color = entry.kind === "W" ? RED : CYAN;
  const background = first
    ? (entry.kind === "W"
      ? "color-mix(in srgb, var(--red, #FF3B30) 9%, transparent)"
      : "color-mix(in srgb, var(--cyan, #3DE1E8) 3.5%, transparent)")
    : "transparent";
  return '<div style="padding: 14px 22px; border-bottom: 1px solid var(--line-faint, rgba(229,225,214,0.06)); animation: ' + (first ? "org-in .22s ease-out" : "none") + "; background: " + background + ';">' +
    '<div style="display: flex; align-items: baseline; gap: 12px;">' +
    '<span style="width: 6px; height: 6px; border-radius: 50%; background: ' + color + '; flex-shrink: 0; transform: translateY(-2px);"></span>' +
    '<div style="flex: 1;">' +
    '<div style="font-size: 13px; color: var(--ink, #E5E1D6);">' + entry.headline + "</div>" +
    '<div style="font-size: 11px; line-height: 1.6; color: var(--ink3, #8C887E); margin-top: 4px; max-width: 78ch;">' + entry.explanation + "</div>" +
    '<div style="font-size: 10px; color: var(--ink5, #4A4844); margin-top: 6px; white-space: nowrap; overflow: hidden; text-overflow: ellipsis;">' + entry.raw + "</div>" +
    "</div>" +
    '<span style="font-size: 10px; color: var(--ink4, #6F6C64); flex-shrink: 0;">' + entry.time + "</span>" +
    "</div></div>";
}

function pushEvent(entry) {
  events.unshift(entry);
  events.length = Math.min(events.length, 9);
  if (eventsEl) eventsEl.innerHTML = events.map((e, i) => eventRow(e, i === 0)).join("");
}

function tapeRow(entry, first) {
  const color = entry.type === "W" ? RED : CYAN;
  const background = first
    ? (entry.type === "W"
      ? "color-mix(in srgb, var(--red, #FF3B30) 9%, transparent)"
      : "color-mix(in srgb, var(--cyan, #3DE1E8) 7%, transparent)")
    : "transparent";
  return '<div style="display: grid; grid-template-columns: 84px 30px 112px 50px minmax(0, 1fr) 52px; column-gap: 12px; box-sizing: border-box; padding: 6px 22px; border-bottom: 1px solid var(--line-hair, rgba(229,225,214,0.05)); font-size: 11px; white-space: nowrap; background: ' + background + "; animation: " + (first ? "org-in .18s ease-out" : "none") + ';">' +
    '<div style="color: var(--ink4, #6F6C64);">' + entry.time + "</div>" +
    '<div style="color: ' + color + '; font-weight: 600;">' + entry.type + "</div>" +
    '<div style="color: var(--ink, #E5E1D6);">' + entry.prefix + "</div>" +
    '<div style="color: var(--cyan, #3DE1E8);">' + entry.origin + "</div>" +
    '<div style="color: var(--ink3, #8C887E); overflow: hidden; text-overflow: ellipsis; white-space: nowrap;">' + entry.path + "</div>" +
    '<div style="text-align: right; color: var(--ink4, #6F6C64);">' + entry.collector + "</div>" +
    "</div>";
}

function renderTape() {
  if (!tapeEl) return;
  const rows = tape.filter((entry) =>
    filter === "all" || (filter === "removed" ? entry.type === "W" : entry.type === "A"));
  tapeEl.innerHTML = rows.map((entry, index) => tapeRow(entry, index === 0)).join("");
}

function pushTape(entry) {
  tape.unshift(entry);
  tape.length = Math.min(tape.length, 30);
  renderTape();
}

function renderSpark(count) {
  if (!sparkEl) return;
  // Real messages per second, scaled against the largest bar on screen.
  const bar = document.createElement("div");
  bar.dataset.count = count;
  sparkEl.appendChild(bar);
  while (sparkEl.children.length > 42) sparkEl.removeChild(sparkEl.firstChild);
  const bars = [...sparkEl.children];
  const max = Math.max(1, ...bars.map((b) => Number(b.dataset.count) || 0));
  for (const b of bars) {
    const value = Number(b.dataset.count) || 0;
    const h = value / max;
    b.style.cssText = "flex: 1; height: " + Math.max(4, Math.round(h * 100)) + "%; background: " + (h > 0.85 ? RED : CYAN) + "; opacity: " + (0.35 + h * 0.6) + ";";
  }
}

function updateRates() {
  const now = Date.now();
  const cutoff = now - 60000;
  while (announceTimes.length && announceTimes[0] < cutoff) announceTimes.shift();
  while (withdrawTimes.length && withdrawTimes[0] < cutoff) withdrawTimes.shift();
  let announce = "—";
  let withdraw = "—";
  if (ratesStartedAt !== null) {
    const elapsed = Math.min(60, (now - ratesStartedAt) / 1000);
    if (elapsed >= 5) {
      const factor = 60 / elapsed;
      announce = Math.round(announceTimes.length * factor).toLocaleString();
      withdraw = Math.round(withdrawTimes.length * factor).toLocaleString();
    }
  }
  if (rateAnnounceEl) rateAnnounceEl.textContent = announce;
  if (rateWithdrawEl) rateWithdrawEl.textContent = withdraw;
  renderSpark(secondCount);
  secondCount = 0;
}
setInterval(updateRates, 1000);

function feedStatus() {
  if (!feedStatusEl || !feedDotEl) return;
  let label;
  let dot;
  if (floodRows > 0) {
    label = "Feed stood down · " + floodRows.toLocaleString() + " rows mounted";
    dot = "background: " + AMBER + ";";
  } else if (paused) {
    label = "Feed paused";
    dot = "background: " + MID + ";";
  } else if (mode === "live") {
    label = "Every network, live · RIS";
    dot = "background: " + CYAN + "; animation: org-pulse 1.6s ease-in-out infinite;";
  } else if (mode === "replay") {
    label = "Replay · fictional data, RIS unreachable";
    dot = "background: " + AMBER + ";";
  } else {
    label = "Connecting to RIS Live…";
    dot = "background: " + MID + "; animation: org-pulse 1.6s ease-in-out infinite;";
  }
  feedStatusEl.textContent = label;
  feedDotEl.style.cssText = "flex-shrink: 0; width: 6px; height: 6px; " + dot;
}

// ---------------------------------------------------------------------------
// RIS Live: the real firehose. Every message is counted for the rates; the
// tape renders a sampled subset so the DOM keeps up; the narrated feed
// derives one plainly-worded entry every few seconds from a real message.
// ---------------------------------------------------------------------------

let socket = null;
let lastTapeAt = 0;
let lastEventAt = 0;
let replayTimer = null;
let replaySeq = 0;

function narrate(message, kind, prefix, origin) {
  const known = origin && origin !== "?";
  const verb = kind === "W" ? "withdrew" : "announced";
  const headline = known
    ? "AS" + origin + " " + verb + " " + prefix
    : "A route to " + prefix + " was withdrawn";
  const explanation = kind === "W"
    ? "A route for this range was removed at one or more collectors. Other paths may still carry its traffic."
    : "A route for this range was announced or updated. This is the routing table changing in real time.";
  return { headline, explanation, kind };
}

function handleRisMessage(payload) {
  if (paused || floodRows > 0) return;
  const announcements = payload.announcements || [];
  const withdrawals = payload.withdrawals || [];
  const now = Date.now();
  if (ratesStartedAt === null) ratesStartedAt = now;
  for (const a of announcements) {
    for (const _ of (a.prefixes || [])) { announceTimes.push(now); secondCount += 1; }
  }
  for (const _ of withdrawals) { withdrawTimes.push(now); secondCount += 1; }

  const isWithdraw = withdrawals.length > 0 && announcements.length === 0;
  const prefix = isWithdraw
    ? withdrawals[0]
    : announcements[0] && announcements[0].prefixes && announcements[0].prefixes[0];
  if (!prefix) return;
  const hops = (payload.path || []).flat();
  const path = hops.join(" ");
  // The message's `origin` field is the BGP origin attribute (igp/egp), not
  // an ASN; the originating AS is the last hop of the path.
  const origin = hops.length ? hops[hops.length - 1] : "?";
  const collector = (payload.host || "").replace(".ripe.net", "") || "ris";
  const kind = isWithdraw ? "W" : "A";

  if (now - lastTapeAt > 250) {
    lastTapeAt = now;
    pushTape({
      time: utc(true),
      type: kind,
      prefix,
      origin: origin === "?" ? "—" : "AS" + origin,
      path,
      collector,
    });
  }
  if (now - lastEventAt > 3000) {
    lastEventAt = now;
    const n = narrate(payload, kind, prefix, origin);
    pushEvent({
      ...n,
      time: utc(false),
      raw: (kind === "W" ? "WITHDRAW " : "ANNOUNCE ") + prefix +
        (origin === "?" ? "" : "  origin AS" + origin) +
        (path ? "  path " + path : "") + "  collector " + collector,
    });
  }
}

function startReplay() {
  if (replayTimer) return;
  mode = "replay";
  feedStatus();
  replayTimer = setInterval(() => {
    if (paused || floodRows > 0) return;
    const t = REPLAY_EVENTS[replaySeq % REPLAY_EVENTS.length];
    const collector = REPLAY_COLLECTORS[replaySeq % REPLAY_COLLECTORS.length];
    const raw = (t.kind === "W" ? "WITHDRAW " : "ANNOUNCE ") + t.prefix + "  origin AS" + t.origin + "  collector " + collector + "  (replay)";
    pushTape({ time: utc(true), type: t.kind, prefix: t.prefix, origin: "AS" + t.origin, path: "64496 " + t.origin, collector });
    if (replaySeq % 3 === 0) {
      pushEvent({ headline: t.headline + " (replay)", explanation: t.explanation, kind: t.kind, time: utc(false), raw });
    }
    if (ratesStartedAt === null) ratesStartedAt = Date.now();
    if (t.kind === "W") withdrawTimes.push(Date.now()); else announceTimes.push(Date.now());
    secondCount += 1;
    replaySeq += 1;
  }, 900);
}

function stopReplay() {
  clearInterval(replayTimer);
  replayTimer = null;
}

function connectRis() {
  try {
    socket = new WebSocket("wss://ris-live.ripe.net/v1/ws/?client=proa-origin-atlas");
  } catch (e) {
    startReplay();
    return;
  }
  const guard = setTimeout(() => { if (mode !== "live") { try { socket.close(); } catch (e) {} } }, 8000);
  socket.onopen = () => {
    socket.send(JSON.stringify({ type: "ris_subscribe", data: { type: "UPDATE" } }));
  };
  socket.onmessage = (event) => {
    let parsed;
    try { parsed = JSON.parse(event.data); } catch (e) { return; }
    if (parsed.type !== "ris_message" || !parsed.data) return;
    if (mode !== "live") {
      mode = "live";
      stopReplay();
      clearTimeout(guard);
      feedStatus();
    }
    handleRisMessage(parsed.data);
  };
  socket.onerror = () => { if (mode !== "live") startReplay(); };
  socket.onclose = () => {
    socket = null;
    if (mode === "live") { mode = "connecting"; feedStatus(); }
    startReplay();
    // Keep trying to get back to the real thing.
    setTimeout(() => { if (!socket) connectRis(); }, 15000);
  };
}

// Wired to the TapeControls island.
document.addEventListener("origin:pause", () => {
  paused = !paused;
  feedStatus();
});
document.addEventListener("origin:filter", (event) => {
  filter = event.detail || "all";
  for (const other of document.querySelectorAll("[data-tape-filter]")) {
    const key = other.getAttribute("data-tape-filter");
    const active = key === filter;
    const tone = key === "removed" ? RED : key === "new" ? CYAN : BONE;
    other.style.borderColor = active ? tone : "var(--line-soft, rgba(229,225,214,0.18))";
    other.style.color = active ? tone : MID;
  }
  renderTape();
});

// ---------------------------------------------------------------------------
// The flood lab, wired to the FloodControls island. The server number is
// fetched and measured, not modelled; the browser numbers are measured here.
// ---------------------------------------------------------------------------

const floodTable = query("[data-flood-table]");
const floodEmpty = query("[data-flood-empty]");
const floodStage = query("[data-flood-stage]");
const labRows = query("[data-lab-rows]");
const labServer = query("[data-lab-server]");
const labBuild = query("[data-lab-build]");
const labPaint = query("[data-lab-paint]");
const labBytes = query("[data-lab-bytes]");
const receiptRecords = query("[data-receipt-records]");
const labVerdict = query("[data-lab-verdict]");

let lastServerMillis = null;
let lastFlood = null;
function updateVerdict() {
  if (!labVerdict) return;
  if (lastServerMillis === null || lastFlood === null) return;
  const f = lastFlood;
  labVerdict.innerHTML =
    "Proa rendered " + f.count.toLocaleString() + " rows in " +
    '<span style="color: var(--cyan, #3DE1E8);">' + lastServerMillis.toFixed(1) + " ms</span>" +
    ", measured on this server. This browser needed " +
    '<span style="color: var(--brand, #DD7627);">' + (f.paint / 1000).toFixed(2) + " s</span>" +
    " to build and lay out the same rows — and only " + f.build.toFixed(0) +
    " ms of that was building the HTML, the same work the server timed. The rest is layout, a cost only browsers pay.";
  labVerdict.hidden = false;
}

const HOLDERS = ["64496", "64497", "64498", "64499", "64500", "64501", "64502", "64503"];

function labMetric(element, value, color) {
  if (!element) return;
  element.textContent = value;
  element.style.color = color;
}

let baseRecordsText = null;
function receipt() {
  if (!receiptRecords) return;
  if (baseRecordsText === null) baseRecordsText = receiptRecords.textContent || "—";
  if (floodRows === 0) {
    receiptRecords.textContent = baseRecordsText;
    return;
  }
  const base = Number(baseRecordsText.replace(/[^0-9]/g, "")) || 0;
  receiptRecords.textContent = (base + floodRows).toLocaleString();
}

async function flood(count) {
  if (!floodTable) return;
  // A mounted table of this size makes every feed tick a full-document style
  // recalc, so the feeds stand down until it is cleared.
  if (floodStage) floodStage.style.height = "520px";

  // Server side, measured: the server renders the same rows and reports how
  // long the render took.
  labMetric(labServer, "…", MID);
  lastServerMillis = null;
  fetch("/lab/rows?n=" + count)
    .then((r) => r.json())
    .then((s) => {
      // A debug dev binary is an order of magnitude slower than release;
      // say so rather than let the number mislead either way.
      const suffix = s.profile === "release" ? "" : " · debug build";
      labMetric(labServer, s.millis.toFixed(1) + " ms" + suffix, CYAN);
      lastServerMillis = s.millis;
      updateVerdict();
    })
    .catch(() => labMetric(labServer, "unavailable", MID));

  const t0 = performance.now();
  const parts = new Array(count);
  const rowFor = (i) => {
    const n = i;
    if (n % 6 === 5) {
      return { range: "2001:db8:" + (1000 + ((n * 13) % 8000)).toString(16) + "::/48", owner: HOLDERS[n % HOLDERS.length] };
    }
    // RFC 5737 documentation blocks only; the lab renders rows, not claims.
    const block = ["192.0.2", "198.51.100", "203.0.113"][n % 3];
    return { range: block + "." + ((n * 7) % 256) + "/32", owner: HOLDERS[n % HOLDERS.length] };
  };
  for (let i = 0; i < count; i++) {
    const { range, owner } = rowFor(i);
    parts[i] =
      '<div style="display:grid;grid-template-columns:76px 176px 104px minmax(0,1fr) 84px;column-gap:10px;box-sizing:border-box;padding:4px 22px;border-bottom:1px solid var(--line-hair, rgba(229,225,214,.05));font-size:11px;white-space:nowrap">' +
      '<span style="color:var(--ink5, #4A4844)">' + (i + 1) + "</span>" +
      '<span style="color:var(--ink, #E5E1D6)">' + range + "</span>" +
      '<span style="color:#3DE1E8">AS' + owner + "</span>" +
      '<span style="color:var(--ink3, #8C887E)">64496 ' + owner + "</span>" +
      '<span style="color:var(--ink4, #6F6C64);text-align:right">' + ((i * 7) % 1400) + "d</span>" +
      "</div>";
  }
  const COLS = "display:grid;grid-template-columns:76px 176px 104px minmax(0,1fr) 84px;column-gap:10px;box-sizing:border-box;padding:4px 22px";
  const header =
    '<div style="' + COLS + ';position:sticky;top:0;z-index:2;padding-top:9px;padding-bottom:9px;font-size:9px;letter-spacing:0.16em;color:var(--ink4, #6F6C64);text-transform:uppercase;border-bottom:1px solid var(--line, rgba(229,225,214,0.14));background:var(--bg2, #0E0E0F)">' +
    "<div>#</div><div>Address range</div><div>Announced by</div><div>Route</div><div style=\"text-align:right\">Age</div></div>";
  const html = header + parts.join("");
  floodTable.innerHTML = html;
  const build = performance.now() - t0;
  // Reading offsetHeight forces synchronous layout of every inserted row, so
  // the sample is real work rather than a queued frame.
  void floodTable.offsetHeight;
  const paint = performance.now() - t0;
  floodRows = count;
  if (floodEmpty) floodEmpty.hidden = true;
  labMetric(labRows, count.toLocaleString(), BONE);
  labMetric(labBuild, build.toFixed(1) + " ms", BONE);
  labMetric(labPaint, paint.toFixed(1) + " ms", BONE);
  if (labBytes) labBytes.textContent = (html.length / 1024 / 1024).toFixed(2) + " MB of HTML";
  lastFlood = { count, build, paint };
  updateVerdict();
  receipt();
  feedStatus();
}

function clearFlood() {
  if (floodTable) floodTable.innerHTML = "";
  floodRows = 0;
  if (floodEmpty) floodEmpty.hidden = false;
  if (floodStage) floodStage.style.height = "180px";
  const dim = "var(--ink4, #6F6C64)";
  labMetric(labRows, "—", dim);
  labMetric(labServer, "—", dim);
  labMetric(labBuild, "—", dim);
  labMetric(labPaint, "—", dim);
  if (labBytes) labBytes.textContent = "nothing rendered yet";
  lastServerMillis = null;
  lastFlood = null;
  if (labVerdict) { labVerdict.hidden = true; labVerdict.textContent = ""; }
  receipt();
  feedStatus();
}

document.addEventListener("origin:flood", (event) => {
  const count = Number(event.detail || 0);
  if (count > 0) flood(count);
  else clearFlood();
});

feedStatus();
connectRis();
