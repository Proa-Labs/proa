// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

import { spawnSync } from "node:child_process";
import fs from "node:fs/promises";
import path from "node:path";
import { isDeepStrictEqual } from "node:util";
import { fileURLToPath } from "node:url";

import { importRuntime, importSnapshot, makeElement } from "./runtime/harness.mjs";

const here = path.dirname(fileURLToPath(import.meta.url));
const workspaceRoot = path.resolve(here, "..");
const corpusPath = path.join(here, "semantic-corpus.json");
const stringBundlePath =
  "rsjs_conformance/snapshots/client_helper/rust_string_semantics.bundle.js";

export async function runSemanticDifferentialCorpus(assert) {
  const corpus = JSON.parse(await fs.readFile(corpusPath, "utf8"));
  const rustOutput = evaluateWithRust();
  const rustResults = new Map(
    rustOutput.results.map((result) => [result.id, result.value]),
  );
  const helpers = await importSnapshot(
    stringBundlePath,
    "semantic-corpus-string-helpers.mjs",
  );
  const runtime = await importRuntime();

  let parityCount = 0;
  let divergenceCount = 0;

  for (const testCase of corpus.cases) {
    const rustValue = rustResults.get(testCase.id);
    assert(
      rustValue !== undefined,
      `Rust evaluator omitted semantic corpus case ${testCase.id}`,
    );
    if (rustValue === undefined) {
      continue;
    }

    const browserValue = evaluateInRuntime(testCase, rustValue, helpers, runtime);
    const expected = browserExpected(testCase, rustValue);
    if (testCase.expectation === "parity") {
      parityCount += 1;
      assert(
        isDeepStrictEqual(browserValue, expected),
        `${testCase.id} diverged: Rust=${JSON.stringify(expected)} browser/runtime=${JSON.stringify(browserValue)}`,
      );
    } else if (testCase.expectation === "known_divergence") {
      divergenceCount += 1;
      assert(
        !isDeepStrictEqual(browserValue, expected),
        `${testCase.id} now matches Rust and should be promoted from known_divergence to parity`,
      );
      assert(
        isDeepStrictEqual(browserValue, testCase.browser_characterization),
        `${testCase.id} changed outside its explicit characterization: expected browser/runtime=${JSON.stringify(testCase.browser_characterization)}, got ${JSON.stringify(browserValue)}`,
      );
    } else {
      assert(false, `${testCase.id} has unknown expectation ${testCase.expectation}`);
    }
  }

  assert(
    rustResults.size === corpus.cases.length,
    `Rust evaluator returned ${rustResults.size} results for ${corpus.cases.length} cases`,
  );
  assert(parityCount > 0, "semantic corpus must execute parity cases");
  assert(
    divergenceCount > 0,
    "semantic corpus must keep known divergences visibly separate from parity",
  );

  return {
    cases: corpus.cases.length,
    parity: parityCount,
    knownDivergences: divergenceCount,
    exclusions: corpus.exclusions.length,
  };
}

function evaluateWithRust() {
  const result = spawnSync(
    "cargo",
    ["run", "--quiet", "-p", "rsjs", "--example", "semantic_corpus"],
    {
      cwd: workspaceRoot,
      encoding: "utf8",
      env: {
        ...process.env,
        CARGO_TERM_COLOR: "never",
      },
    },
  );
  if (result.error) {
    throw result.error;
  }
  if (result.status !== 0) {
    throw new Error(
      `Rust semantic corpus evaluator failed with status ${result.status}\n${result.stderr}`,
    );
  }
  try {
    return JSON.parse(result.stdout);
  } catch (error) {
    throw new Error(
      `Rust semantic corpus evaluator emitted invalid JSON: ${error}\nstdout:\n${result.stdout}\nstderr:\n${result.stderr}`,
    );
  }
}

function browserExpected(testCase, rustValue) {
  return isWireOperation(testCase.operation) ? rustValue.semantic : rustValue;
}

function evaluateInRuntime(testCase, rustValue, helpers, runtime) {
  const input = testCase.input;
  switch (testCase.operation) {
    case "string_utf8_len":
      return new TextEncoder().encode(input.value).length;
    case "string_trim":
      return helpers.rust_string_semantics__trim(input.value);
    case "i64_increment": {
      const initial = runtime.signal_initial_wide([input.value], 0, 0n);
      return runtime.num_add(initial, BigInt(input.delta)).toString();
    }
    case "i64_div_rem": {
      const lhs = BigInt(input.lhs);
      const rhs = BigInt(input.rhs);
      return {
        quotient: runtime.checked_int_div(lhs, rhs, "i64").toString(),
        remainder: runtime.checked_int_rem(lhs, rhs, "i64").toString(),
      };
    }
    case "f64_display": {
      const node = makeElement("span");
      runtime.bind_text(node, () => f64Input(input.value), [], [], "rust-float");
      return node.textContent;
    }
    case "option_i32_wire":
    case "option_string_wire":
    case "fieldless_enum_wire":
    case "nested_account_wire": {
      assertWireRoundTrip(testCase.id, rustValue);
      const parsed = JSON.parse(rustValue.wire);
      const cell = runtime.signal(runtime.signal_initial([parsed], 0, "missing"));
      return cell.get();
    }
    default:
      throw new Error(
        `semantic corpus case ${testCase.id} has unknown operation ${testCase.operation}`,
      );
  }
}

function assertWireRoundTrip(id, rustValue) {
  if (
    typeof rustValue?.wire !== "string" ||
    rustValue.roundtrip_wire !== rustValue.wire
  ) {
    throw new Error(`${id} did not round-trip byte-identically in the Rust codec`);
  }
}

function isWireOperation(operation) {
  return (
    operation === "option_i32_wire" ||
    operation === "option_string_wire" ||
    operation === "fieldless_enum_wire" ||
    operation === "nested_account_wire"
  );
}

function f64Input(token) {
  switch (token) {
    case "sum_0_1_0_2":
      return 0.1 + 0.2;
    case "nan":
      return Number.NaN;
    case "positive_infinity":
      return Number.POSITIVE_INFINITY;
    case "negative_infinity":
      return Number.NEGATIVE_INFINITY;
    case "negative_zero":
      return -0;
    case "small_exponent":
      return 1e-7;
    default:
      throw new Error(`unknown f64 display token ${token}`);
  }
}
