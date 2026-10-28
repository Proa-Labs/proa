// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

export class ProaApiError extends Error {
  constructor(status, problem, retryAfter) {
    super(typeof problem?.detail === "string" ? problem.detail : `Proa API returned HTTP ${status}`);
    this.name = "ProaApiError";
    this.status = status;
    this.problem = problem;
    this.retryAfter = retryAfter;
  }
}

/** Read-only client for https://proa.so/openapi.json. No credentials required. */
export class ProaDocs {
  constructor({ baseUrl = "https://proa.so", timeoutMs = 15000, fetch: fetcher = globalThis.fetch } = {}) {
    const url = new URL(baseUrl);
    if (!["https:", "http:"].includes(url.protocol) || url.username || url.password || url.search || url.hash) {
      throw new TypeError("baseUrl must be an HTTP(S) base URL without credentials, query, or fragment");
    }
    if (!Number.isFinite(timeoutMs) || timeoutMs <= 0 || !Number.isInteger(timeoutMs)) {
      throw new TypeError("timeoutMs must be a positive integer");
    }
    this.baseUrl = url.href.replace(/\/+$/, "") + "/";
    this.timeoutMs = timeoutMs;
    this.fetch = fetcher;
  }

  async request(path, parameters, signal) {
    const url = new URL(path, this.baseUrl);
    for (const [name, value] of Object.entries(parameters)) {
      if (value !== undefined && value !== null) url.searchParams.set(name, String(value));
    }
    const timeout = AbortSignal.timeout(this.timeoutMs);
    const response = await this.fetch(url, {
      method: "GET",
      headers: { Accept: "application/json" },
      credentials: "omit",
      signal: signal ? AbortSignal.any([signal, timeout]) : timeout,
    });
    if (!response.ok) {
      const problem = await response.json().catch(() => null);
      const header = response.headers.get("retry-after");
      const retryAfter = header !== null && /^\d+$/.test(header) ? Number(header) : null;
      throw new ProaApiError(response.status, problem, retryAfter);
    }
    return response.json();
  }

  search(q, { limit, cursor, signal } = {}) {
    return this.request("api/v1/search", { q, limit, cursor }, signal);
  }

  list({ limit, cursor, signal } = {}) {
    return this.request("api/v1/docs", { limit, cursor }, signal);
  }

  get(path, { signal } = {}) {
    return this.request("api/v1/docs/page", { path }, signal);
  }

  /** Follow pagination while detecting a broken/repeating server cursor. */
  async *documents({ limit = 50, signal } = {}) {
    const seen = new Set();
    let cursor;
    do {
      const page = await this.list({ limit, cursor, signal });
      for (const document of page.results) yield document;
      cursor = page.next_cursor;
      if (cursor !== null) {
        if (typeof cursor !== "string" || seen.has(cursor)) throw new Error("Invalid or repeating Proa pagination cursor");
        seen.add(cursor);
      }
    } while (cursor !== null);
  }
}
