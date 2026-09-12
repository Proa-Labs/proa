// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

import { readFileSync, existsSync } from "node:fs";
import { resolve } from "node:path";
import { convertKeys, normalizeSections } from "./normalize";

const cssCandidates = [
  resolve(process.cwd(), "..", "react-ssr-shared", "nike-shared.css"),
  resolve(process.cwd(), "..", "..", "proa_showcase", "src", "nike", "shared.css"),
];

const cssPath = cssCandidates.find(existsSync);
if (!cssPath) {
  throw new Error(
    `Nike CSS not found. Tried:\n${cssCandidates.map((p) => `  - ${p}`).join("\n")}`,
  );
}

export const NIKE_CSS = readFileSync(cssPath, "utf8");
export const NIKE_NEXT_CSS = NIKE_CSS.replace(/body\.nike-page/g, "body");

export function nikeComponentProps(props: unknown): { data?: unknown } {
  const converted = props ? convertKeys(props) : undefined;
  const data = converted ? normalizeSections(converted) : undefined;
  return data ? { data } : {};
}

export function wrapInNikeShell(title: string, bodyHtml: string): string {
  return `<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="UTF-8"/>
<meta name="viewport" content="width=device-width, initial-scale=1.0"/>
<meta name="robots" content="noindex, nofollow"/>
<title>${title}</title>
<style>${NIKE_CSS}</style>
</head><body class="nike-page">${bodyHtml}</body></html>`;
}
