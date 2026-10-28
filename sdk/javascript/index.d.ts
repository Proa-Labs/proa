// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

export interface DocumentSummary {
  title: string;
  path: string;
  markdown_url: string;
  description: string;
}
export interface Document extends DocumentSummary { markdown: string }
export interface DocumentList {
  results: DocumentSummary[];
  total: number;
  next_cursor: string | null;
}
export interface Problem {
  type: string;
  title: string;
  status: number;
  code: string;
  detail: string;
  documentation: string;
}
export interface PageOptions {
  limit?: number;
  cursor?: string;
  signal?: AbortSignal;
}
export interface ClientOptions {
  baseUrl?: string;
  timeoutMs?: number;
  fetch?: typeof globalThis.fetch;
}
export class ProaApiError extends Error {
  constructor(status: number, problem: Problem | null, retryAfter: number | null);
  status: number;
  problem: Problem | null;
  /** Retry-After seconds on rate-limited responses; no automatic retry. */
  retryAfter: number | null;
}
export class ProaDocs {
  constructor(options?: ClientOptions);
  search(q: string, options?: PageOptions): Promise<DocumentList>;
  list(options?: PageOptions): Promise<DocumentList>;
  get(path: string, options?: Pick<PageOptions, "signal">): Promise<Document>;
  documents(options?: Omit<PageOptions, "cursor">): AsyncGenerator<DocumentSummary, void, unknown>;
}
