#!/usr/bin/env node
// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

import { runSemanticDifferentialCorpus } from "../semantic-corpus.mjs";
import { assert, finish } from "./harness.mjs";

const summary = await runSemanticDifferentialCorpus(assert);

assert(
  summary.parity >= 20,
  `expected at least 20 Rust/browser parity cases, got ${summary.parity}`,
);
assert(
  summary.knownDivergences >= 2,
  `expected explicit known-divergence coverage, got ${summary.knownDivergences}`,
);

finish("rsjs conformance runtime Rust/browser semantic differential corpus");
