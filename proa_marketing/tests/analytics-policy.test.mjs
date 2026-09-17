// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import vm from "node:vm";

const policy = readFileSync(new URL("../src/analytics.js", import.meta.url), "utf8");
for (const hostname of ["proa.so", "www.proa.so", "localhost", "127.0.0.1", "preview.proa.so", "proa-pr.up.railway.app", "proa.so.evil.test"]) {
  for (const embedded of [false, true]) {
    test(`${hostname}, embedded=${embedded}`, () => {
      const window = {};
      window.self = window;
      window.top = embedded ? {} : window;
      vm.runInNewContext(policy, {
        window, location: { hostname },
        document: { currentScript: { dataset: { measurementId: "G-TEST123456" } } },
      });
      const allowed = !embedded && ["proa.so", "www.proa.so"].includes(hostname);
      assert.equal(window["ga-disable-G-TEST123456"], allowed ? undefined : true);
    });
  }
}
