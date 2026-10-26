// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

// Optional browser integration; all operations also work without JavaScript.
// Use fixed same-origin API routes and let the server resolve public doc paths.
export async function registerAgentTools(context) {
  if (!context || typeof context.registerTool !== "function") return;

  const request = async (route, input, signal) => {
    const params = new URLSearchParams();
    for (const [key, value] of Object.entries(input)) {
      if (value !== undefined) params.set(key, String(value));
    }
    const response = await fetch(`${route}?${params}`, {
      headers: { Accept: "application/json" },
      credentials: "omit",
      redirect: "error",
      signal,
    });
    const result = await response.json();
    if (!response.ok) throw new Error(result.detail || `Documentation request failed (${response.status})`);
    return JSON.stringify(result);
  };

  const tools = [
    {
      name: "search_docs",
      description: "Search official Proa Rust framework documentation. Returns document titles, paths, Markdown URLs, and a next_cursor. Use get_doc to read a returned path.",
      inputSchema: {
        type: "object",
        properties: {
          q: { type: "string", minLength: 1, maxLength: 256, description: "Phrase to find in Proa documentation." },
          limit: { type: "integer", minimum: 1, maximum: 50, description: "Maximum results; defaults to 10." },
          cursor: { type: "string", description: "next_cursor from the preceding search with the same q." },
        },
        required: ["q"],
        additionalProperties: false,
      },
      annotations: { readOnlyHint: true },
      execute: ({ q, limit, cursor }, { signal } = {}) => request("/api/v1/search", { q, limit, cursor }, signal),
    },
    {
      name: "get_doc",
      description: "Read a published Proa documentation page as Markdown with metadata. Supply a path returned by search_docs, for example /docs/core/html-rendering. Does not navigate or change the page.",
      inputSchema: {
        type: "object",
        properties: { path: { type: "string", description: "Published documentation route or Markdown path." } },
        required: ["path"],
        additionalProperties: false,
      },
      annotations: { readOnlyHint: true },
      execute: ({ path }, { signal } = {}) => request("/api/v1/docs/page", { path }, signal),
    },
  ];
  // Registration is optional and may be denied by browser permissions policy.
  await Promise.allSettled(tools.map(tool => Promise.resolve().then(() => context.registerTool(tool))));
}

if (typeof document !== "undefined") {
  void registerAgentTools(document.modelContext || globalThis.navigator?.modelContext);
}
