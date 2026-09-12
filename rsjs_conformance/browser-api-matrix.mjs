#!/usr/bin/env node
// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

// Validates browser_api_matrix.json and prints a coverage summary.
//
// Usage:
//   node rsjs_conformance/browser-api-matrix.mjs           # summary
//   node rsjs_conformance/browser-api-matrix.mjs --check   # exit 1 on schema violations
//
// To refresh `baseline` values, compare against the web-features package
// (https://www.npmjs.com/package/web-features). To verify `bcd_keys`, look
// them up in @mdn/browser-compat-data. Both are intentionally not runtime
// dependencies; the matrix is a curated artifact, not a generated one.

import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const here = path.dirname(fileURLToPath(import.meta.url));
const matrixPath = path.join(here, "browser_api_matrix.json");
const matrix = JSON.parse(fs.readFileSync(matrixPath, "utf8"));
const check = process.argv.includes("--check");

const statusOrder = [
  "supported",
  "partial",
  "helper_only",
  "rejected_by_design",
  "planned",
  "missing",
];
const baselineValues = new Set(["high", "low", "false", "unknown"]);
const requiredFields = ["id", "area", "description", "status", "bcd_keys", "baseline"];

const errors = [];
const entries = matrix.entries ?? [];

if (matrix.schema_version !== 1) {
  errors.push(`unexpected schema_version: ${matrix.schema_version}`);
}

const declaredStatuses = new Set(Object.keys(matrix.statuses ?? {}));
for (const status of statusOrder) {
  if (!declaredStatuses.has(status)) {
    errors.push(`statuses block is missing a definition for "${status}"`);
  }
}

const seenIds = new Set();
for (const entry of entries) {
  const label = entry.id ?? "<missing id>";
  for (const field of requiredFields) {
    if (entry[field] === undefined) {
      errors.push(`${label}: missing required field "${field}"`);
    }
  }
  if (entry.id !== undefined) {
    if (seenIds.has(entry.id)) {
      errors.push(`duplicate id: ${entry.id}`);
    }
    seenIds.add(entry.id);
  }
  if (entry.status !== undefined && !statusOrder.includes(entry.status)) {
    errors.push(`${label}: unknown status "${entry.status}"`);
  }
  if (entry.baseline !== undefined && !baselineValues.has(entry.baseline)) {
    errors.push(`${label}: unknown baseline "${entry.baseline}" (use high, low, false, or unknown)`);
  }
  if (entry.bcd_keys !== undefined && !Array.isArray(entry.bcd_keys)) {
    errors.push(`${label}: bcd_keys must be an array`);
  }
  // Statuses that assert a current behavior must say where that behavior lives.
  const needsEvidence = ["supported", "partial", "helper_only", "rejected_by_design"];
  if (needsEvidence.includes(entry.status) && !entry.evidence) {
    errors.push(`${label}: status "${entry.status}" requires an evidence field`);
  }
  if (["partial", "rejected_by_design"].includes(entry.status) && !entry.notes) {
    errors.push(`${label}: status "${entry.status}" requires a notes field explaining the restriction`);
  }
}

const byStatus = new Map(statusOrder.map((s) => [s, 0]));
const byArea = new Map();
for (const entry of entries) {
  if (byStatus.has(entry.status)) {
    byStatus.set(entry.status, byStatus.get(entry.status) + 1);
  }
  const area = entry.area ?? "<none>";
  byArea.set(area, (byArea.get(area) ?? 0) + 1);
}

console.log(`browser API matrix: ${entries.length} entries across ${byArea.size} areas`);
for (const status of statusOrder) {
  console.log(`  ${status.padEnd(20)} ${byStatus.get(status)}`);
}
const openGaps = entries.filter((e) => e.status === "planned" || e.status === "missing");
if (openGaps.length > 0) {
  console.log("open gaps:");
  for (const entry of openGaps) {
    console.log(`  [${entry.status}] ${entry.id}: ${entry.description}`);
  }
}

if (errors.length > 0) {
  console.error(`\n${errors.length} schema violation(s):`);
  for (const error of errors) {
    console.error(`  - ${error}`);
  }
  if (check) {
    process.exit(1);
  }
} else if (check) {
  console.log("matrix check passed");
}
