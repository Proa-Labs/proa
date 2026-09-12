#!/usr/bin/env node
// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

import { assert, finish, importSnapshot } from "./harness.mjs";

const mod = await importSnapshot(
  "rsjs_conformance/snapshots/client_helper/control_flow_early_return.bundle.js",
  "client-helper-early-return.mjs",
);

assert(
  mod.pricing_labels__label(101) === "premium",
  `price above threshold should be premium, got ${JSON.stringify(mod.pricing_labels__label(101))}`,
);
assert(
  mod.pricing_labels__label(100) === "standard",
  `price at threshold should be standard, got ${JSON.stringify(mod.pricing_labels__label(100))}`,
);

finish("rsjs conformance runtime client-helper early return");
