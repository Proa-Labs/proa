// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

export function snakeToCamel(str: string): string {
  return str.replace(/_([a-z])/g, (_, c: string) => c.toUpperCase());
}

export function convertKeys(obj: unknown): unknown {
  if (Array.isArray(obj)) return obj.map(convertKeys);
  if (obj && typeof obj === "object") {
    const result: Record<string, unknown> = {};
    for (const [key, value] of Object.entries(obj as Record<string, unknown>)) {
      result[snakeToCamel(key)] = convertKeys(value);
    }
    return result;
  }
  return obj;
}

export function normalizeSections(data: unknown): unknown {
  if (!data || typeof data !== "object") return data;

  const obj = data as Record<string, unknown>;
  for (const key of ["homesSections", "experiencesSections", "servicesSections"]) {
    const sections = obj[key];
    if (!Array.isArray(sections)) continue;
    obj[key] = sections.map((section: any) =>
      Array.isArray(section)
        ? section
        : [section.title ?? "", section.subtitle ?? "", section.listingIndexes ?? []],
    );
  }

  return obj;
}
