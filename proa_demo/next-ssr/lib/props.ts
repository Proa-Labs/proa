// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

import { readFileSync } from "node:fs";
import { join } from "node:path";

let cachedProps: Record<string, unknown> | null = null;

export function loadDefaultProps(): Record<string, unknown> {
  if (cachedProps) return cachedProps;

  const path = join(process.cwd(), "..", "showcase-props.json");
  cachedProps = JSON.parse(readFileSync(path, "utf8"));
  return cachedProps;
}
