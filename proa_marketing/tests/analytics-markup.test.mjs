// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import vm from "node:vm";

// The Rust test passes actual component output, so these checks exercise the
// application's policy and shared provider in their rendered order.
const html = readFileSync(0, "utf8");
const tags = [...html.matchAll(/<script\b([^>]*)>([\s\S]*?)<\/script>/g)];
const id = "G-7WSVNZVD6V";

test("markup carries one policy and one configured provider", () => {
  assert.equal(tags.length, 2);
  assert.match(tags[0][1], /data-proa-analytics-policy/);
  assert.match(tags[1][1], /data-proa-google-analytics/);
  for (const [, attributes] of tags) {
    assert.match(attributes, /data-measurement-id="G-7WSVNZVD6V"/);
    assert.match(attributes, /nonce="YWJjZA=="/);
  }
  assert.match(tags[1][1], /data-consent="granted"/);
  assert.match(tags[1][1], /data-page-view="true"/);
  assert.match(tags[1][1], /data-strategy="after-load"/);
});

function browser(hostname, embedded) {
  const scripts = [], loads = [], errors = [];
  const document = {
    readyState: "loading", baseURI: `https://${hostname}/`,
    head: { appendChild: script => scripts.push(script) },
    createElement: kind => ({ kind, addEventListener() {}, remove() {} }),
  };
  const window = {
    addEventListener(name, fn) {
      assert.equal(name, "load");
      loads.push(fn);
    },
    dispatchEvent: event => errors.push(event.detail),
  };
  window.self = window;
  window.top = embedded ? {} : window;
  const context = vm.createContext({ window, document, location: { hostname }, URL,
    CustomEvent: class { constructor(type, options) { this.type = type; this.detail = options.detail; } },
  });
  for (const [, attributes, source] of tags) {
    const dataset = {};
    for (const [, name, value] of attributes.matchAll(/data-([a-z-]+)="([^"]*)"/g)) {
      dataset[name.replace(/-([a-z])/g, (_, letter) => letter.toUpperCase())] = value;
    }
    document.currentScript = { dataset, nonce: attributes.match(/nonce="([^"]*)"/)?.[1] };
    vm.runInContext(source, context);
  }
  return { window, scripts, errors, load() {
    document.readyState = "complete";
    loads.forEach(fn => fn());
  } };
}

for (const hostname of ["proa.so", "www.proa.so"]) {
  test(`${hostname} queues config once and loads the correct async SDK after load`, () => {
    const b = browser(hostname, false);
    assert.equal(typeof b.window.gtag, "function");
    assert.equal(b.window.dataLayer.filter(args => args[0] === "js").length, 1);
    const configs = b.window.dataLayer.filter(args => args[0] === "config");
    assert.equal(configs.length, 1);
    assert.equal(configs[0][1], id);
    assert.equal(configs[0][2].send_page_view, true);
    assert.equal(configs[0][2].allow_google_signals, false);
    assert.equal(configs[0][2].allow_ad_personalization_signals, false);
    assert.equal(b.scripts.length, 0);
    b.load();
    assert.equal(b.scripts.length, 1);
    assert.equal(b.scripts[0].src, `https://www.googletagmanager.com/gtag/js?id=${id}`);
    assert.equal(b.scripts[0].async, true);
    assert.equal(b.scripts[0].nonce, "YWJjZA==");
    assert.equal(b.errors.length, 0);
  });
}

for (const [hostname, embedded] of [["localhost", false], ["proa-pr.up.railway.app", false], ["proa.so", true]]) {
  test(`${hostname}, embedded=${embedded}, does not configure or download GA`, () => {
    const b = browser(hostname, embedded);
    b.load();
    assert.equal(b.window[`ga-disable-${id}`], true);
    assert.equal(b.window.gtag, undefined);
    assert.equal(b.scripts.length, 0);
    assert.equal(b.errors.length, 0);
  });
}
