// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import vm from "node:vm";

const source = name => readFileSync(new URL(`../src/${name}.js`, import.meta.url), "utf8");

function browser({ ready = "loading", idle = true } = {}) {
  const callbacks = new Map();
  const scripts = [], idleCallbacks = [], timers = [], errors = [];
  const document = {
    readyState: ready, baseURI: "https://example.com/",
    currentScript: { dataset: {}, nonce: "request-nonce" },
    head: { appendChild: script => scripts.push(script) },
    createElement(kind) {
      const events = new Map();
      return { kind, events, addEventListener: (name, fn) => events.set(name, fn),
        remove() { this.removed = true; }, focus(options) { this.focused = true; this.preventScroll = options.preventScroll; } };
    },
  };
  const window = {
    addEventListener(name, fn) { callbacks.set(name, [...(callbacks.get(name) || []), fn]); },
    dispatchEvent: event => errors.push(event.detail),
    setTimeout: fn => timers.push(fn),
  };
  if (idle) window.requestIdleCallback = (fn, options) => { assert.equal(options.timeout, 2000); idleCallbacks.push(fn); };
  const context = vm.createContext({ window, document, URL, CustomEvent: class { constructor(type, options) { this.type = type; this.detail = options.detail; } } });
  const run = name => vm.runInContext(source(name), context);
  run("loader");
  function ga(id = "G-TEST123456", consent = "granted", strategy = "after-load") {
    document.currentScript = { nonce: "request-nonce", dataset: { measurementId: id, consent, strategy, pageView: "true" } };
    run("google");
  }
  function load() {
    document.readyState = "complete";
    const listeners = callbacks.get("load") || [];
    callbacks.delete("load");
    listeners.forEach(fn => fn());
  }
  return { window, document, scripts, idleCallbacks, timers, errors, run, ga, load, context };
}

test("after-load scripts wait and deduplicate equivalent URLs", async () => {
  const b = browser(), api = b.window.proaThirdParty;
  const first = api.loadScript("/sdk.js", { nonce: "abc", integrity: "sha384-test" });
  assert.equal(first, api.loadScript("https://example.com/sdk.js"));
  assert.equal(b.scripts.length, 0);
  b.load();
  assert.equal(b.scripts.length, 1);
  assert.equal(b.scripts[0].nonce, "abc");
  assert.equal(b.scripts[0].crossOrigin, "anonymous");
  assert.equal(b.scripts[0].integrity, "sha384-test");
  assert.equal(b.scripts[0].async, true);
  b.scripts[0].events.get("load")();
  assert.equal(await first, b.scripts[0]);
});

test("async starts immediately; idle waits for load and bounded idle", () => {
  const b = browser();
  b.window.proaThirdParty.loadScript("/async.js", { strategy: "async" });
  b.window.proaThirdParty.loadScript("/idle.js", { strategy: "idle" });
  assert.equal(b.scripts.length, 1);
  b.load();
  assert.equal(b.scripts.length, 1);
  b.idleCallbacks.shift()();
  assert.equal(b.scripts.length, 2);
});

test("late scripts and browsers without idle callbacks still load", () => {
  const b = browser({ ready: "complete", idle: false });
  b.window.proaThirdParty.loadScript("/late.js");
  b.window.proaThirdParty.loadScript("/idle.js", { strategy: "idle" });
  assert.equal(b.scripts.length, 1);
  b.timers.shift()();
  assert.equal(b.scripts.length, 2);
});

test("failed loads report errors and permit an explicit retry", async () => {
  const b = browser({ ready: "complete" }), api = b.window.proaThirdParty;
  const first = api.loadScript("/retry.js");
  const handled = first.catch(api.reportError);
  b.scripts[0].events.get("error")();
  await handled;
  assert.equal(b.errors.length, 1);
  assert.equal(b.scripts[0].removed, true);
  api.loadScript("/retry.js");
  assert.equal(b.scripts.length, 2);
});

test("shared loader bootstrap preserves an existing registry", () => {
  const b = browser({ ready: "complete" });
  const loadScript = b.window.proaThirdParty.loadScript;
  b.run("loader");
  assert.equal(b.window.proaThirdParty.loadScript, loadScript);
});

test("malformed browser API URLs reject without a request", async () => {
  const b = browser({ ready: "complete" });
  await assert.rejects(b.window.proaThirdParty.loadScript("https://[bad"));
  assert.equal(b.scripts.length, 0);
});

test("GA can disable the automatic page view", () => {
  const b = browser();
  b.document.currentScript.dataset = {
    measurementId: "G-TEST123456", consent: "granted", pageView: "false",
  };
  b.run("google");
  assert.equal(b.window.dataLayer.find(args => args[0] === "config")[2].send_page_view, false);
});

test("GA queues config and events once, preserves queues, and shares one SDK", () => {
  const b = browser();
  b.window.dataLayer = ["existing"];
  b.ga(); b.ga(); b.ga("G-SECOND123");
  const api = b.window.proaThirdParty.googleAnalytics;
  assert.equal(api.track("G-TEST123456", "signup", { send_to: "wrong", method: "email" }), true);
  const queue = b.window.dataLayer;
  assert.equal(queue[0], "existing");
  const configurations = queue.filter(args => args[0] === "config");
  assert.equal(configurations.length, 2);
  assert.equal(configurations[0][2].allow_google_signals, false);
  assert.equal(configurations[0][2].allow_ad_personalization_signals, false);
  assert.equal(queue.at(-1)[2].send_to, "G-TEST123456");
  assert.equal(b.scripts.length, 0);
  b.load();
  assert.equal(b.scripts.length, 1);
  assert.equal(b.scripts[0].nonce, "request-nonce");
});

test("consent required drops events and makes no request until granted", () => {
  const b = browser({ ready: "complete" });
  b.ga("G-TEST123456", "required");
  const api = b.window.proaThirdParty.googleAnalytics;
  assert.equal(api.track("G-TEST123456", "before-consent"), false);
  assert.equal(b.window.dataLayer.length, 0);
  assert.equal(b.scripts.length, 0);
  api.setConsent(true);
  assert.equal(b.scripts.length, 1);
  assert.equal(api.track("G-TEST123456", "after-consent"), true);
  api.setConsent(false);
  assert.equal(api.track("G-TEST123456", "after-revocation"), false);
  assert.equal(b.window["ga-disable-G-TEST123456"], true);
});

test("revocation while waiting cancels a scheduled load and later consent retries", () => {
  const b = browser();
  b.ga();
  const api = b.window.proaThirdParty.googleAnalytics;
  api.setConsent(false);
  b.load();
  assert.equal(b.scripts.length, 0);
  api.setConsent(true);
  assert.equal(b.scripts.length, 1);
  assert.equal(b.window.dataLayer.filter(args => args[0] === "config").length, 1);
});

test("explicit consent decisions govern subsequently rendered properties", () => {
  const b = browser({ ready: "complete" });
  b.ga("G-FIRST123", "required");
  b.window.proaThirdParty.googleAnalytics.setConsent(false);
  b.ga("G-LATER123", "granted");
  assert.equal(b.scripts.length, 0);
  assert.equal(b.window.proaThirdParty.googleAnalytics.track("G-LATER123", "event"), false);
});

test("GA respects an application opt-out flag", () => {
  const b = browser({ ready: "complete" });
  b.window["ga-disable-G-TEST123456"] = true;
  b.ga();
  assert.equal(b.scripts.length, 0);
  assert.equal(b.window.proaThirdParty.googleAnalytics, undefined);
});

test("YouTube creates no player until activation and preserves focus and title", () => {
  const b = browser();
  let click, iframe;
  const button = {
    dataset: { src: "https://www.youtube-nocookie.com/embed/M7lc1UVf-VE?autoplay=1", title: "Video title" },
    addEventListener(name, fn, options) { assert.equal(name, "click"); assert.equal(options.once, true); click = fn; },
    replaceWith(value) { iframe = value; },
  };
  b.document.currentScript.parentElement = { querySelector: () => button };
  b.run("youtube");
  assert.equal(iframe, undefined);
  const firstClick = click;
  b.run("youtube");
  assert.equal(click, firstClick);
  click();
  assert.equal(iframe.kind, "iframe");
  assert.equal(iframe.title, "Video title");
  assert.equal(iframe.referrerPolicy, "strict-origin-when-cross-origin");
  assert.equal(iframe.allowFullscreen, true);
  assert.equal(iframe.focused, true);
  assert.equal(iframe.preventScroll, true);
});
