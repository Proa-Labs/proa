#!/usr/bin/env node
// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

import fs from "node:fs";
import path from "node:path";
import { execFileSync } from "node:child_process";
import { fileURLToPath } from "node:url";

const here = path.dirname(fileURLToPath(import.meta.url));
const manifestPath = path.join(here, "features.json");
const workspaceRoot = path.resolve(here, "..");
const manifest = JSON.parse(fs.readFileSync(manifestPath, "utf8"));
const features = manifest.features ?? [];
const check = process.argv.includes("--check");
// Keep this independent from the generated snapshots: a Client IR schema bump
// must deliberately advance the repository-level conformance contract too.
const clientIrSchemaVersion = 3;

const statusOrder = [
  "supported",
  "partial",
  "expected_fail",
  "planned",
  "out_of_scope",
  "unknown",
];
const coverageKeys = [
  "compile_pass",
  "compile_fail",
  "hir_snapshot",
  "ir_snapshot",
  "js_snapshot",
  "descriptor_snapshot",
  "runtime",
];
const snapshotKeys = [
  "hir_snapshot",
  "ir_snapshot",
  "js_snapshot",
  "descriptor_snapshot",
];
const coverageKeySet = new Set(coverageKeys);
const snapshotKeySet = new Set(snapshotKeys);
const fixtureGateKeys = [
  "pass",
  "fail",
  "case_ids",
  "compile_fail_stderr",
];
const fixtureGateKeySet = new Set(fixtureGateKeys);
const statusSet = new Set(statusOrder);
const semanticContractSet = new Set(["parity", "characterization", "unsupported"]);
const fixtureCaseIdPattern = /^[a-z0-9]+(?:[._-][a-z0-9]+)*$/;
const runtimeHarnessPath = "rsjs_conformance/runtime/harness.mjs";

function countBy(items, keyFn) {
  const out = new Map();
  for (const item of items) {
    const key = keyFn(item);
    out.set(key, (out.get(key) ?? 0) + 1);
  }
  return out;
}

function pad(label, width = 24) {
  return `${label}:`.padEnd(width, " ");
}

function validateManifest() {
  const errors = [];
  const manifestFixtures = new Set();
  const fixtureCaseIds = new Map();
  if (manifest.schema_version !== 1) {
    errors.push("manifest must declare schema_version = 1");
  }

  const coverageMin = manifest.gates?.coverage_min ?? {};
  if (!coverageMin || typeof coverageMin !== "object" || Array.isArray(coverageMin)) {
    errors.push("manifest gates.coverage_min must be an object when present");
  } else {
    for (const [key, min] of Object.entries(coverageMin)) {
      if (!coverageKeySet.has(key)) {
        errors.push(`unknown coverage gate ${key}`);
        continue;
      }
      if (!Number.isInteger(min) || min < 0) {
        errors.push(`coverage gate ${key} must be a non-negative integer`);
        continue;
      }
      const actual = coverageCounts.get(key) ?? 0;
      if (actual < min) {
        errors.push(`coverage.${key} regressed below gate: ${actual} < ${min}`);
      }
    }
  }

  const fixtureMin = manifest.gates?.fixture_min ?? {};
  if (!fixtureMin || typeof fixtureMin !== "object" || Array.isArray(fixtureMin)) {
    errors.push("manifest gates.fixture_min must be an object when present");
  } else {
    for (const [key, min] of Object.entries(fixtureMin)) {
      if (!fixtureGateKeySet.has(key)) {
        errors.push(`unknown fixture gate ${key}`);
        continue;
      }
      if (!Number.isInteger(min) || min < 0) {
        errors.push(`fixture gate ${key} must be a non-negative integer`);
        continue;
      }
      const actual = fixtureGateCounts.get(key) ?? 0;
      if (actual < min) {
        errors.push(`fixtures.${key} regressed below gate: ${actual} < ${min}`);
      }
    }
  }

  const ids = new Set();
  const snapshotOwners = new Map();
  const runtimeScriptOwners = new Map();
  for (const feature of features) {
    const id = typeof feature.id === "string" ? feature.id : "<missing id>";
    if (ids.has(id)) {
      errors.push(`duplicate feature id ${id}`);
    }
    ids.add(id);

    if (!statusSet.has(feature.status)) {
      errors.push(`${id}: unknown status ${JSON.stringify(feature.status)}`);
    }
    if (feature.category === "semantics") {
      if (!semanticContractSet.has(feature.semantic_contract)) {
        errors.push(
          `${id}: semantic feature needs semantic_contract = parity, characterization, or unsupported`,
        );
      }
      if (feature.status === "supported" && feature.semantic_contract !== "parity") {
        errors.push(`${id}: supported semantic feature must be a parity guarantee`);
      }
      if (
        feature.semantic_contract === "characterization" &&
        feature.status === "supported"
      ) {
        errors.push(`${id}: characterization must not claim supported parity`);
      }
    }
    if (feature.differential_corpus !== undefined) {
      validateDifferentialCorpus(id, feature.differential_corpus, errors);
    }

    const coverage = feature.coverage;
    if (!coverage || typeof coverage !== "object" || Array.isArray(coverage)) {
      errors.push(`${id}: coverage must be an object`);
      continue;
    }

    for (const key of Object.keys(coverage)) {
      if (!coverageKeySet.has(key)) {
        errors.push(`${id}: unknown coverage key ${key}`);
      }
    }

    if (feature.status === "supported" && coverage.compile_pass !== true) {
      errors.push(`${id}: supported features must have coverage.compile_pass = true`);
    }
    if (feature.status === "expected_fail" && coverage.compile_fail !== true) {
      errors.push(`${id}: expected_fail features must have coverage.compile_fail = true`);
    }
    if (coverage.compile_pass === true) {
      for (const key of ["hir_snapshot", "ir_snapshot", "js_snapshot", "runtime"]) {
        if (coverage[key] !== true) {
          errors.push(`${id}: compile_pass features must have coverage.${key} = true`);
        }
      }
    }
    if (
      coverage.compile_pass === true &&
      descriptorSnapshotRequired(feature) &&
      coverage.descriptor_snapshot !== true
    ) {
      errors.push(`${id}: descriptor-relevant source positions require coverage.descriptor_snapshot = true`);
    }
    if (coverage.runtime === true && coverage.compile_pass !== true) {
      errors.push(`${id}: runtime coverage requires coverage.compile_pass = true`);
    }

    const runtimeScript = feature.runtime_script;
    if (coverage.runtime === true) {
      if (typeof runtimeScript !== "string") {
        errors.push(`${id}: coverage.runtime=true requires runtime_script`);
      } else {
        const owner = runtimeScriptOwners.get(runtimeScript);
        if (owner !== undefined) {
          errors.push(`${id}: runtime_script ${runtimeScript} is already referenced by ${owner}`);
        } else {
          runtimeScriptOwners.set(runtimeScript, id);
        }
        if (
          !runtimeScript.startsWith("rsjs_conformance/runtime/") ||
          !runtimeScript.endsWith(".mjs")
        ) {
          errors.push(`${id}: runtime_script must point at an rsjs_conformance/runtime/*.mjs file`);
        }
        if (!fs.existsSync(path.join(workspaceRoot, runtimeScript))) {
          errors.push(`${id}: missing runtime script ${runtimeScript}`);
        } else {
          validateJavaScriptSyntax(
            id,
            "runtime_script",
            runtimeScript,
            path.join(workspaceRoot, runtimeScript),
            errors,
          );
        }
      }
    } else if (runtimeScript !== undefined) {
      errors.push(`${id}: runtime_script is present but coverage.runtime is not true`);
    }

    const snapshots = feature.snapshots;
    if (snapshots !== undefined && (!snapshots || typeof snapshots !== "object" || Array.isArray(snapshots))) {
      errors.push(`${id}: snapshots must be an object when present`);
      continue;
    }

    for (const key of Object.keys(snapshots ?? {})) {
      if (!snapshotKeySet.has(key)) {
        errors.push(`${id}: unknown snapshot key ${key}`);
      }
    }

    for (const key of snapshotKeys) {
      const covered = coverage[key] === true;
      const snapshotPath = snapshots?.[key];
      if (typeof snapshotPath === "string") {
        const owner = snapshotOwners.get(snapshotPath);
        if (owner !== undefined) {
          errors.push(`${id}: snapshot ${snapshotPath} is already referenced by ${owner}`);
        } else {
          snapshotOwners.set(snapshotPath, id);
        }
      }
      if (covered) {
        if (typeof snapshotPath !== "string") {
          errors.push(`${id}: coverage.${key}=true requires snapshots.${key}`);
          continue;
        }
        const absoluteSnapshotPath = path.join(workspaceRoot, snapshotPath);
        if (!fs.existsSync(absoluteSnapshotPath)) {
          errors.push(`${id}: missing snapshot file ${snapshotPath}`);
        } else {
          validateSnapshotFile(id, key, snapshotPath, absoluteSnapshotPath, errors);
        }
      } else if (snapshotPath !== undefined) {
        errors.push(`${id}: snapshots.${key} is present but coverage.${key} is not true`);
      }
    }

    const supportingSnapshots = feature.supporting_snapshots;
    if (
      supportingSnapshots !== undefined &&
      (!Array.isArray(supportingSnapshots) ||
        supportingSnapshots.some((snapshotPath) => typeof snapshotPath !== "string"))
    ) {
      errors.push(`${id}: supporting_snapshots must be an array of string paths when present`);
    } else {
      for (const snapshotPath of supportingSnapshots ?? []) {
        const owner = snapshotOwners.get(snapshotPath);
        if (owner !== undefined) {
          errors.push(`${id}: supporting snapshot ${snapshotPath} is already referenced by ${owner}`);
          continue;
        }
        snapshotOwners.set(snapshotPath, `${id} supporting_snapshots`);
        const key = snapshotKindForPath(snapshotPath);
        if (key === undefined) {
          errors.push(`${id}: supporting snapshot ${snapshotPath} has an unknown file shape`);
          continue;
        }
        const absoluteSnapshotPath = path.join(workspaceRoot, snapshotPath);
        if (!fs.existsSync(absoluteSnapshotPath)) {
          errors.push(`${id}: missing supporting snapshot file ${snapshotPath}`);
        } else {
          validateSnapshotFile(id, key, snapshotPath, absoluteSnapshotPath, errors);
        }
      }
    }

    const fixtures = feature.fixtures;
    if (!Array.isArray(fixtures) || fixtures.length === 0) {
      errors.push(`${id}: fixtures must be a non-empty array`);
      continue;
    }
    for (const fixture of fixtures) {
      const caseId = fixture?.case_id;
      const fixturePath = fixture?.path;
      const expect = fixture?.expect;
      if (typeof caseId !== "string") {
        errors.push(`${id}: fixture ${fixturePath ?? "<missing path>"} must declare string case_id`);
      } else if (!fixtureCaseIdPattern.test(caseId)) {
        errors.push(`${id}: fixture ${fixturePath ?? "<missing path>"} has invalid case_id ${JSON.stringify(caseId)}`);
      } else {
        const previous = fixtureCaseIds.get(caseId);
        if (previous !== undefined) {
          errors.push(`${id}: fixture case_id ${caseId} is already used by ${previous}`);
        } else {
          fixtureCaseIds.set(caseId, fixturePath ?? "<missing path>");
        }
      }
      if (typeof fixturePath !== "string") {
        errors.push(`${id}: fixture path must be a string`);
        continue;
      }
      if (expect !== "pass" && expect !== "fail") {
        errors.push(`${id}: fixture ${fixturePath} has invalid expect ${JSON.stringify(expect)}`);
        continue;
      }
      if (manifestFixtures.has(fixturePath)) {
        errors.push(`${id}: duplicate fixture reference ${fixturePath}`);
      } else {
        manifestFixtures.add(fixturePath);
      }
      const absoluteFixturePath = path.join(workspaceRoot, fixturePath);
      if (!fs.existsSync(absoluteFixturePath)) {
        errors.push(`${id}: missing fixture ${fixturePath}`);
      }
      const coverageKey = expect === "pass" ? "compile_pass" : "compile_fail";
      if (coverage[coverageKey] !== true) {
        errors.push(`${id}: fixture ${fixturePath} expects ${expect} but coverage.${coverageKey} is not true`);
      }
      if (expect === "fail" && !fs.existsSync(stderrPathFor(absoluteFixturePath))) {
        errors.push(`${id}: compile-fail fixture ${fixturePath} is missing its .stderr snapshot`);
      }
    }
  }

  const expectedFixtures = new Set([
    ...collectFixturePaths(path.join(workspaceRoot, "rsjs_macros/tests/ui/conformance/pass")),
    ...collectFixturePaths(path.join(workspaceRoot, "rsjs_macros/tests/ui/conformance/fail")),
  ]);
  for (const expected of expectedFixtures) {
    if (!manifestFixtures.has(expected)) {
      errors.push(`manifest does not reference conformance fixture ${expected}`);
    }
  }
  for (const actual of manifestFixtures) {
    if (!expectedFixtures.has(actual)) {
      errors.push(`manifest references non-conformance fixture ${actual}`);
    }
  }

  const expectedRuntimeScripts = new Set(
    collectRuntimeScriptPaths(path.join(workspaceRoot, "rsjs_conformance/runtime")),
  );
  for (const expected of expectedRuntimeScripts) {
    if (!runtimeScriptOwners.has(expected)) {
      errors.push(`manifest does not reference runtime script ${expected}`);
    }
  }
  for (const actual of runtimeScriptOwners.keys()) {
    if (!expectedRuntimeScripts.has(actual)) {
      errors.push(`manifest references non-runtime conformance script ${actual}`);
    }
  }

  const expectedSnapshotFiles = new Set(
    collectSnapshotFilePaths(path.join(workspaceRoot, "rsjs_conformance/snapshots")),
  );
  for (const expected of expectedSnapshotFiles) {
    if (!snapshotOwners.has(expected)) {
      errors.push(`manifest does not reference conformance snapshot ${expected}`);
    }
  }
  for (const actual of snapshotOwners.keys()) {
    if (!expectedSnapshotFiles.has(actual)) {
      errors.push(`manifest references non-conformance snapshot ${actual}`);
    }
  }

  return errors;
}

function validateDifferentialCorpus(id, corpusPath, errors) {
  if (
    typeof corpusPath !== "string" ||
    !corpusPath.startsWith("rsjs_conformance/") ||
    !corpusPath.endsWith(".json")
  ) {
    errors.push(`${id}: differential_corpus must be an rsjs_conformance JSON path`);
    return;
  }
  const absolutePath = path.join(workspaceRoot, corpusPath);
  if (!fs.existsSync(absolutePath)) {
    errors.push(`${id}: missing differential corpus ${corpusPath}`);
    return;
  }

  let corpus;
  try {
    corpus = JSON.parse(fs.readFileSync(absolutePath, "utf8"));
  } catch (err) {
    errors.push(`${id}: differential corpus ${corpusPath} must be valid JSON: ${err.message}`);
    return;
  }
  if (corpus.schema_version !== 1) {
    errors.push(`${id}: differential corpus must declare schema_version = 1`);
  }
  if (!Array.isArray(corpus.cases) || corpus.cases.length === 0) {
    errors.push(`${id}: differential corpus needs a non-empty cases array`);
  }
  if (!Array.isArray(corpus.exclusions) || corpus.exclusions.length === 0) {
    errors.push(`${id}: differential corpus needs a non-empty exclusions array`);
  }

  const caseIds = new Set();
  for (const testCase of corpus.cases ?? []) {
    const caseId = testCase?.id;
    if (typeof caseId !== "string" || !fixtureCaseIdPattern.test(caseId)) {
      errors.push(`${id}: differential case has invalid id ${JSON.stringify(caseId)}`);
    } else if (caseIds.has(caseId)) {
      errors.push(`${id}: duplicate differential case id ${caseId}`);
    } else {
      caseIds.add(caseId);
    }
    if (!["parity", "known_divergence"].includes(testCase?.expectation)) {
      errors.push(
        `${id}: differential case ${caseId ?? "<missing>"} has unknown expectation ${JSON.stringify(testCase?.expectation)}`,
      );
    }
    if (testCase?.expectation === "known_divergence") {
      if (typeof testCase.reason !== "string" || testCase.reason.length === 0) {
        errors.push(`${id}: known divergence ${caseId ?? "<missing>"} needs a reason`);
      }
      if (!Object.prototype.hasOwnProperty.call(testCase, "browser_characterization")) {
        errors.push(
          `${id}: known divergence ${caseId ?? "<missing>"} needs an exact browser_characterization`,
        );
      }
    }
  }
}

function snapshotKindForPath(snapshotPath) {
  if (snapshotPath.endsWith(".descriptor.json")) {
    return "descriptor_snapshot";
  }
  if (snapshotPath.endsWith(".body-ir.json") || snapshotPath.endsWith(".module-ir.json")) {
    return "ir_snapshot";
  }
  if (snapshotPath.endsWith(".hir.json") || snapshotPath.endsWith(".body-hir.json")) {
    return "hir_snapshot";
  }
  if (snapshotPath.endsWith(".js")) {
    return "js_snapshot";
  }
  return undefined;
}

function descriptorSnapshotRequired(feature) {
  const positions = Array.isArray(feature.source_positions) ? feature.source_positions : [];
  return positions.some(
    (position) => position === "component_prop" || position === "template_loop",
  );
}

function validateSnapshotFile(id, key, snapshotPath, absoluteSnapshotPath, errors) {
  if (path.posix.normalize(snapshotPath) !== snapshotPath || snapshotPath.startsWith("../")) {
    errors.push(`${id}: snapshots.${key} must be a normalized relative path`);
  }
  if (!snapshotPath.startsWith("rsjs_conformance/snapshots/")) {
    errors.push(`${id}: snapshots.${key} must live under rsjs_conformance/snapshots/`);
  }

  if (key === "js_snapshot") {
    if (!snapshotPath.endsWith(".js")) {
      errors.push(`${id}: snapshots.${key} must point at a .js file`);
    }
    validateJavaScriptSyntax(
      id,
      `snapshots.${key}`,
      snapshotPath,
      absoluteSnapshotPath,
      errors,
    );
    return;
  }

  let snapshotJson;
  try {
    snapshotJson = JSON.parse(fs.readFileSync(absoluteSnapshotPath, "utf8"));
  } catch (err) {
    errors.push(`${id}: snapshots.${key} must be valid JSON: ${err.message}`);
    return;
  }

  if (key === "hir_snapshot") {
    if (!snapshotPath.endsWith(".hir.json") && !snapshotPath.endsWith(".body-hir.json")) {
      errors.push(`${id}: snapshots.${key} must end with .hir.json or .body-hir.json`);
    }
    if (!containsSourceSpan(snapshotJson)) {
      errors.push(`${id}: snapshots.${key} ${snapshotPath} must include at least one source span`);
    }
    if (snapshotPath.endsWith(".body-hir.json")) {
      if (!containsExpressionSourceSpan(snapshotJson, isHelperExpressionKind)) {
        errors.push(`${id}: snapshots.${key} ${snapshotPath} must include at least one expression source span`);
      }
    } else if (snapshotPath.endsWith(".hir.json")) {
      if (!containsExpressionSourceSpan(snapshotJson, isHandlerExpressionKind)) {
        errors.push(`${id}: snapshots.${key} ${snapshotPath} must include at least one expression source span`);
      }
      if (
        requiresEffectOperandSpanGranularity(id, snapshotPath) &&
        !containsEffectOperandSpanDifferentFromStatementSpan(snapshotJson)
      ) {
        errors.push(
          `${id}: snapshots.${key} ${snapshotPath} must preserve browser/node-ref effect operand spans instead of only statement spans`,
        );
      }
    }
    return;
  }

  if (key === "ir_snapshot") {
    if (!snapshotPath.endsWith(".body-ir.json") && !snapshotPath.endsWith(".module-ir.json")) {
      errors.push(`${id}: snapshots.${key} must end with .body-ir.json or .module-ir.json`);
    }
    if (snapshotPath.endsWith(".body-ir.json")) {
      validateClientBlockIrSnapshot(
        id,
        snapshotJson,
        errors,
        `snapshots.${key} ${snapshotPath}`,
      );
    }
    if (snapshotPath.endsWith(".module-ir.json")) {
      validateClientModuleIrSnapshot(id, snapshotJson, errors);
    }
    return;
  }

  if (key === "descriptor_snapshot") {
    if (
      !snapshotPath.startsWith("rsjs_conformance/snapshots/descriptor/") ||
      !snapshotPath.endsWith(".descriptor.json")
    ) {
      errors.push(`${id}: snapshots.${key} must point at rsjs_conformance/snapshots/descriptor/*.descriptor.json`);
    }
    validateDescriptorSnapshot(id, snapshotJson, errors);
  }
}

function containsSourceSpan(value) {
  if (Array.isArray(value)) {
    return value.some(containsSourceSpan);
  }
  if (!value || typeof value !== "object") {
    return false;
  }
  if (isSourceSpan(value.span)) {
    return true;
  }
  return Object.values(value).some(containsSourceSpan);
}

function isSourceSpan(value) {
  if (!value || typeof value !== "object" || Array.isArray(value)) {
    return false;
  }
  return isSourcePosition(value.start) && isSourcePosition(value.end);
}

function containsExpressionSourceSpan(value, isExpressionKind) {
  if (Array.isArray(value)) {
    return value.some((entry) => containsExpressionSourceSpan(entry, isExpressionKind));
  }
  if (!value || typeof value !== "object") {
    return false;
  }
  const current = isExpressionKind(value.kind) && isSourceSpan(value.span);
  return current || Object.values(value).some((entry) => containsExpressionSourceSpan(entry, isExpressionKind));
}

function isHelperExpressionKind(kind) {
  return [
    "Lit",
    "Param",
    "Local",
    "BinaryOp",
    "UnaryOp",
    "Conditional",
    "IsNonEmpty",
    "Len",
    "StringToLowercase",
    "StringContains",
    "ClientCall",
  ].includes(kind);
}

function isHandlerExpressionKind(kind) {
  return [
    "literal",
    "array_lit",
    "signal_read",
    "local",
    "event_value",
    "event_attribute",
    "event_payload",
    "node_ref_read",
    "browser_read",
    "encoding",
    "binary",
    "debug",
  ].includes(kind);
}

function requiresEffectOperandSpanGranularity(id, snapshotPath) {
  return (
    (id === "browser.effects.p0" &&
      snapshotPath.endsWith("browser_effects_p0.hir.json")) ||
    (id === "node.refs.p0" && snapshotPath.endsWith("node_refs_p0.hir.json"))
  );
}

function containsEffectOperandSpanDifferentFromStatementSpan(value) {
  if (Array.isArray(value)) {
    return value.some(containsEffectOperandSpanDifferentFromStatementSpan);
  }
  if (!value || typeof value !== "object") {
    return false;
  }
  if (
    (value.kind === "browser_effect" || value.kind === "node_ref_effect") &&
    isSourceSpan(value.span) &&
    containsExpressionSpanDifferentFrom(value.effect, value.span)
  ) {
    return true;
  }
  return Object.values(value).some(containsEffectOperandSpanDifferentFromStatementSpan);
}

function containsExpressionSpanDifferentFrom(value, statementSpan) {
  if (Array.isArray(value)) {
    return value.some((entry) => containsExpressionSpanDifferentFrom(entry, statementSpan));
  }
  if (!value || typeof value !== "object") {
    return false;
  }
  const current =
    isHandlerExpressionKind(value.kind) &&
    isSourceSpan(value.span) &&
    !sourceSpansEqual(value.span, statementSpan);
  return current || Object.values(value).some((entry) => containsExpressionSpanDifferentFrom(entry, statementSpan));
}

function sourceSpansEqual(left, right) {
  return (
    left.start.line === right.start.line &&
    left.start.column === right.start.column &&
    left.end.line === right.end.line &&
    left.end.column === right.end.column
  );
}

function isSourcePosition(value) {
  return (
    value &&
    typeof value === "object" &&
    !Array.isArray(value) &&
    Number.isInteger(value.line) &&
    value.line >= 1 &&
    Number.isInteger(value.column) &&
    value.column >= 0
  );
}

function validateJavaScriptSyntax(id, label, relativePath, absolutePath, errors) {
  try {
    execFileSync(process.execPath, ["--check", absolutePath], {
      encoding: "utf8",
      stdio: "pipe",
    });
  } catch (err) {
    const output = [err.stdout, err.stderr, err.message]
      .filter(Boolean)
      .join("\n")
      .split("\n")
      .find((line) => line.trim().length > 0);
    errors.push(
      `${id}: ${label} ${relativePath} must pass node --check` +
        (output ? `: ${output.trim()}` : ""),
    );
  }
}

function validateClientModuleIrSnapshot(id, value, errors) {
  if (!value || typeof value !== "object" || Array.isArray(value)) {
    errors.push(`${id}: module IR snapshot must be a JSON object`);
    return;
  }
  if (value.format !== "rsjs.client_module_ir") {
    errors.push(`${id}: module IR snapshot must declare format = rsjs.client_module_ir`);
  }
  if (value.client_ir_version !== clientIrSchemaVersion) {
    errors.push(
      `${id}: module IR snapshot must declare client_ir_version = ${clientIrSchemaVersion}`,
    );
  }
  if (!Array.isArray(value.functions)) {
    errors.push(`${id}: module IR snapshot must include functions array`);
    return;
  }
  if (!Array.isArray(value.exports)) {
    errors.push(`${id}: module IR snapshot must include exports array`);
    return;
  }

  const functions = new Map();
  for (const fn of value.functions) {
    if (!fn || typeof fn !== "object" || Array.isArray(fn)) {
      errors.push(`${id}: module IR functions must be objects`);
      continue;
    }
    if (!Number.isInteger(fn.id) || fn.id < 0) {
      errors.push(`${id}: module IR function id must be a non-negative integer`);
      continue;
    }
    if (functions.has(fn.id)) {
      errors.push(`${id}: module IR function id ${fn.id} is duplicated`);
      continue;
    }
    functions.set(fn.id, fn);
  }

  const roots = [];
  for (const exp of value.exports) {
    if (!exp || typeof exp !== "object" || Array.isArray(exp)) {
      errors.push(`${id}: module IR exports must be objects`);
      continue;
    }
    if (!Number.isInteger(exp.function) || exp.function < 0) {
      errors.push(`${id}: module IR export function must be a non-negative integer`);
      continue;
    }
    if (!functions.has(exp.function)) {
      errors.push(`${id}: module IR export references unknown function ${exp.function}`);
      continue;
    }
    roots.push(exp.function);
  }

  const reachable = new Set();
  const stack = [...roots];
  while (stack.length > 0) {
    const functionId = stack.pop();
    if (reachable.has(functionId)) {
      continue;
    }
    reachable.add(functionId);
    const fn = functions.get(functionId);
    validateClientBlockIrSnapshot(
      id,
      fn?.body,
      errors,
      `module IR function ${functionId} body`,
    );
    for (const target of collectLocalFunctionRefs(fn?.body)) {
      if (!functions.has(target)) {
        errors.push(`${id}: module IR function ${functionId} calls unknown local function ${target}`);
        continue;
      }
      stack.push(target);
    }
  }

  for (const functionId of functions.keys()) {
    if (!reachable.has(functionId)) {
      errors.push(`${id}: module IR function ${functionId} is unreachable from exports`);
    }
  }
}

function validateClientBlockIrSnapshot(id, value, errors, label) {
  validateClientBlock(id, value, errors, label);
}

function validateClientBlock(id, value, errors, label) {
  if (!value || typeof value !== "object" || Array.isArray(value)) {
    errors.push(`${id}: ${label} must be a Client IR block object`);
    return false;
  }
  if (value.kind !== "block") {
    errors.push(`${id}: ${label} must have kind = block`);
    return false;
  }
  if (!Array.isArray(value.stmts)) {
    errors.push(`${id}: ${label} must include stmts array`);
    return false;
  }
  for (const [index, stmt] of value.stmts.entries()) {
    validateClientStmt(id, stmt, errors, `${label}.stmts[${index}]`);
  }
  return true;
}

function validateClientStmt(id, value, errors, label) {
  if (!value || typeof value !== "object" || Array.isArray(value)) {
    errors.push(`${id}: ${label} must be a Client IR statement object`);
    return;
  }
  if (typeof value.kind !== "string") {
    errors.push(`${id}: ${label} must include string kind`);
    return;
  }
  validateClientNumericRefs(id, value, errors, label);
  switch (value.kind) {
    case "block":
      validateClientBlock(id, value, errors, label);
      break;
    case "let":
      if (!Number.isInteger(value.local) || value.local < 0) {
        errors.push(`${id}: ${label}.local must be a non-negative integer`);
      }
      if (typeof value.name !== "string" || value.name.length === 0) {
        errors.push(`${id}: ${label}.name must be a non-empty string`);
      }
      validateClientExpr(id, value.value, errors, `${label}.value`);
      break;
    case "if":
      validateClientExpr(id, value.condition, errors, `${label}.condition`);
      validateClientBlock(id, value.then, errors, `${label}.then`);
      if (value.else !== null && value.else !== undefined) {
        validateClientBlock(id, value.else, errors, `${label}.else`);
      }
      break;
    case "signal_set":
      validateClientExpr(id, value.value, errors, `${label}.value`);
      break;
    case "signal_update":
      if (!value.closure || typeof value.closure !== "object" || Array.isArray(value.closure)) {
        errors.push(`${id}: ${label}.closure must be a Client IR closure object`);
      } else {
        if (typeof value.closure.param !== "string" || value.closure.param.length === 0) {
          errors.push(`${id}: ${label}.closure.param must be a non-empty string`);
        }
        validateClientExpr(id, value.closure.body, errors, `${label}.closure.body`);
      }
      break;
    case "callback_invoke":
      validateClientExpr(id, value.payload, errors, `${label}.payload`);
      break;
    case "query_refetch":
    case "query_invalidate":
      break;
    case "run_mutation":
    case "console":
      if (!Array.isArray(value.args)) {
        errors.push(`${id}: ${label}.args must be an array`);
      } else {
        value.args.forEach((arg, index) =>
          validateClientExpr(id, arg, errors, `${label}.args[${index}]`),
        );
      }
      break;
    case "browser_effect":
    case "node_ref_effect":
      validateClientExpr(id, value.effect, errors, `${label}.effect`);
      break;
    case "return":
      if (value.value !== null && value.value !== undefined) {
        validateClientExpr(id, value.value, errors, `${label}.value`);
      }
      break;
    default:
      errors.push(`${id}: ${label} has unknown Client IR statement kind ${value.kind}`);
      break;
  }
}

function validateClientExpr(id, value, errors, label) {
  if (!value || typeof value !== "object" || Array.isArray(value)) {
    errors.push(`${id}: ${label} must be a Client IR expression object`);
    return;
  }
  if (typeof value.kind !== "string") {
    errors.push(`${id}: ${label} must include string kind`);
    return;
  }
  validateClientNumericRefs(id, value, errors, label);
  for (const [key, child] of Object.entries(value)) {
    if (key === "ty" || key === "value" || key === "kind") {
      continue;
    }
    if (Array.isArray(child)) {
      child.forEach((entry, index) => {
        if (entry && typeof entry === "object") {
          validateClientExpr(id, entry, errors, `${label}.${key}[${index}]`);
        }
      });
    } else if (child && typeof child === "object" && typeof child.kind === "string") {
      validateClientExpr(id, child, errors, `${label}.${key}`);
    }
  }
}

function validateClientNumericRefs(id, value, errors, label) {
  for (const key of ["local", "signal", "callback", "query", "mutation", "node_ref"]) {
    if (value[key] !== undefined && (!Number.isInteger(value[key]) || value[key] < 0)) {
      errors.push(`${id}: ${label}.${key} must be a non-negative integer`);
    }
  }
}

function collectLocalFunctionRefs(value) {
  const out = [];
  collectLocalFunctionRefsInto(value, out);
  return out;
}

function collectLocalFunctionRefsInto(value, out) {
  if (Array.isArray(value)) {
    for (const item of value) {
      collectLocalFunctionRefsInto(item, out);
    }
    return;
  }
  if (!value || typeof value !== "object") {
    return;
  }
  if (
    value.kind === "call" &&
    value.function &&
    value.function.kind === "local" &&
    Number.isInteger(value.function.function)
  ) {
    out.push(value.function.function);
  }
  for (const item of Object.values(value)) {
    collectLocalFunctionRefsInto(item, out);
  }
}

function validateDescriptorSnapshot(id, value, errors) {
  if (!value || typeof value !== "object" || Array.isArray(value)) {
    errors.push(`${id}: descriptor snapshot must be a JSON object`);
    return;
  }
  if (value.schema_version !== 9) {
    errors.push(`${id}: descriptor snapshot must declare schema_version = 9`);
  }
  if (
    value.client_origin_permission !== "participant_only" &&
    value.client_origin_permission !== "origin_capable"
  ) {
    errors.push(
      `${id}: descriptor snapshot must declare a valid client_origin_permission`,
    );
  }
  if (value.format !== "rsjs.component_descriptor") {
    errors.push(`${id}: descriptor snapshot must declare format = rsjs.component_descriptor`);
  }
  if (value.client_ir_version !== clientIrSchemaVersion) {
    errors.push(
      `${id}: descriptor snapshot must declare client_ir_version = ${clientIrSchemaVersion}`,
    );
  }
  if (value.asset_id !== "<asset_id>") {
    errors.push(`${id}: descriptor snapshot must normalize asset_id to <asset_id>`);
  }
  if (value.js_hash !== "sha256:<hash>") {
    errors.push(`${id}: descriptor snapshot must normalize js_hash to sha256:<hash>`);
  }
  if (value.descriptor_hash !== "sha256:<hash>") {
    errors.push(`${id}: descriptor snapshot must normalize descriptor_hash to sha256:<hash>`);
  }
  if (!Array.isArray(value.handlers)) {
    errors.push(`${id}: descriptor snapshot must include handlers array`);
  } else {
    validateDescriptorHandlers(id, value.handlers, errors);
  }
  if (!value.view_tree || typeof value.view_tree !== "object" || Array.isArray(value.view_tree)) {
    errors.push(`${id}: descriptor snapshot must include view_tree object`);
  }
  validateDescriptorCallbackSlots(id, value.callback_slots, errors);
  validateDescriptorFields(id, value.fields, value.callback_slots, errors);
  validateDescriptorBindings(id, value.bindings, value.handlers, errors);
  validateDescriptorCounts(id, value.counts, value, errors);
}

function validateDescriptorHandlers(id, handlers, errors) {
  const handlerIds = new Set();
  for (const handler of handlers) {
    if (!handler || typeof handler !== "object" || Array.isArray(handler)) {
      errors.push(`${id}: descriptor handlers must be objects`);
      continue;
    }
    if (!Number.isInteger(handler.id) || handler.id < 0) {
      errors.push(`${id}: descriptor handler id must be a non-negative integer`);
      continue;
    }
    if (handlerIds.has(handler.id)) {
      errors.push(`${id}: descriptor handler id ${handler.id} is duplicated`);
    }
    handlerIds.add(handler.id);
    if (handler.statements !== undefined) {
      errors.push(`${id}: descriptor handler ${handler.id} must not use legacy statements`);
    }
    if (handler.body_ir_version !== clientIrSchemaVersion) {
      errors.push(
        `${id}: descriptor handler ${handler.id} must declare body_ir_version = ${clientIrSchemaVersion}`,
      );
    }
    if (!handler.body_ir || typeof handler.body_ir !== "object" || Array.isArray(handler.body_ir)) {
      errors.push(`${id}: descriptor handler ${handler.id} must include body_ir object`);
      continue;
    }
    validateClientBlockIrSnapshot(
      id,
      handler.body_ir,
      errors,
      `descriptor handler ${handler.id} body_ir`,
    );
  }
}

function validateDescriptorCallbackSlots(id, callbackSlots, errors) {
  if (!Array.isArray(callbackSlots)) {
    errors.push(`${id}: descriptor snapshot must include callback_slots array`);
    return;
  }
  const slotIds = new Set();
  for (const slot of callbackSlots) {
    if (!slot || typeof slot !== "object" || Array.isArray(slot)) {
      errors.push(`${id}: descriptor callback_slots entries must be objects`);
      continue;
    }
    if (!Number.isInteger(slot.id) || slot.id < 0) {
      errors.push(`${id}: descriptor callback slot id must be a non-negative integer`);
      continue;
    }
    if (slotIds.has(slot.id)) {
      errors.push(`${id}: descriptor callback slot id ${slot.id} is duplicated`);
    }
    slotIds.add(slot.id);
    if (typeof slot.name !== "string" || slot.name.length === 0) {
      errors.push(`${id}: descriptor callback slot ${slot.id} must include a non-empty name`);
    }
  }
}

function validateDescriptorFields(id, fields, callbackSlots, errors) {
  if (!Array.isArray(fields)) {
    errors.push(`${id}: descriptor snapshot must include fields array`);
    return;
  }
  const callbackSlotIds = new Set(
    Array.isArray(callbackSlots)
      ? callbackSlots
          .map((slot) => slot?.id)
          .filter((slotId) => Number.isInteger(slotId) && slotId >= 0)
      : [],
  );
  for (const field of fields) {
    if (!field || typeof field !== "object" || Array.isArray(field)) {
      errors.push(`${id}: descriptor fields entries must be objects`);
      continue;
    }
    if (typeof field.name !== "string" || field.name.length === 0) {
      errors.push(`${id}: descriptor field must include a non-empty name`);
    }
    if (field.kind === "handler") {
      if (!Number.isInteger(field.callback_slot) || field.callback_slot < 0) {
        errors.push(`${id}: descriptor handler field ${field.name ?? "<unknown>"} must include callback_slot`);
      } else if (!callbackSlotIds.has(field.callback_slot)) {
        errors.push(`${id}: descriptor handler field ${field.name ?? "<unknown>"} references unknown callback slot ${field.callback_slot}`);
      }
    }
  }
}

function validateDescriptorBindings(id, bindings, handlers, errors) {
  if (!bindings || typeof bindings !== "object" || Array.isArray(bindings)) {
    errors.push(`${id}: descriptor snapshot must include bindings object`);
    return;
  }
  const handlerIds = new Set(
    Array.isArray(handlers)
      ? handlers
          .map((handler) => handler?.id)
          .filter((handlerId) => Number.isInteger(handlerId) && handlerId >= 0)
      : [],
  );
  if (!Array.isArray(bindings.events)) {
    errors.push(`${id}: descriptor bindings must include events array`);
    return;
  }
  for (const eventBinding of bindings.events) {
    if (!eventBinding || typeof eventBinding !== "object" || Array.isArray(eventBinding)) {
      errors.push(`${id}: descriptor event bindings must be objects`);
      continue;
    }
    if (!Number.isInteger(eventBinding.handler) || eventBinding.handler < 0) {
      errors.push(`${id}: descriptor event binding must reference a non-negative handler id`);
    } else if (!handlerIds.has(eventBinding.handler)) {
      errors.push(`${id}: descriptor event binding references unknown handler ${eventBinding.handler}`);
    }
  }
}

function validateDescriptorCounts(id, counts, descriptor, errors) {
  if (!counts || typeof counts !== "object" || Array.isArray(counts)) {
    errors.push(`${id}: descriptor snapshot must include counts object`);
    return;
  }
  const countChecks = [
    ["handlers", descriptor.handlers],
    ["callback_slots", descriptor.callback_slots],
    ["signals", descriptor.signals],
    ["node_refs", descriptor.node_refs],
    ["exprs", descriptor.exprs],
  ];
  for (const [key, values] of countChecks) {
    if (!Array.isArray(values)) {
      continue;
    }
    if (counts[key] !== values.length) {
      errors.push(`${id}: descriptor counts.${key} must match ${key} array length`);
    }
  }
  if (descriptor.view_tree && typeof descriptor.view_tree === "object" && Array.isArray(descriptor.view_tree.nodes)) {
    if (counts.nodes !== descriptor.view_tree.nodes.length) {
      errors.push(`${id}: descriptor counts.nodes must match view_tree.nodes length`);
    }
  }
}

function collectFixturePaths(dir) {
  return fs
    .readdirSync(dir, { withFileTypes: true })
    .filter((entry) => entry.isFile() && entry.name.endsWith(".rs"))
    .map((entry) => path.join(dir, entry.name))
    .map((fixturePath) => path.relative(workspaceRoot, fixturePath).split(path.sep).join("/"));
}

function stderrPathFor(fixturePath) {
  return fixturePath.replace(/\.rs$/, ".stderr");
}

function collectRuntimeScriptPaths(dir) {
  return fs
    .readdirSync(dir, { withFileTypes: true })
    .filter((entry) => entry.isFile() && entry.name.endsWith(".mjs"))
    .map((entry) => path.join(dir, entry.name))
    .map((scriptPath) => path.relative(workspaceRoot, scriptPath).split(path.sep).join("/"))
    .filter((scriptPath) => scriptPath !== runtimeHarnessPath);
}

function collectSnapshotFilePaths(dir) {
  return fs.readdirSync(dir, { withFileTypes: true }).flatMap((entry) => {
    const entryPath = path.join(dir, entry.name);
    if (entry.isDirectory()) {
      return collectSnapshotFilePaths(entryPath);
    }
    if (!entry.isFile()) {
      return [];
    }
    return path.relative(workspaceRoot, entryPath).split(path.sep).join("/");
  });
}

const byStatus = countBy(features, (feature) => feature.status ?? "unknown");
const byCategory = countBy(features, (feature) => feature.category ?? "unknown");
const bySemanticContract = countBy(
  features.filter((feature) => feature.category === "semantics"),
  (feature) => feature.semantic_contract ?? "unknown",
);
const fixtures = features.flatMap((feature) => feature.fixtures ?? []);
const byFixtureExpectation = countBy(fixtures, (fixture) => fixture.expect ?? "unknown");
const fixturesWithCaseId = fixtures.filter((fixture) => typeof fixture.case_id === "string");
const compilePassFeatures = features.filter((feature) => feature.coverage?.compile_pass === true);
const compileFailFixtures = fixtures.filter((fixture) => fixture.expect === "fail");
const compileFailStderrCount = compileFailFixtures.filter(
  (fixture) => typeof fixture.path === "string" && fs.existsSync(stderrPathFor(path.join(workspaceRoot, fixture.path))),
).length;
const coverageCounts = new Map(
  coverageKeys.map((key) => [
    key,
    features.filter((feature) => feature.coverage?.[key] === true).length,
  ]),
);
const snapshotCounts = new Map(
  snapshotKeys.map((key) => [
    key,
    features.filter((feature) => typeof feature.snapshots?.[key] === "string").length,
  ]),
);
const runtimeScriptCount = features.filter((feature) => typeof feature.runtime_script === "string").length;
const fixtureGateCounts = new Map([
  ["pass", byFixtureExpectation.get("pass") ?? 0],
  ["fail", byFixtureExpectation.get("fail") ?? 0],
  ["case_ids", fixturesWithCaseId.length],
  ["compile_fail_stderr", compileFailStderrCount],
]);

function percent(count, total) {
  return total === 0 ? "0.0" : ((count / total) * 100).toFixed(1);
}

function fixtureGate(key) {
  const min = manifest.gates?.fixture_min?.[key];
  return Number.isInteger(min) ? ` gate ${min}` : "";
}

console.log("rsjs DSL conformance");
console.log("");
console.log("Feature status:");
for (const status of statusOrder) {
  console.log(`  ${pad(status)}${byStatus.get(status) ?? 0}`);
}

console.log("");
console.log("Semantic contracts:");
for (const semanticContract of [...semanticContractSet, "unknown"]) {
  const count = bySemanticContract.get(semanticContract) ?? 0;
  if (count > 0 || semanticContract !== "unknown") {
    console.log(`  ${pad(semanticContract)}${count}`);
  }
}

console.log("");
console.log("Fixtures:");
for (const expect of ["pass", "fail", "unknown"]) {
  const count = byFixtureExpectation.get(expect) ?? 0;
  if (count > 0 || expect !== "unknown") {
    console.log(`  ${pad(expect)}${count}${fixtureGate(expect)}`);
  }
}
console.log(`  ${pad("case ids")}${fixturesWithCaseId.length}/${fixtures.length} ${percent(fixturesWithCaseId.length, fixtures.length)}%${fixtureGate("case_ids")}`);

console.log("");
console.log("Current pass-feature coverage:");
for (const key of ["hir_snapshot", "ir_snapshot", "js_snapshot", "runtime"]) {
  const count = compilePassFeatures.filter((feature) => feature.coverage?.[key] === true).length;
  console.log(`  ${pad(key)}${count}/${compilePassFeatures.length} ${percent(count, compilePassFeatures.length)}%`);
}

console.log("");
console.log("Diagnostic snapshots:");
console.log(`  ${pad("compile_fail stderr")}${compileFailStderrCount}/${compileFailFixtures.length} ${percent(compileFailStderrCount, compileFailFixtures.length)}%${fixtureGate("compile_fail_stderr")}`);

console.log("");
console.log("Coverage flags:");
for (const key of coverageKeys) {
  const count = coverageCounts.get(key) ?? 0;
  const min = manifest.gates?.coverage_min?.[key];
  const gate = Number.isInteger(min) ? ` gate ${min}` : "";
  console.log(`  ${pad(key)}${count}/${features.length} ${percent(count, features.length)}%${gate}`);
}

console.log("");
console.log("Snapshot files:");
for (const key of snapshotKeys) {
  const count = snapshotCounts.get(key) ?? 0;
  console.log(`  ${pad(key)}${count}/${features.length} ${percent(count, features.length)}%`);
}
console.log(`  ${pad("runtime_script")}${runtimeScriptCount}/${features.length} ${percent(runtimeScriptCount, features.length)}%`);

console.log("");
console.log("By category:");
for (const [category, count] of [...byCategory.entries()].sort(([a], [b]) =>
  a.localeCompare(b),
)) {
  console.log(`  ${pad(category)}${count}`);
}

if (check) {
  const errors = validateManifest();
  console.log("");
  if (errors.length === 0) {
    console.log("Conformance manifest check passed.");
  } else {
    console.error("Conformance manifest check failed:");
    for (const error of errors) {
      console.error(`  - ${error}`);
    }
    process.exitCode = 1;
  }
}
