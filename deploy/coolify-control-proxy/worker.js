// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

const ORIGIN = "https://coolify.proa.cool";
const GITHUB_ISSUER = "https://token.actions.githubusercontent.com";
const GITHUB_JWKS_URL = `${GITHUB_ISSUER}/.well-known/jwks`;
const SESSION_PREFIX = "proa1";
const MAX_WEBHOOK_BYTES = 5 * 1024 * 1024;
const MAX_EXCHANGE_BYTES = 8 * 1024;
const MAX_ARCHIVE_REQUEST_BYTES = 1024;
const MAX_ARCHIVE_RESPONSE_BYTES = 8 * 1024;
const MAX_GITHUB_APP_PRIVATE_KEY_BYTES = 32 * 1024;
const MAX_GITHUB_API_RESPONSE_BYTES = 64 * 1024;
const MAX_COOLIFY_JSON_BYTES = 1024 * 1024;
// Coolify v4.1.2 serializes unbounded deployment logs in every history row.
// Keep history on its own bounded ceiling so a normal multi-row build page
// cannot disable reconciliation, while retaining the tighter limit for every
// other Coolify response.
const MAX_COOLIFY_HISTORY_BYTES = 8 * 1024 * 1024;
// Five rows keep the bounded 100-row scan within Workers' subrequest budget.
const COOLIFY_HISTORY_PAGE_SIZE = 5;
const MAX_COOLIFY_HISTORY_ROWS = 100;
const MAX_READY_BYTES = 32;
const DEFAULT_SESSION_TTL_SECONDS = 4_500;
const SHORT_SESSION_TTL_SECONDS = 300;
const MAX_ACTIVE_REUSE_AGE_MS = 45 * 60 * 1000;
const MAX_TIMESTAMP_FUTURE_SKEW_MS = 5 * 60 * 1000;
const ARCHIVE_ORIGIN_PATH = "/_proa/origin/preview-archive";
const PRESENTATION_TOKEN_PATH = "/_proa/github-actions/presentation-token";
const PRESENTATION_REPOSITORY = "Proa-Labs/proa";
const PRESENTATION_REPOSITORY_OWNER = "Proa-Labs";
const PRESENTATION_REPOSITORY_NAME = "proa";
const GITHUB_API_ORIGIN = "https://api.github.com";
const FULL_COMMIT_RE = /^[0-9a-fA-F]{40}$/u;
const ARCHIVE_DEPLOYMENT_RE = /^[a-z0-9]{24}$/u;
const ARCHIVE_APPLICATION_RE = /^[a-z0-9](?:[a-z0-9-]{0,61}[a-z0-9])?$/u;
const IDENTIFIER_RE = /^[A-Za-z0-9._-]{1,128}$/u;
const STATUS_RE = /^[a-z0-9._-]{1,64}$/u;
const ACTIVE_DEPLOYMENT_STATUSES = new Set(["queued", "in_progress", "pending", "running"]);
const SUCCESS_DEPLOYMENT_STATUSES = new Set(["finished", "succeeded", "success", "completed"]);
const SESSION_PURPOSES = new Set(["archive", "deploy", "presentation", "teardown"]);
const SHORT_SESSION_PURPOSES = new Set(["archive", "presentation", "teardown"]);
const ARCHIVE_OPERATIONS = new Set(["promote", "revoke"]);
const PRESENTATION_PERMISSIONS = Object.freeze({
  deployments: "write",
  issues: "write",
  pull_requests: "write",
});

let jwksCache = null;
const reconcileFlights = new Map();

class DeploymentHistoryUnavailableError extends Error {
  constructor(reason) {
    super("deployment history unavailable");
    this.reason = reason;
  }
}

class DeploymentDetailScopeError extends Error {}

class DeploymentDetailUnavailableError extends Error {}

function historyUnavailableReason(error) {
  return error instanceof DeploymentHistoryUnavailableError
    ? error.reason
    : "history_unavailable";
}

function textResponse(status, message) {
  return new Response(`${message}\n`, {
    status,
    headers: { "content-type": "text/plain; charset=utf-8" },
  });
}

function jsonResponse(status, payload) {
  return new Response(JSON.stringify(payload), {
    status,
    headers: {
      "cache-control": "no-store",
      "content-type": "application/json; charset=utf-8",
    },
  });
}

function base64UrlEncode(bytes) {
  let binary = "";
  for (const byte of bytes) binary += String.fromCharCode(byte);
  return btoa(binary).replaceAll("+", "-").replaceAll("/", "_").replace(/=+$/u, "");
}

function base64UrlDecode(value) {
  if (typeof value !== "string" || !/^[A-Za-z0-9_-]+$/u.test(value)) {
    throw new Error("invalid base64url");
  }
  const padded = value.replaceAll("-", "+").replaceAll("_", "/").padEnd(
    Math.ceil(value.length / 4) * 4,
    "=",
  );
  const binary = atob(padded);
  return Uint8Array.from(binary, (character) => character.charCodeAt(0));
}

function utf8(value) {
  return new TextEncoder().encode(value);
}

function isPlainObject(value) {
  return value !== null && typeof value === "object" && !Array.isArray(value);
}

function decodeJsonSegment(value) {
  return JSON.parse(new TextDecoder().decode(base64UrlDecode(value)));
}

async function importHmacKey(secret) {
  return crypto.subtle.importKey(
    "raw",
    utf8(secret),
    { name: "HMAC", hash: "SHA-256" },
    false,
    ["sign", "verify"],
  );
}

async function hmac(secret, bytes) {
  const key = await importHmacKey(secret);
  return new Uint8Array(await crypto.subtle.sign("HMAC", key, bytes));
}

function concatenateBytes(...parts) {
  const length = parts.reduce((total, part) => total + part.byteLength, 0);
  const bytes = new Uint8Array(length);
  let offset = 0;
  for (const part of parts) {
    bytes.set(part, offset);
    offset += part.byteLength;
  }
  return bytes;
}

async function archiveOriginSignature(secret, timestamp, body) {
  const prefix = utf8(`v1\nPOST\n${ARCHIVE_ORIGIN_PATH}\n${timestamp}\n`);
  return bytesToHex(await hmac(secret, concatenateBytes(prefix, body)));
}

function constantTimeEqual(left, right) {
  if (left.length !== right.length) return false;
  let difference = 0;
  for (let index = 0; index < left.length; index += 1) {
    difference |= left[index] ^ right[index];
  }
  return difference === 0;
}

function hexToBytes(value) {
  if (!/^[0-9a-f]+$/u.test(value) || value.length % 2 !== 0) return null;
  const bytes = new Uint8Array(value.length / 2);
  for (let index = 0; index < bytes.length; index += 1) {
    bytes[index] = Number.parseInt(value.slice(index * 2, index * 2 + 2), 16);
  }
  return bytes;
}

function bytesToHex(bytes) {
  return [...bytes].map((byte) => byte.toString(16).padStart(2, "0")).join("");
}

async function verifyWebhookSignature(request, body, secret) {
  const header = request.headers.get("x-hub-signature-256") || "";
  const match = /^sha256=([0-9a-f]{64})$/u.exec(header);
  if (!match) return false;
  const supplied = hexToBytes(match[1]);
  const expected = await hmac(secret, body);
  return supplied !== null && constantTimeEqual(supplied, expected);
}

function declaredLength(headers) {
  const value = headers.get("content-length");
  if (value === null) return null;
  if (!/^(0|[1-9][0-9]*)$/u.test(value)) throw new Error("invalid content length");
  const length = Number(value);
  if (!Number.isSafeInteger(length)) throw new Error("invalid content length");
  return length;
}

async function readBoundedStream(stream, maximumBytes) {
  if (stream === null) return new Uint8Array();
  const reader = stream.getReader();
  const chunks = [];
  let length = 0;
  try {
    while (true) {
      const { done, value } = await reader.read();
      if (done) break;
      length += value.byteLength;
      if (length > maximumBytes) throw new Error("body too large");
      chunks.push(value);
    }
  } catch (error) {
    await reader.cancel().catch(() => {});
    throw error;
  }
  const bytes = new Uint8Array(length);
  let offset = 0;
  for (const chunk of chunks) {
    bytes.set(chunk, offset);
    offset += chunk.byteLength;
  }
  return bytes;
}

async function readRequestBytes(request, maximumBytes) {
  const length = declaredLength(request.headers);
  if (length !== null && length > maximumBytes) throw new Error("body too large");
  return readBoundedStream(request.body, maximumBytes);
}

async function readResponseBytes(response, maximumBytes) {
  const length = declaredLength(response.headers);
  if (length !== null && length > maximumBytes) throw new Error("body too large");
  return readBoundedStream(response.body, maximumBytes);
}

function cancelResponseBody(response) {
  try {
    const cancellation = response?.body?.cancel();
    cancellation?.catch(() => {});
  } catch {
    // A consumed/locked body is already unavailable to callers. Cancellation
    // is best-effort cleanup and must never become an unhandled rejection.
  }
}

function decodeJson(bytes) {
  return JSON.parse(new TextDecoder("utf-8", { fatal: true }).decode(bytes));
}

function responseCookies(headers) {
  if (typeof headers.getSetCookie === "function") {
    const values = headers.getSetCookie();
    if (Array.isArray(values) && values.length > 0) return values;
  }
  if (typeof headers.getAll === "function") {
    try {
      const values = headers.getAll("Set-Cookie");
      if (Array.isArray(values) && values.length > 0) return values;
    } catch {
      // getAll is a legacy Workers extension and throws for unsupported names.
    }
  }
  const value = headers.get("set-cookie");
  return value === null ? [] : [value];
}

function rewriteOriginCookie(cookie) {
  return cookie.replace(
    /;\s*Domain=\.?coolify\.proa\.cool(?=;|$)/giu,
    "",
  );
}

function responseHeadersForPublicOrigin(response, publicUrl) {
  const cookies = responseCookies(response.headers);
  const headers = new Headers(response.headers);
  headers.set("strict-transport-security", "max-age=31536000; includeSubDomains");
  const location = headers.get("location");
  if (location) {
    try {
      const parsed = new URL(location, ORIGIN);
      if (parsed.origin === ORIGIN) {
        parsed.protocol = publicUrl.protocol;
        parsed.host = publicUrl.host;
        headers.set("location", parsed.toString());
      }
    } catch {
      // Preserve malformed/relative upstream values; the browser will reject
      // malformed redirects and resolve valid relatives against the public URL.
    }
  }
  if (cookies.length > 0) {
    headers.delete("set-cookie");
    for (const cookie of cookies) headers.append("set-cookie", rewriteOriginCookie(cookie));
  }
  return headers;
}

async function proxyToOrigin(request, env, { body = undefined, headers = undefined } = {}) {
  const publicUrl = new URL(request.url);
  const originUrl = new URL(request.url);
  originUrl.protocol = "https:";
  originUrl.host = new URL(ORIGIN).host;

  const outboundHeaders = new Headers(headers || request.headers);
  outboundHeaders.set("x-proa-control-plane", env.ORIGIN_AUTH);
  outboundHeaders.set("x-forwarded-host", publicUrl.host);
  outboundHeaders.set("x-forwarded-proto", "https");

  const requestInit = {
    method: request.method,
    headers: outboundHeaders,
    redirect: "manual",
  };
  if (!["GET", "HEAD"].includes(request.method)) {
    requestInit.body = body === undefined ? request.body : body;
  }
  const outbound = new Request(originUrl, requestInit);
  const response = await fetch(outbound);
  const responseHeaders = responseHeadersForPublicOrigin(response, publicUrl);
  return new Response(response.body, {
    status: response.status,
    statusText: response.statusText,
    headers: responseHeaders,
  });
}

async function handleGithubWebhook(request, env) {
  if (!env.GITHUB_WEBHOOK_SECRET || !env.GITHUB_APP_ID || !env.GITHUB_REPOSITORY_ID) {
    return textResponse(503, "webhook policy is not configured");
  }
  if (request.method !== "POST") return textResponse(405, "method not allowed");
  let body;
  try {
    body = await readRequestBytes(request, MAX_WEBHOOK_BYTES);
  } catch {
    return textResponse(413, "webhook payload is too large");
  }
  if (!(await verifyWebhookSignature(request, body, env.GITHUB_WEBHOOK_SECRET))) {
    return textResponse(401, "invalid webhook signature");
  }

  if (request.headers.get("x-github-hook-installation-target-id") !== env.GITHUB_APP_ID) {
    return textResponse(403, "GitHub App identity rejected");
  }

  const event = request.headers.get("x-github-event");
  if (["pull_request", "push"].includes(event)) {
    let payload;
    try {
      payload = decodeJson(body);
    } catch {
      return textResponse(400, "invalid webhook payload");
    }
    const repositoryId = payload?.repository?.id;
    const repositoryName = payload?.repository?.full_name;
    if (!Number.isSafeInteger(repositoryId) || repositoryId <= 0) {
      return textResponse(400, "invalid webhook repository identity");
    }
    const repositoryIdMatches = String(repositoryId) === env.GITHUB_REPOSITORY_ID;
    const repositoryNameMatches = repositoryName === env.GITHUB_REPOSITORY;
    if (repositoryIdMatches !== repositoryNameMatches) {
      return textResponse(409, "webhook repository identity mismatch");
    }
    if (repositoryIdMatches && repositoryNameMatches) {
      // Protected workflows own ordered deploy and preview transitions for
      // this repository. Authentication is still checked above so spoofed
      // traffic cannot use this endpoint as a generic suppression oracle.
      return jsonResponse(202, { accepted: false, reason: "actions_owned" });
    }
  }
  // Preserve the GitHub App's behavior for every other installed repository
  // and for installation lifecycle events. The control policy is deliberately
  // scoped to GITHUB_REPOSITORY_ID rather than the entire App installation.
  if (event) {
    return proxyToOrigin(request, env, { body });
  }
  return textResponse(400, "missing GitHub event");
}

async function githubJwks() {
  const now = Date.now();
  if (jwksCache && jwksCache.expiresAt > now) return jwksCache.keys;
  const response = await fetch(GITHUB_JWKS_URL, {
    headers: { accept: "application/json" },
    cf: { cacheTtl: 300, cacheEverything: true },
  });
  if (!response.ok) throw new Error("GitHub OIDC keys unavailable");
  const payload = await response.json();
  if (!Array.isArray(payload?.keys)) throw new Error("GitHub OIDC keys malformed");
  jwksCache = { keys: payload.keys, expiresAt: now + 5 * 60 * 1000 };
  return payload.keys;
}

function claimEquals(claims, name, expected) {
  return typeof expected === "string" && expected && claims?.[name] === expected;
}

function positiveInteger(value) {
  if (typeof value === "number") {
    return Number.isSafeInteger(value) && value > 0 ? value : null;
  }
  if (typeof value === "string" && /^[1-9][0-9]*$/u.test(value)) {
    const parsed = Number(value);
    return Number.isSafeInteger(parsed) ? parsed : null;
  }
  return null;
}

function decodePkcs8PrivateKey(pem) {
  if (
    typeof pem !== "string" ||
    pem.length === 0 ||
    utf8(pem).byteLength > MAX_GITHUB_APP_PRIVATE_KEY_BYTES
  ) {
    throw new Error("presentation private key is not configured");
  }
  const match = /^-----BEGIN PRIVATE KEY-----\r?\n([A-Za-z0-9+/=\r\n]+)\r?\n-----END PRIVATE KEY-----\r?\n?$/u.exec(pem);
  if (!match) throw new Error("presentation private key is not PKCS8 PEM");
  const encoded = match[1].replace(/[\r\n]/gu, "");
  if (
    encoded.length === 0 ||
    encoded.length % 4 !== 0 ||
    !/^[A-Za-z0-9+/]+={0,2}$/u.test(encoded)
  ) {
    throw new Error("presentation private key is not PKCS8 PEM");
  }
  let binary;
  try {
    binary = atob(encoded);
  } catch {
    throw new Error("presentation private key is not PKCS8 PEM");
  }
  if (binary.length === 0 || binary.length > MAX_GITHUB_APP_PRIVATE_KEY_BYTES) {
    throw new Error("presentation private key is not PKCS8 PEM");
  }
  return Uint8Array.from(binary, (character) => character.charCodeAt(0));
}

async function presentationConfiguration(env) {
  const appId = positiveInteger(env.PROA_PREVIEW_APP_ID);
  const appSlug = env.PROA_PREVIEW_APP_SLUG;
  if (
    env.GITHUB_REPOSITORY !== PRESENTATION_REPOSITORY ||
    appId === null ||
    typeof appSlug !== "string" ||
    !/^[a-z0-9](?:[a-z0-9-]{0,98}[a-z0-9])?$/u.test(appSlug)
  ) {
    throw new Error("presentation App is not configured");
  }
  const privateKey = await crypto.subtle.importKey(
    "pkcs8",
    decodePkcs8PrivateKey(env.PROA_PREVIEW_APP_PRIVATE_KEY),
    { name: "RSASSA-PKCS1-v1_5", hash: "SHA-256" },
    false,
    ["sign"],
  );
  return { appId, appSlug, privateKey };
}

async function githubAppJwt(configuration, now = Math.floor(Date.now() / 1000)) {
  const header = base64UrlEncode(utf8(JSON.stringify({ alg: "RS256", typ: "JWT" })));
  const payload = base64UrlEncode(
    utf8(JSON.stringify({ iat: now - 60, exp: now + 540, iss: configuration.appId })),
  );
  const signingInput = `${header}.${payload}`;
  const signature = new Uint8Array(
    await crypto.subtle.sign(
      "RSASSA-PKCS1-v1_5",
      configuration.privateKey,
      utf8(signingInput),
    ),
  );
  return `${signingInput}.${base64UrlEncode(signature)}`;
}

function githubApiHeaders(authorization) {
  return {
    accept: "application/vnd.github+json",
    authorization,
    "user-agent": "Proa-Preview-Token-Broker",
    "x-github-api-version": "2022-11-28",
  };
}

async function githubJson(request, expectedStatus) {
  let response;
  try {
    response = await fetch(request);
    if (response.status !== expectedStatus) {
      await cancelResponseBody(response);
      throw new Error("GitHub API status rejected");
    }
    return decodeJson(await readResponseBytes(response, MAX_GITHUB_API_RESPONSE_BYTES));
  } catch {
    await cancelResponseBody(response);
    throw new Error("GitHub API response rejected");
  }
}

function githubInstallationId(payload, configuration) {
  if (
    !isPlainObject(payload) ||
    positiveInteger(payload.id) === null ||
    positiveInteger(payload.app_id) !== configuration.appId ||
    payload.app_slug !== configuration.appSlug ||
    payload.repository_selection !== "selected" ||
    payload.target_type !== "Organization" ||
    !isPlainObject(payload.account) ||
    payload.account.login !== PRESENTATION_REPOSITORY_OWNER
  ) {
    throw new Error("GitHub App installation rejected");
  }
  return positiveInteger(payload.id);
}

function exactPresentationPermissions(permissions) {
  if (!isPlainObject(permissions)) return false;
  const keys = Object.keys(permissions).sort();
  const expectedKeys = Object.keys(PRESENTATION_PERMISSIONS).sort();
  if (permissions.metadata === "read") expectedKeys.push("metadata");
  expectedKeys.sort();
  return (
    keys.length === expectedKeys.length &&
    keys.every((key, index) => key === expectedKeys[index]) &&
    Object.entries(PRESENTATION_PERMISSIONS).every(
      ([permission, level]) => permissions[permission] === level,
    )
  );
}

function projectPresentationToken(payload, configuration, now = Date.now()) {
  const token = payload?.token;
  const expiresAt = payload?.expires_at;
  const expiry = typeof expiresAt === "string" ? Date.parse(expiresAt) : Number.NaN;
  const repositories = payload?.repositories;
  if (
    !isPlainObject(payload) ||
    typeof token !== "string" ||
    !/^ghs_[A-Za-z0-9._-]{36,2044}$/u.test(token) ||
    !/^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}Z$/u.test(expiresAt || "") ||
    !Number.isFinite(expiry) ||
    expiry <= now + 30_000 ||
    expiry > now + 3_700_000 ||
    payload.repository_selection !== "selected" ||
    !exactPresentationPermissions(payload.permissions) ||
    !Array.isArray(repositories) ||
    repositories.length !== 1 ||
    !isPlainObject(repositories[0]) ||
    repositories[0].name !== PRESENTATION_REPOSITORY_NAME ||
    repositories[0].full_name !== PRESENTATION_REPOSITORY
  ) {
    throw new Error("GitHub App token rejected");
  }
  return {
    token,
    expires_at: expiresAt,
    bot_login: `${configuration.appSlug}[bot]`,
  };
}

async function mintPresentationToken(configuration) {
  const appToken = await githubAppJwt(configuration);
  const installation = await githubJson(
    new Request(
      `${GITHUB_API_ORIGIN}/repos/${PRESENTATION_REPOSITORY_OWNER}/${PRESENTATION_REPOSITORY_NAME}/installation`,
      {
        method: "GET",
        headers: githubApiHeaders(`Bearer ${appToken}`),
        redirect: "manual",
      },
    ),
    200,
  );
  const installationId = githubInstallationId(installation, configuration);
  const body = JSON.stringify({
    repositories: [PRESENTATION_REPOSITORY_NAME],
    permissions: PRESENTATION_PERMISSIONS,
  });
  const token = await githubJson(
    new Request(`${GITHUB_API_ORIGIN}/app/installations/${installationId}/access_tokens`, {
      method: "POST",
      headers: {
        ...githubApiHeaders(`Bearer ${appToken}`),
        "content-type": "application/json",
      },
      body,
      redirect: "manual",
    }),
    201,
  );
  return projectPresentationToken(token, configuration);
}

async function handlePresentationToken(request, env, token) {
  if (request.method !== "POST") return textResponse(405, "method not allowed");
  let claims;
  try {
    claims = await verifySessionToken(token, env);
  } catch {
    return textResponse(403, "Actions session rejected");
  }
  if (claims.purpose !== "presentation") {
    return textResponse(403, "Actions session purpose rejected");
  }
  try {
    const body = await readRequestBytes(request, 0);
    if (body.byteLength !== 0) throw new Error("unexpected request body");
  } catch {
    return jsonResponse(400, { result: "error", reason: "invalid_request" });
  }
  let configuration;
  try {
    configuration = await presentationConfiguration(env);
  } catch {
    return textResponse(503, "GitHub presentation broker is not configured");
  }
  try {
    return jsonResponse(200, await mintPresentationToken(configuration));
  } catch {
    return jsonResponse(502, {
      result: "error",
      reason: "github_app_token_unavailable",
    });
  }
}

async function verifyGithubOidc(token, env) {
  const parts = token.split(".");
  if (parts.length !== 3) throw new Error("invalid OIDC token");
  const header = decodeJsonSegment(parts[0]);
  const claims = decodeJsonSegment(parts[1]);
  if (header?.alg !== "RS256" || typeof header?.kid !== "string") {
    throw new Error("invalid OIDC algorithm");
  }
  const keys = await githubJwks();
  const jwk = keys.find((candidate) => candidate?.kid === header.kid && candidate?.kty === "RSA");
  if (!jwk) throw new Error("unknown OIDC signing key");
  const key = await crypto.subtle.importKey(
    "jwk",
    jwk,
    { name: "RSASSA-PKCS1-v1_5", hash: "SHA-256" },
    false,
    ["verify"],
  );
  const valid = await crypto.subtle.verify(
    "RSASSA-PKCS1-v1_5",
    key,
    base64UrlDecode(parts[2]),
    utf8(`${parts[0]}.${parts[1]}`),
  );
  if (!valid) throw new Error("invalid OIDC signature");

  const now = Math.floor(Date.now() / 1000);
  if (
    !claimEquals(claims, "iss", GITHUB_ISSUER) ||
    !claimEquals(claims, "aud", env.OIDC_AUDIENCE) ||
    !claimEquals(claims, "repository", env.GITHUB_REPOSITORY) ||
    !claimEquals(claims, "repository_id", env.GITHUB_REPOSITORY_ID) ||
    !claimEquals(claims, "workflow_ref", env.GITHUB_WORKFLOW_REF) ||
    !claimEquals(claims, "ref", `refs/heads/${env.GITHUB_BASE_REF}`) ||
    !["pull_request_target", "workflow_dispatch"].includes(claims?.event_name) ||
    positiveInteger(claims?.run_id) === null ||
    positiveInteger(claims?.run_attempt) === null ||
    typeof claims?.exp !== "number" ||
    typeof claims?.iat !== "number" ||
    claims.exp < now - 30 ||
    claims.iat > now + 30 ||
    claims.iat < now - 600 ||
    (typeof claims.nbf === "number" && claims.nbf > now + 30)
  ) {
    throw new Error("OIDC claims rejected");
  }
  return claims;
}

async function createSessionToken(payload, secret) {
  const encoded = base64UrlEncode(utf8(JSON.stringify(payload)));
  const signature = base64UrlEncode(await hmac(secret, utf8(`${SESSION_PREFIX}.${encoded}`)));
  return `${SESSION_PREFIX}.${encoded}.${signature}`;
}

async function verifySessionToken(token, env) {
  const parts = token.split(".");
  if (parts.length !== 3 || parts[0] !== SESSION_PREFIX) throw new Error("invalid session token");
  const expected = await hmac(env.GATEWAY_SIGNING_SECRET, utf8(`${parts[0]}.${parts[1]}`));
  const supplied = base64UrlDecode(parts[2]);
  if (!constantTimeEqual(expected, supplied)) throw new Error("invalid session signature");
  const claims = decodeJsonSegment(parts[1]);
  const now = Math.floor(Date.now() / 1000);
  const maximumTtl = SHORT_SESSION_PURPOSES.has(claims?.purpose)
    ? SHORT_SESSION_TTL_SECONDS
    : DEFAULT_SESSION_TTL_SECONDS;
  if (
    claims?.v !== 1 ||
    !SESSION_PURPOSES.has(claims?.purpose) ||
    claims?.repository_id !== env.GITHUB_REPOSITORY_ID ||
    claims?.application_uuid !== env.COOLIFY_APPLICATION_UUID ||
    !Number.isInteger(claims?.pr_number) ||
    claims.pr_number <= 0 ||
    typeof claims?.expected_commit !== "string" ||
    !FULL_COMMIT_RE.test(claims.expected_commit) ||
    claims.expected_commit !== claims.expected_commit.toLowerCase() ||
    !Number.isInteger(claims?.iat) ||
    !Number.isInteger(claims?.exp) ||
    positiveInteger(claims?.run_id) === null ||
    positiveInteger(claims?.run_attempt) === null ||
    claims.exp < now ||
    claims.iat > now + 30 ||
    claims.exp <= claims.iat ||
    claims.exp - claims.iat > maximumTtl
  ) {
    throw new Error("invalid session claims");
  }
  return claims;
}

async function handleTokenExchange(request, env) {
  if (!env.GATEWAY_SIGNING_SECRET || !env.COOLIFY_API_TOKEN) {
    return textResponse(503, "Actions gateway is not configured");
  }
  if (request.method !== "POST") return textResponse(405, "method not allowed");
  const authorization = request.headers.get("authorization") || "";
  if (!authorization.startsWith("Bearer ")) return textResponse(401, "missing OIDC token");
  let oidcClaims;
  try {
    oidcClaims = await verifyGithubOidc(authorization.slice(7), env);
  } catch {
    return textResponse(403, "OIDC identity rejected");
  }
  let payload;
  try {
    payload = decodeJson(await readRequestBytes(request, MAX_EXCHANGE_BYTES));
  } catch (error) {
    if (error instanceof Error && error.message === "body too large") {
      return textResponse(413, "exchange payload is too large");
    }
    return textResponse(400, "invalid exchange payload");
  }
  if (!payload || typeof payload !== "object" || Array.isArray(payload)) {
    return textResponse(400, "invalid exchange payload");
  }
  const applicationUuid = payload.application_uuid;
  const prNumber = payload.pr_number;
  const expectedCommit = payload.expected_commit;
  // Defaulting only at exchange keeps a rolling Worker deployment compatible
  // with the previous trusted workflow. Every issued token remains explicit.
  const purpose = payload.purpose ?? "deploy";
  if (
    applicationUuid !== env.COOLIFY_APPLICATION_UUID ||
    !Number.isInteger(prNumber) ||
    prNumber <= 0 ||
    typeof expectedCommit !== "string" ||
    !FULL_COMMIT_RE.test(expectedCommit) ||
    !SESSION_PURPOSES.has(purpose)
  ) {
    return textResponse(403, "exchange scope rejected");
  }
  const now = Math.floor(Date.now() / 1000);
  const requestedTtl = Number(env.SESSION_TTL_SECONDS || DEFAULT_SESSION_TTL_SECONDS);
  const ttl = SHORT_SESSION_PURPOSES.has(purpose)
    ? SHORT_SESSION_TTL_SECONDS
    : Math.max(300, Math.min(DEFAULT_SESSION_TTL_SECONDS, requestedTtl));
  const session = await createSessionToken(
    {
      v: 1,
      iat: now,
      exp: now + ttl,
      repository_id: env.GITHUB_REPOSITORY_ID,
      application_uuid: applicationUuid,
      pr_number: prNumber,
      expected_commit: expectedCommit.toLowerCase(),
      purpose,
      run_id: positiveInteger(oidcClaims.run_id),
      run_attempt: positiveInteger(oidcClaims.run_attempt),
    },
    env.GATEWAY_SIGNING_SECRET,
  );
  return jsonResponse(200, { token: session, expires_in: ttl });
}

function validPreviewDomain(value) {
  return (
    typeof value === "string" &&
    value === value.toLowerCase() &&
    value.length <= 253 &&
    /^(?:[a-z0-9](?:[a-z0-9-]{0,61}[a-z0-9])?\.)+[a-z0-9](?:[a-z0-9-]{0,61}[a-z0-9])?$/u.test(value)
  );
}

function configuredRepositoryId(env) {
  if (!/^[1-9][0-9]*$/u.test(env.GITHUB_REPOSITORY_ID || "")) {
    throw new Error("repository id is not configured");
  }
  const value = Number(env.GITHUB_REPOSITORY_ID);
  if (!Number.isSafeInteger(value)) throw new Error("repository id is not configured");
  return value;
}

function requireReconcileConfiguration(env) {
  if (
    !env.COOLIFY_MANUAL_WEBHOOK_SECRET ||
    !env.COOLIFY_API_TOKEN ||
    !env.ORIGIN_AUTH ||
    !env.GITHUB_REPOSITORY ||
    !env.GITHUB_BASE_REF ||
    !ARCHIVE_APPLICATION_RE.test(env.ARCHIVE_APPLICATION_NAME || "") ||
    !validPreviewDomain(env.PREVIEW_DOMAIN)
  ) {
    throw new Error("reconcile is not configured");
  }
  configuredRepositoryId(env);
}

function syntheticPullRequest(claims, env) {
  const repositoryId = configuredRepositoryId(env);
  const repository = {
    id: repositoryId,
    full_name: env.GITHUB_REPOSITORY,
    fork: false,
  };
  const htmlUrl = `https://github.com/${env.GITHUB_REPOSITORY}/pull/${claims.pr_number}`;
  return {
    // `synchronize` with no `before` value makes Coolify use the exact signed
    // commit without an auxiliary GitHub API call. It still creates a missing
    // ApplicationPreview in v4.1.2 and is safe because preflight requires an
    // empty watch-path filter.
    action: "synchronize",
    number: claims.pr_number,
    after: claims.expected_commit,
    repository,
    sender: { login: "github-actions[bot]", type: "Bot" },
    pull_request: {
      number: claims.pr_number,
      state: "open",
      draft: false,
      html_url: htmlUrl,
      title: "GitHub Actions preview reconciliation",
      author_association: "OWNER",
      user: { login: "github-actions[bot]", type: "Bot" },
      head: {
        ref: `actions/pr-${claims.pr_number}`,
        sha: claims.expected_commit,
        repo: repository,
      },
      base: {
        ref: env.GITHUB_BASE_REF,
        repo: repository,
      },
    },
  };
}

function integerPr(value, { missingIsBranch = false } = {}) {
  if (value === undefined || value === null || value === "") {
    return missingIsBranch ? 0 : null;
  }
  if (typeof value === "boolean") return null;
  if (typeof value === "number") {
    return Number.isSafeInteger(value) && value >= 0 ? value : null;
  }
  if (typeof value === "string" && /^(0|[1-9][0-9]*)$/u.test(value)) {
    const parsed = Number(value);
    return Number.isSafeInteger(parsed) ? parsed : null;
  }
  return null;
}

function recordPr(record, { missingIsBranch = false } = {}) {
  const values = [record.pull_request_id, record.pr].filter(
    (value) => value !== undefined && value !== null && value !== "",
  );
  if (values.length === 0) return missingIsBranch ? 0 : null;
  const parsed = values.map((value) => integerPr(value));
  if (parsed.some((value) => value === null) || new Set(parsed).size !== 1) return null;
  return parsed[0];
}

function stringAlias(record, names, { lowercase = false } = {}) {
  const values = names
    .map((name) => record[name])
    .filter((value) => value !== undefined && value !== null && value !== "");
  if (values.length === 0) return undefined;
  if (values.some((value) => typeof value !== "string")) return null;
  const normalized = values.map((value) => lowercase ? value.toLowerCase() : value);
  return new Set(normalized).size === 1 ? normalized[0] : null;
}

function historyRows(payload, claims, seen = new Set()) {
  if (!payload || typeof payload !== "object" || Array.isArray(payload)) {
    throw new Error("malformed deployment history");
  }
  if (!Array.isArray(payload.deployments)) throw new Error("malformed deployment history");
  const rows = [];
  for (const raw of payload.deployments) {
    if (!raw || typeof raw !== "object" || Array.isArray(raw)) {
      throw new Error("malformed deployment history");
    }
    const deploymentUuid = raw.deployment_uuid;
    if (typeof deploymentUuid !== "string" || !IDENTIFIER_RE.test(deploymentUuid)) {
      throw new Error("malformed deployment history");
    }
    if (seen.has(deploymentUuid)) throw new Error("malformed deployment history");
    seen.add(deploymentUuid);
    const applicationUuid = stringAlias(raw, ["application_uuid", "resource_uuid"]);
    if (
      applicationUuid === null ||
      (applicationUuid !== undefined && applicationUuid !== claims.application_uuid)
    ) {
      throw new Error("malformed deployment history");
    }
    const prNumber = recordPr(raw, { missingIsBranch: true });
    if (prNumber === null) throw new Error("malformed deployment history");
    if (prNumber !== claims.pr_number) continue;
    const commit = stringAlias(raw, ["commit", "commit_sha", "git_commit_sha"], {
      lowercase: true,
    });
    const statusValue = raw.status;
    if (
      typeof commit !== "string" ||
      !FULL_COMMIT_RE.test(commit) ||
      typeof statusValue !== "string"
    ) {
      throw new Error("malformed deployment history");
    }
    const status = statusValue.trim().toLowerCase();
    if (!STATUS_RE.test(status)) throw new Error("malformed deployment history");
    const createdAt = raw.created_at ?? raw.createdAt ?? null;
    const updatedAt = raw.updated_at ?? raw.updatedAt ?? null;
    if (
      (createdAt !== null && (typeof createdAt !== "string" || createdAt.length > 64)) ||
      (updatedAt !== null && (typeof updatedAt !== "string" || updatedAt.length > 64))
    ) {
      throw new Error("malformed deployment history");
    }
    rows.push({
      deployment_uuid: deploymentUuid,
      commit,
      pull_request_id: prNumber,
      status,
      created_at: createdAt,
      updated_at: updatedAt,
      deployment_url: safeDeploymentUrl(raw.deployment_url ?? raw.url),
    });
  }
  return rows;
}

async function coolifyHistory(claims, env) {
  const application = encodeURIComponent(claims.application_uuid);
  const url = new URL(`/api/v1/deployments/applications/${application}`, ORIGIN);
  const rows = [];
  const seen = new Set();
  for (
    let skip = 0;
    skip < MAX_COOLIFY_HISTORY_ROWS;
    skip += COOLIFY_HISTORY_PAGE_SIZE
  ) {
    url.searchParams.set("skip", String(skip));
    url.searchParams.set("take", String(COOLIFY_HISTORY_PAGE_SIZE));
    const response = await fetch(new Request(url, {
      method: "GET",
      redirect: "manual",
      headers: {
        accept: "application/json",
        authorization: `Bearer ${env.COOLIFY_API_TOKEN}`,
        "x-proa-control-plane": env.ORIGIN_AUTH,
        "x-forwarded-host": "coolify.proa.so",
        "x-forwarded-proto": "https",
      },
    }));
    if (response.status !== 200) {
      await cancelResponseBody(response);
      throw new Error("deployment history unavailable");
    }
    let responseBytes;
    try {
      responseBytes = await readResponseBytes(response, MAX_COOLIFY_HISTORY_BYTES);
    } catch (error) {
      cancelResponseBody(response);
      if (error instanceof Error && error.message === "body too large") {
        throw new DeploymentHistoryUnavailableError("history_payload_too_large");
      }
      throw error;
    }
    const payload = decodeJson(responseBytes);
    if (
      !isPlainObject(payload) ||
      !Array.isArray(payload.deployments) ||
      payload.deployments.length > COOLIFY_HISTORY_PAGE_SIZE
    ) {
      throw new Error("malformed deployment history");
    }
    const page = historyRows(payload, claims, seen);
    rows.push(...page);
    if (payload.deployments.length < COOLIFY_HISTORY_PAGE_SIZE) break;
  }
  return rows;
}

async function coolifyDeploymentDetail(claims, env, deploymentUuid) {
  const deployment = encodeURIComponent(deploymentUuid);
  const url = new URL(`/api/v1/deployments/${deployment}`, ORIGIN);
  let response;
  try {
    response = await fetch(new Request(url, {
      method: "GET",
      redirect: "manual",
      headers: {
        accept: "application/json",
        authorization: `Bearer ${env.COOLIFY_API_TOKEN}`,
        "x-proa-actions-run": String(claims.run_id || "unknown"),
        "x-proa-control-plane": env.ORIGIN_AUTH,
        "x-forwarded-host": "coolify.proa.so",
        "x-forwarded-proto": "https",
      },
    }));
    if (response.status !== 200) {
      cancelResponseBody(response);
      throw new DeploymentDetailUnavailableError("deployment detail unavailable");
    }
    const payload = decodeJson(
      await readResponseBytes(response, MAX_COOLIFY_JSON_BYTES),
    );
    return projectDeploymentDetail(payload, deploymentUuid, claims);
  } catch (error) {
    cancelResponseBody(response);
    if (error instanceof DeploymentDetailScopeError) throw error;
    throw new DeploymentDetailUnavailableError("deployment detail unavailable");
  }
}

async function readyHostnameMatches(hostname, expectedCommit) {
  let response;
  try {
    response = await fetch(new Request(`https://${hostname}/readyz`, {
      method: "GET",
      redirect: "manual",
      headers: { accept: "text/plain" },
    }));
    if (
      response.status !== 200 ||
      response.headers.get("x-proa-revision")?.toLowerCase() !== expectedCommit
    ) {
      await cancelResponseBody(response);
      return false;
    }
    const body = new TextDecoder("utf-8", { fatal: true }).decode(
      await readResponseBytes(response, MAX_READY_BYTES),
    );
    return body === "ready";
  } catch {
    await cancelResponseBody(response);
    return false;
  }
}

async function reusablePreviewMatches(claims, env, deploymentUuid) {
  if (!ARCHIVE_DEPLOYMENT_RE.test(deploymentUuid)) return false;
  const stableHostname = `pr-${claims.pr_number}.${env.PREVIEW_DOMAIN}`;
  const deploymentHostname = `${env.ARCHIVE_APPLICATION_NAME}-${deploymentUuid.slice(0, 12)}.${env.PREVIEW_DOMAIN}`;
  const [stableReady, deploymentReady] = await Promise.all([
    readyHostnameMatches(stableHostname, claims.expected_commit),
    readyHostnameMatches(deploymentHostname, claims.expected_commit),
  ]);
  return stableReady && deploymentReady;
}

function activeDeploymentFresh(deployment, now = Date.now()) {
  const createdAt = deployment.created_at;
  if (
    typeof createdAt !== "string" ||
    !/^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(?:\.\d{1,6})?Z$/u.test(createdAt)
  ) {
    return false;
  }
  const createdEpoch = Date.parse(createdAt);
  if (!Number.isFinite(createdEpoch)) return false;
  const age = now - createdEpoch;
  return age >= -MAX_TIMESTAMP_FUTURE_SKEW_MS && age <= MAX_ACTIVE_REUSE_AGE_MS;
}

async function syntheticWebhookAccepted(response) {
  if (response.status < 200 || response.status >= 300) {
    console.error(`coolify_synthetic_webhook_rejected status=${response.status}`);
    await cancelResponseBody(response);
    return false;
  }
  try {
    const payload = decodeJson(await readResponseBytes(response, 64 * 1024));
    if (!Array.isArray(payload) || payload.length === 0) return false;
    const queued = payload.filter(
      (item) => item && typeof item === "object" && item.status === "queued",
    );
    // A per-application secret should authenticate exactly one matching app.
    // More than one queued row means that secret was duplicated and the
    // mutation boundary is no longer unambiguous.
    return queued.length === 1;
  } catch {
    console.error("coolify_synthetic_webhook_rejected response=malformed");
    await cancelResponseBody(response);
    return false;
  }
}

async function triggerSyntheticWebhook(request, claims, env) {
  const payload = syntheticPullRequest(claims, env);
  const body = utf8(JSON.stringify(payload));
  // Coolify's manual GitHub webhook authenticates independently against each
  // matching application's own secret before dispatching its PR job. This is
  // the application-scoped creation path that the GitHub App webhook lacks.
  const signature = bytesToHex(await hmac(env.COOLIFY_MANUAL_WEBHOOK_SECRET, body));
  const publicUrl = new URL(request.url);
  const url = new URL("/webhooks/source/github/events/manual", ORIGIN);
  const headers = new Headers({
    accept: "application/json",
    "content-type": "application/json",
    "user-agent": "GitHub-Hookshot/proa-actions",
    "x-github-delivery": crypto.randomUUID(),
    "x-github-event": "pull_request",
    "x-hub-signature-256": `sha256=${signature}`,
    "x-proa-actions-run": String(claims.run_id || "unknown"),
    "x-proa-control-plane": env.ORIGIN_AUTH,
    "x-proa-preview-policy": "deploy",
    "x-forwarded-host": publicUrl.host,
    "x-forwarded-proto": "https",
  });
  // Build the signed mutation as a direct, redirect-disabled origin request.
  // This keeps its origin credential on exactly one hop and avoids routing a
  // server-to-server webhook through the public response-rewriting path.
  const response = await fetch(new Request(url, {
    method: "POST",
    headers,
    body,
    redirect: "manual",
  }));
  return syntheticWebhookAccepted(response);
}

async function handleReconcile(request, env, token) {
  if (request.method !== "POST") return textResponse(405, "method not allowed");
  let claims;
  try {
    claims = await verifySessionToken(token, env);
  } catch {
    return textResponse(403, "Actions session rejected");
  }
  if (claims.purpose !== "deploy") {
    return textResponse(403, "Actions session purpose rejected");
  }
  try {
    requireReconcileConfiguration(env);
    const body = await readRequestBytes(request, 0);
    if (body.byteLength !== 0) throw new Error("reconcile body is not allowed");
  } catch {
    return jsonResponse(400, {
      result: "error",
      reason: "invalid_request",
      mutation: "not_attempted",
    });
  }

  const key = `${claims.application_uuid}:${claims.pr_number}:${claims.expected_commit}`;
  const now = Date.now();
  for (const [candidate, entry] of reconcileFlights) {
    if (entry.expiresAt <= now) reconcileFlights.delete(candidate);
  }
  const existing = reconcileFlights.get(key);
  if (existing) {
    const result = await existing.promise;
    return jsonResponse(result.status, result.payload);
  }
  if (reconcileFlights.size >= 256) {
    reconcileFlights.delete(reconcileFlights.keys().next().value);
  }
  const entry = {
    expiresAt: Number.POSITIVE_INFINITY,
    promise: performReconcile(request, claims, env),
  };
  reconcileFlights.set(key, entry);
  const result = await entry.promise;
  if (result.status === 200) {
    entry.expiresAt = Date.now() + 30_000;
  } else {
    reconcileFlights.delete(key);
  }
  return jsonResponse(result.status, result.payload);
}

function exactObjectKeys(payload, expected) {
  if (!payload || typeof payload !== "object" || Array.isArray(payload)) return false;
  const actual = Object.keys(payload).sort();
  const wanted = [...expected].sort();
  return actual.length === wanted.length && actual.every((key, index) => key === wanted[index]);
}

function archiveHostPayload(payload, claims, issuedAt) {
  if (
    !exactObjectKeys(payload, ["deployment_uuid", "operation"]) ||
    !ARCHIVE_OPERATIONS.has(payload.operation) ||
    typeof payload.deployment_uuid !== "string" ||
    !ARCHIVE_DEPLOYMENT_RE.test(payload.deployment_uuid)
  ) {
    throw new Error("invalid archive request");
  }
  const runId = positiveInteger(claims.run_id);
  const runAttempt = positiveInteger(claims.run_attempt);
  if (runId === null || runAttempt === null) throw new Error("invalid archive session audit");
  return {
    v: 1,
    operation: payload.operation,
    application_uuid: claims.application_uuid,
    pr_number: claims.pr_number,
    expected_commit: claims.expected_commit,
    deployment_uuid: payload.deployment_uuid,
    run_id: runId,
    run_attempt: runAttempt,
    issued_at: issuedAt,
  };
}

function archiveControlResult(payload, operation, deploymentUuid) {
  const expectedResult = operation === "promote" ? "promoted" : "revoked";
  const allowedKeys = new Set(["deployment_uuid", "ok", "operation", "result"]);
  if (
    !payload ||
    typeof payload !== "object" ||
    Array.isArray(payload) ||
    Object.keys(payload).some((key) => !allowedKeys.has(key)) ||
    payload.result !== expectedResult ||
    payload.deployment_uuid !== deploymentUuid ||
    (Object.hasOwn(payload, "ok") && payload.ok !== true) ||
    (Object.hasOwn(payload, "operation") && payload.operation !== operation)
  ) {
    throw new Error("archive response scope rejected");
  }
  return { result: expectedResult, deployment_uuid: deploymentUuid };
}

async function handleArchiveControl(request, env, token) {
  if (request.method !== "POST") return textResponse(405, "method not allowed");
  if (!/^[0-9a-f]{64}$/u.test(env.ARCHIVE_CONTROL_SECRET || "")) {
    return textResponse(503, "archive control is not configured");
  }
  let claims;
  try {
    claims = await verifySessionToken(token, env);
  } catch {
    return textResponse(403, "Actions session rejected");
  }
  if (claims.purpose !== "archive") {
    return textResponse(403, "Actions session purpose rejected");
  }
  if ((request.headers.get("content-type") || "").split(";", 1)[0].trim().toLowerCase() !== "application/json") {
    return jsonResponse(400, { result: "error", reason: "invalid_request" });
  }

  let clientPayload;
  try {
    clientPayload = decodeJson(await readRequestBytes(request, MAX_ARCHIVE_REQUEST_BYTES));
  } catch (error) {
    if (error instanceof Error && error.message === "body too large") {
      return jsonResponse(413, { result: "error", reason: "request_too_large" });
    }
    return jsonResponse(400, { result: "error", reason: "invalid_request" });
  }

  const issuedAt = Math.floor(Date.now() / 1000);
  let hostPayload;
  try {
    hostPayload = archiveHostPayload(clientPayload, claims, issuedAt);
  } catch {
    return jsonResponse(400, { result: "error", reason: "invalid_request" });
  }
  let deployment;
  try {
    deployment = await coolifyDeploymentDetail(
      claims,
      env,
      hostPayload.deployment_uuid,
    );
  } catch (error) {
    if (error instanceof DeploymentDetailScopeError) {
      return jsonResponse(409, { result: "error", reason: "deployment_scope_rejected" });
    }
    return jsonResponse(502, { result: "error", reason: "detail_unavailable" });
  }
  if (deployment.commit !== claims.expected_commit) {
    return jsonResponse(409, { result: "error", reason: "deployment_scope_rejected" });
  }
  if (
    hostPayload.operation === "promote" &&
    !SUCCESS_DEPLOYMENT_STATUSES.has(deployment.status)
  ) {
    return jsonResponse(409, { result: "error", reason: "deployment_not_ready" });
  }
  const body = utf8(JSON.stringify(hostPayload));
  const signature = await archiveOriginSignature(env.ARCHIVE_CONTROL_SECRET, issuedAt, body);
  const url = new URL(ARCHIVE_ORIGIN_PATH, ORIGIN);
  let response;
  try {
    response = await fetch(new Request(url, {
      method: "POST",
      redirect: "manual",
      headers: {
        accept: "application/json",
        "content-type": "application/json",
        "user-agent": "proa-coolify-control/1",
        "x-proa-archive-signature": `sha256=${signature}`,
        "x-proa-archive-timestamp": String(issuedAt),
        "x-proa-control-plane": env.ORIGIN_AUTH,
        "x-forwarded-host": "coolify.proa.so",
        "x-forwarded-proto": "https",
      },
      body,
    }));
    if (response.status !== 200) {
      const status = response.status >= 500 ? 502 : response.status;
      await cancelResponseBody(response);
      return jsonResponse(status, { result: "error", reason: "archive_control_rejected" });
    }
    const payload = decodeJson(await readResponseBytes(response, MAX_ARCHIVE_RESPONSE_BYTES));
    return jsonResponse(
      200,
      archiveControlResult(payload, hostPayload.operation, hostPayload.deployment_uuid),
    );
  } catch {
    await cancelResponseBody(response);
    return jsonResponse(502, { result: "error", reason: "archive_control_unavailable" });
  }
}

async function performReconcile(request, claims, env) {
  let rows;
  try {
    rows = await coolifyHistory(claims, env);
  } catch (error) {
    return {
      status: 502,
      payload: {
        result: "error",
        reason: historyUnavailableReason(error),
        mutation: "not_attempted",
      },
    };
  }
  const newestExact = rows.find((row) => row.commit === claims.expected_commit);
  if (
    newestExact &&
    ACTIVE_DEPLOYMENT_STATUSES.has(newestExact.status) &&
    activeDeploymentFresh(newestExact)
  ) {
    return {
      status: 200,
      payload: {
        result: "reused",
        state: "active",
        deployment_uuid: newestExact.deployment_uuid,
      },
    };
  }
  if (
    newestExact &&
    SUCCESS_DEPLOYMENT_STATUSES.has(newestExact.status) &&
    await reusablePreviewMatches(claims, env, newestExact.deployment_uuid)
  ) {
    return {
      status: 200,
      payload: {
        result: "reused",
        state: "ready",
        deployment_uuid: newestExact.deployment_uuid,
      },
    };
  }

  try {
    if (!(await triggerSyntheticWebhook(request, claims, env))) {
      return { status: 502, payload: { result: "error", reason: "webhook_rejected" } };
    }
  } catch (error) {
    console.error(
      `coolify_synthetic_webhook_unavailable error=${
        error instanceof Error ? error.name : "unknown"
      }`,
    );
    return { status: 502, payload: { result: "error", reason: "webhook_unavailable" } };
  }
  return {
    status: 200,
    payload: {
      result: "queued",
      baseline_deployment_uuids: rows
        .filter((row) => row.commit === claims.expected_commit)
        .map((row) => row.deployment_uuid),
    },
  };
}

function safeDeploymentUrl(value) {
  if (value === undefined || value === null || value === "") return null;
  if (typeof value !== "string" || value.length > 2_048 || /[\u0000-\u0020<>]/u.test(value)) {
    return null;
  }
  let parsed;
  try {
    parsed = new URL(value);
  } catch {
    return null;
  }
  if (
    !["http:", "https:"].includes(parsed.protocol) ||
    !parsed.hostname ||
    parsed.username ||
    parsed.password
  ) {
    return null;
  }
  return parsed.toString();
}

function projectDeploymentDetail(payload, requestedUuid, claims) {
  if (!payload || typeof payload !== "object" || Array.isArray(payload)) {
    throw new Error("deployment detail malformed");
  }
  const deploymentUuid = payload.deployment_uuid;
  const applicationUuid = stringAlias(
    { nested: payload.application?.uuid, direct: payload.application_uuid },
    ["nested", "direct"],
  );
  const prNumber = recordPr(payload);
  const statusValue = payload.status;
  const commitValue = stringAlias(payload, ["commit", "commit_sha", "git_commit_sha"], {
    lowercase: true,
  });
  if (
    deploymentUuid !== requestedUuid ||
    applicationUuid !== claims.application_uuid ||
    prNumber !== claims.pr_number
  ) {
    throw new DeploymentDetailScopeError("deployment detail scope rejected");
  }
  if (
    !IDENTIFIER_RE.test(deploymentUuid || "") ||
    typeof statusValue !== "string" ||
    typeof commitValue !== "string" ||
    !FULL_COMMIT_RE.test(commitValue)
  ) {
    throw new Error("deployment detail malformed");
  }
  const status = statusValue.trim().toLowerCase();
  if (!STATUS_RE.test(status)) throw new Error("deployment detail malformed");
  return {
    deployment_uuid: deploymentUuid,
    status,
    deployment_url: safeDeploymentUrl(payload.deployment_url ?? payload.url),
    commit: commitValue,
    pull_request_id: prNumber,
    application: { uuid: applicationUuid },
  };
}

function deploymentDetailUuid(request) {
  if (request.method !== "GET") return null;
  const match = /^\/api\/v1\/deployments\/([A-Za-z0-9._-]{1,128})$/u.exec(
    new URL(request.url).pathname,
  );
  return match?.[1] || null;
}

function safeScalar(value) {
  if (value === null || typeof value === "boolean") return value;
  if (typeof value === "number" && Number.isFinite(value)) return value;
  if (typeof value === "string" && value.length <= 4_096 && !/[\u0000-\u001F\u007F]/u.test(value)) {
    return value;
  }
  throw new Error("unsafe upstream field");
}

function copyScalars(payload, fields) {
  const projected = {};
  for (const field of fields) {
    if (Object.hasOwn(payload, field)) projected[field] = safeScalar(payload[field]);
  }
  return projected;
}

function projectApplication(payload, claims, env) {
  if (!payload || typeof payload !== "object" || Array.isArray(payload)) {
    throw new Error("application response malformed");
  }
  const projected = copyScalars(payload, [
    "uuid",
    "git_repository",
    "git_branch",
    "fqdn",
    "preview_url_template",
    "is_preview_deployments_enabled",
    "is_pr_deployments_public_enabled",
    "is_force_https_enabled",
    "is_auto_deploy_enabled",
    "build_pack",
    "base_directory",
    "dockerfile_location",
    "ports_exposes",
    "health_check_enabled",
    "health_check_path",
    "health_check_port",
    "watch_paths",
    "limits_memory",
    "limits_memory_swap",
    "limits_memory_reservation",
    "limits_cpus",
    "limits_pids",
  ]);
  if (payload.manual_webhook_secret_github !== env.COOLIFY_MANUAL_WEBHOOK_SECRET) {
    throw new Error("application manual webhook binding rejected");
  }
  if (
    Object.hasOwn(payload, "custom_docker_run_options") &&
    [null, "", "--pids-limit=256"].includes(payload.custom_docker_run_options)
  ) {
    projected.custom_docker_run_options = payload.custom_docker_run_options;
  }
  if (payload.uuid !== claims.application_uuid) throw new Error("application scope rejected");
  if (typeof projected.git_repository === "string" && projected.git_repository.includes("://")) {
    let repositoryUrl;
    try {
      repositoryUrl = new URL(projected.git_repository);
    } catch {
      throw new Error("application response malformed");
    }
    if (repositoryUrl.username || repositoryUrl.password) {
      throw new Error("application response malformed");
    }
  }
  if (Object.hasOwn(payload, "ports_mappings")) {
    if (Array.isArray(payload.ports_mappings) && payload.ports_mappings.length === 0) {
      projected.ports_mappings = [];
    } else {
      projected.ports_mappings = safeScalar(payload.ports_mappings);
    }
  }
  if (Object.hasOwn(payload, "destination")) {
    if (!payload.destination || typeof payload.destination !== "object" || Array.isArray(payload.destination)) {
      throw new Error("application response malformed");
    }
    projected.destination = { network: safeScalar(payload.destination.network) };
  }
  if (Object.hasOwn(payload, "settings")) {
    if (!payload.settings || typeof payload.settings !== "object" || Array.isArray(payload.settings)) {
      throw new Error("application response malformed");
    }
    projected.settings = copyScalars(payload.settings, [
      "is_preview_deployments_enabled",
      "is_pr_deployments_public_enabled",
      "is_force_https_enabled",
      "is_auto_deploy_enabled",
    ]);
  }
  return projected;
}

function projectEnvironment(payload) {
  if (!Array.isArray(payload)) throw new Error("environment response malformed");
  return payload.map((variable) => {
    if (!variable || typeof variable !== "object" || Array.isArray(variable)) {
      throw new Error("environment response malformed");
    }
    return copyScalars(variable, [
      "key",
      "is_buildtime",
      "is_runtime",
      "is_preview",
      "is_shared",
      "is_coolify",
      "is_literal",
      "is_multiline",
    ]);
  });
}

function projectStorages(payload) {
  if (!payload || typeof payload !== "object" || Array.isArray(payload)) {
    throw new Error("storage response malformed");
  }
  const projectList = (value) => {
    if (!Array.isArray(value)) throw new Error("storage response malformed");
    return value.map((storage) => {
      if (!storage || typeof storage !== "object" || Array.isArray(storage)) {
        throw new Error("storage response malformed");
      }
      // Preflight needs only the presence/count of storage records. Exposing
      // their host or mount paths would reveal control-plane topology without
      // improving attestation.
      return {};
    });
  };
  return {
    persistent_storages: projectList(payload.persistent_storages),
    file_storages: projectList(payload.file_storages),
  };
}

async function projectedUpstreamJson(request, env, headers, projector) {
  let upstream;
  try {
    upstream = await proxyToOrigin(request, env, { headers });
    if (upstream.status !== 200) {
      await cancelResponseBody(upstream);
      return jsonResponse(502, { result: "error", reason: "upstream_unavailable" });
    }
    const payload = decodeJson(await readResponseBytes(upstream, MAX_COOLIFY_JSON_BYTES));
    return jsonResponse(200, projector(payload));
  } catch {
    await cancelResponseBody(upstream);
    return textResponse(403, "Coolify API response scope rejected");
  }
}

function authorizedApiPath(request, claims) {
  const url = new URL(request.url);
  const path = url.pathname;
  const app = claims.application_uuid;
  const pr = String(claims.pr_number);
  const previewDelete =
    request.method === "DELETE" && path === `/api/v1/applications/${app}/previews/${pr}`;
  if (claims.purpose === "teardown") return previewDelete;
  if (claims.purpose !== "deploy") return false;
  if (request.method === "GET") {
    if (path === `/api/v1/applications/${app}`) return true;
    if (path === `/api/v1/applications/${app}/envs`) return true;
    if (path === `/api/v1/applications/${app}/storages`) return true;
    if (path === `/api/v1/deployments/applications/${app}`) return true;
    if (/^\/api\/v1\/deployments\/[A-Za-z0-9_-]+$/u.test(path)) return true;
  }
  return previewDelete;
}

async function handleApiGateway(request, env, token) {
  let claims;
  try {
    claims = await verifySessionToken(token, env);
  } catch {
    return textResponse(403, "Actions session rejected");
  }
  if (!authorizedApiPath(request, claims)) return textResponse(403, "Coolify API scope rejected");
  const headers = new Headers(request.headers);
  headers.set("authorization", `Bearer ${env.COOLIFY_API_TOKEN}`);
  headers.set("x-proa-actions-run", String(claims.run_id || "unknown"));
  const path = new URL(request.url).pathname;
  const applicationPath = `/api/v1/applications/${claims.application_uuid}`;
  if (request.method === "GET" && path === applicationPath) {
    return projectedUpstreamJson(
      request,
      env,
      headers,
      (payload) => projectApplication(payload, claims, env),
    );
  }
  if (request.method === "GET" && path === `${applicationPath}/envs`) {
    return projectedUpstreamJson(request, env, headers, projectEnvironment);
  }
  if (request.method === "GET" && path === `${applicationPath}/storages`) {
    return projectedUpstreamJson(request, env, headers, projectStorages);
  }
  if (request.method === "GET" && path === `/api/v1/deployments/applications/${claims.application_uuid}`) {
    try {
      const rows = await coolifyHistory(claims, env);
      return jsonResponse(200, { count: rows.length, deployments: rows });
    } catch (error) {
      return jsonResponse(502, {
        result: "error",
        reason: historyUnavailableReason(error),
      });
    }
  }
  const detailUuid = deploymentDetailUuid(request);
  if (detailUuid !== null) {
    try {
      return jsonResponse(
        200,
        await coolifyDeploymentDetail(claims, env, detailUuid),
      );
    } catch (error) {
      if (!(error instanceof DeploymentDetailScopeError)) {
        return jsonResponse(502, { result: "error", reason: "detail_unavailable" });
      }
      return textResponse(403, "Coolify deployment detail scope rejected");
    }
  }
  if (request.method === "DELETE") {
    if (claims.purpose === "deploy") {
      let rows;
      try {
        rows = await coolifyHistory(claims, env);
      } catch (error) {
        return jsonResponse(502, {
          result: "error",
          reason: historyUnavailableReason(error),
        });
      }
      const newest = rows[0];
      if (!newest) return new Response(null, { status: 404 });
      if (newest.commit !== claims.expected_commit) {
        return new Response(null, { status: 412 });
      }
    }

    let upstream;
    try {
      upstream = await proxyToOrigin(request, env, { headers });
      await cancelResponseBody(upstream);
      if ([200, 204, 404].includes(upstream.status)) {
        return new Response(null, { status: upstream.status });
      }
      return jsonResponse(upstream.status, { result: "error" });
    } catch {
      await cancelResponseBody(upstream);
      return jsonResponse(502, { result: "error", reason: "upstream_unavailable" });
    }
  }
  return textResponse(403, "Coolify API scope rejected");
}

export default {
  async fetch(request, env) {
    const url = new URL(request.url);
    if (url.protocol !== "https:") {
      url.protocol = "https:";
      return new Response(null, {
        status: 308,
        headers: { location: url.toString() },
      });
    }
    // This path exists only on the origin loopback boundary. Normal public
    // proxying injects the origin credential, so deny it before any fallback
    // can make a caller look like the Worker itself.
    if (url.pathname.startsWith("/_proa/origin/")) {
      return textResponse(404, "not found");
    }
    if (!env.ORIGIN_AUTH) return textResponse(503, "control-plane proxy is not configured");
    if (url.pathname === "/webhooks/source/github/events") {
      return handleGithubWebhook(request, env);
    }
    if (url.pathname === "/_proa/github-actions/token") {
      return handleTokenExchange(request, env);
    }
    const authorization = request.headers.get("authorization") || "";
    if (url.pathname === PRESENTATION_TOKEN_PATH) {
      if (!authorization.startsWith(`Bearer ${SESSION_PREFIX}.`)) {
        return textResponse(401, "missing Actions session");
      }
      return handlePresentationToken(request, env, authorization.slice(7));
    }
    if (url.pathname === "/_proa/github-actions/reconcile") {
      if (!authorization.startsWith(`Bearer ${SESSION_PREFIX}.`)) {
        return textResponse(401, "missing Actions session");
      }
      return handleReconcile(request, env, authorization.slice(7));
    }
    if (url.pathname === "/_proa/github-actions/archive") {
      if (!authorization.startsWith(`Bearer ${SESSION_PREFIX}.`)) {
        return textResponse(401, "missing Actions session");
      }
      return handleArchiveControl(request, env, authorization.slice(7));
    }
    if (url.pathname.startsWith("/api/v1/") && authorization.startsWith(`Bearer ${SESSION_PREFIX}.`)) {
      return handleApiGateway(request, env, authorization.slice(7));
    }
    return proxyToOrigin(request, env);
  },
};

export {
  archiveHostPayload,
  archiveOriginSignature,
  authorizedApiPath,
  base64UrlDecode,
  base64UrlEncode,
  createSessionToken,
  githubAppJwt,
  historyRows,
  projectPresentationToken,
  projectDeploymentDetail,
  responseHeadersForPublicOrigin,
  syntheticPullRequest,
  verifySessionToken,
  verifyGithubOidc,
  verifyWebhookSignature,
};
