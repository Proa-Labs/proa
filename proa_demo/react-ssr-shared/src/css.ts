// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

import { existsSync, readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

// This module is initialized before any benchmark timing begins. Reading the
// canonical stylesheet here prevents the React and Proa fixtures from silently
// benchmarking different documents while keeping filesystem work out of the
// measured render loop.
const moduleDir = dirname(fileURLToPath(import.meta.url));
const candidates = [
	resolve(moduleDir, "../airbnb-shared.css"),
	resolve(moduleDir, "../../../proa_showcase/src/airbnb/shared.css"),
];
const cssPath = candidates.find(existsSync);

if (!cssPath) {
	throw new Error(
		`Airbnb CSS not found. Tried:\n${candidates.map((path) => `  - ${path}`).join("\n")}`,
	);
}

export const SHARED_CSS = readFileSync(cssPath, "utf8");
