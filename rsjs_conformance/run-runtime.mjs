#!/usr/bin/env node
// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

import { spawnSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const here = path.dirname(fileURLToPath(import.meta.url));
const workspaceRoot = path.resolve(here, "..");
const manifestPath = path.join(here, "features.json");
const manifest = JSON.parse(fs.readFileSync(manifestPath, "utf8"));
const features = manifest.features ?? [];

const scripts = [];
const seen = new Set();

for (const feature of features) {
  if (feature.coverage?.runtime !== true) {
    continue;
  }

  const runtimeScript = feature.runtime_script;
  if (typeof runtimeScript !== "string") {
    console.error(`${feature.id ?? "<unknown>"} has runtime coverage but no runtime_script`);
    process.exit(1);
  }
  if (seen.has(runtimeScript)) {
    console.error(`${runtimeScript} is referenced by multiple runtime-covered features`);
    process.exit(1);
  }
  seen.add(runtimeScript);
  scripts.push({ id: feature.id ?? runtimeScript, path: runtimeScript });
}

if (scripts.length === 0) {
  console.error("No runtime conformance scripts are declared.");
  process.exit(1);
}

for (const script of scripts) {
  const absolutePath = path.join(workspaceRoot, script.path);
  console.log(`rsjs runtime conformance: ${script.id}`);
  const result = spawnSync(process.execPath, [absolutePath], {
    cwd: workspaceRoot,
    env: process.env,
    stdio: "inherit",
  });

  if (result.error) {
    throw result.error;
  }
  if (result.status !== 0) {
    process.exit(result.status ?? 1);
  }
}
