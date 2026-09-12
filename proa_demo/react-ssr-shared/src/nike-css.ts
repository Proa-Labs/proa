// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

// Nike showcase CSS — resolved from Proa showcase source.
// Tries multiple locations: packaged distribution, then dev source tree.
import { readFileSync, existsSync } from "node:fs";
import { resolve, dirname } from "node:path";
import { fileURLToPath } from "node:url";

const __dirname = dirname(fileURLToPath(import.meta.url));

const candidates = [
  // Packaged: bundled alongside react-ssr-shared
  resolve(__dirname, "../nike-shared.css"),
  // Dev: relative to proa_demo/react-ssr-shared/src/
  resolve(__dirname, "../../../proa_showcase/src/nike/shared.css"),
];

const cssPath = candidates.find(existsSync);
if (!cssPath) {
  throw new Error(
    `Nike CSS not found. Tried:\n${candidates.map((p) => `  - ${p}`).join("\n")}`,
  );
}

export const NIKE_CSS = readFileSync(cssPath, "utf-8");
