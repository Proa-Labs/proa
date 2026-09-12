// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";

import {
  archiveHostPayload,
  archiveOriginSignature,
  authorizedApiPath,
  base64UrlEncode,
  createSessionToken,
  historyRows,
  projectDeploymentDetail,
  projectPresentationToken,
  syntheticPullRequest,
  verifySessionToken,
  verifyGithubOidc,
  verifyWebhookSignature,
} from "./worker.js";
import worker from "./worker.js";

const COMMIT = "abcdef1234567890abcdef1234567890abcdef12";
const OTHER_COMMIT = "1111111111111111111111111111111111111111";
const DEPLOYMENT = "dy2vxce7udvt9q2otsiwb6o1";

const env = {
  ARCHIVE_APPLICATION_NAME: "proa-marketing",
  ARCHIVE_CONTROL_SECRET: "a".repeat(64),
  COOLIFY_APPLICATION_UUID: "app-123",
  COOLIFY_API_TOKEN: "root-coolify-token",
  COOLIFY_MANUAL_WEBHOOK_SECRET: "application-manual-webhook-secret",
  GATEWAY_SIGNING_SECRET: "test-signing-secret-with-enough-entropy",
  GITHUB_APP_ID: "4531960",
  GITHUB_BASE_REF: "main",
  GITHUB_REPOSITORY: "Proa-Labs/proa",
  GITHUB_REPOSITORY_ID: "12345",
  GITHUB_WEBHOOK_SECRET: "webhook-secret",
  ORIGIN_AUTH: "origin-auth-secret",
  PREVIEW_DOMAIN: "proa.cool",
};

test("source webhooks and presentation use distinct verified GitHub Apps", () => {
  const wrangler = readFileSync(new URL("./wrangler.toml", import.meta.url), "utf8");
  const configured = wrangler.match(/^GITHUB_APP_ID = "([0-9]+)"$/m);
  assert.ok(configured);
  assert.equal(configured[1], env.GITHUB_APP_ID);
  assert.equal(configured[1], "4531960");
  assert.notEqual(configured[1], "4541370");
});

function claimsFor(prNumber = 14, commit = COMMIT) {
  const now = Math.floor(Date.now() / 1000);
  return {
    v: 1,
    iat: now,
    exp: now + 600,
    repository_id: env.GITHUB_REPOSITORY_ID,
    application_uuid: env.COOLIFY_APPLICATION_UUID,
    pr_number: prNumber,
    expected_commit: commit,
    purpose: "deploy",
    run_id: 10_000 + prNumber,
    run_attempt: 1,
  };
}

function teardownClaimsFor(prNumber = 14, commit = COMMIT) {
  const claims = claimsFor(prNumber, commit);
  return {
    ...claims,
    purpose: "teardown",
    exp: claims.iat + 300,
  };
}

function archiveClaimsFor(prNumber = 14, commit = COMMIT) {
  const claims = claimsFor(prNumber, commit);
  return {
    ...claims,
    purpose: "archive",
    exp: claims.iat + 300,
  };
}

function presentationClaimsFor(prNumber = 14, commit = COMMIT) {
  const claims = claimsFor(prNumber, commit);
  return {
    ...claims,
    purpose: "presentation",
    exp: claims.iat + 300,
  };
}

async function sessionFor(claims = claimsFor()) {
  return createSessionToken(claims, env.GATEWAY_SIGNING_SECRET);
}

async function signedRequest(body, event = "pull_request", secret = env.GITHUB_WEBHOOK_SECRET) {
  const bytes = new TextEncoder().encode(body);
  const key = await crypto.subtle.importKey(
    "raw",
    new TextEncoder().encode(secret),
    { name: "HMAC", hash: "SHA-256" },
    false,
    ["sign"],
  );
  const signature = [...new Uint8Array(await crypto.subtle.sign("HMAC", key, bytes))]
    .map((byte) => byte.toString(16).padStart(2, "0"))
    .join("");
  return new Request("https://coolify.proa.so/webhooks/source/github/events", {
    method: "POST",
    headers: {
      "content-type": "application/json",
      "x-github-delivery": crypto.randomUUID(),
      "x-github-event": event,
      "x-github-hook-installation-target-id": env.GITHUB_APP_ID,
      "x-hub-signature-256": `sha256=${signature}`,
    },
    body,
  });
}

async function signedOidc(claims, privateKey, headerOverrides = {}) {
  const header = base64UrlEncode(
    new TextEncoder().encode(JSON.stringify({
      alg: "RS256",
      kid: "test-key",
      typ: "JWT",
      ...headerOverrides,
    })),
  );
  const payload = base64UrlEncode(new TextEncoder().encode(JSON.stringify(claims)));
  const signingInput = `${header}.${payload}`;
  const signature = new Uint8Array(
    await crypto.subtle.sign(
      "RSASSA-PKCS1-v1_5",
      privateKey,
      new TextEncoder().encode(signingInput),
    ),
  );
  return `${signingInput}.${base64UrlEncode(signature)}`;
}

function historyRow({
  prNumber = 14,
  commit = COMMIT,
  status = "queued",
  uuid = `deploy-${prNumber}`,
  ...extra
} = {}) {
  return {
    deployment_uuid: uuid,
    application_uuid: env.COOLIFY_APPLICATION_UUID,
    pull_request_id: prNumber,
    commit,
    status,
    created_at: new Date().toISOString(),
    ...extra,
  };
}

function deploymentDetail({
  prNumber = 14,
  commit = COMMIT,
  status = "finished",
  uuid = DEPLOYMENT,
  applicationUuid = env.COOLIFY_APPLICATION_UUID,
  ...extra
} = {}) {
  return {
    deployment_uuid: uuid,
    pull_request_id: prNumber,
    commit,
    status,
    application: { uuid: applicationUuid },
    ...extra,
  };
}

function json(payload, status = 200, headers = {}) {
  return new Response(JSON.stringify(payload), {
    status,
    headers: { "content-type": "application/json", ...headers },
  });
}

async function reconcileRequest(claims = claimsFor(), init = {}) {
  const token = await sessionFor(claims);
  return new Request("https://coolify.proa.so/_proa/github-actions/reconcile", {
    method: "POST",
    headers: { authorization: `Bearer ${token}` },
    ...init,
  });
}

async function apiRequest(path, claims = claimsFor(), init = {}) {
  const token = await sessionFor(claims);
  return new Request(`https://coolify.proa.so${path}`, {
    method: "GET",
    headers: { authorization: `Bearer ${token}` },
    ...init,
  });
}

async function archiveRequest(payload, claims = archiveClaimsFor(), init = {}) {
  const token = await sessionFor(claims);
  return new Request("https://coolify.proa.so/_proa/github-actions/archive", {
    method: "POST",
    headers: {
      authorization: `Bearer ${token}`,
      "content-type": "application/json",
    },
    body: JSON.stringify(payload),
    ...init,
  });
}

async function presentationRequest(claims = presentationClaimsFor(), init = {}) {
  const token = await sessionFor(claims);
  return new Request("https://coolify.proa.so/_proa/github-actions/presentation-token", {
    method: "POST",
    headers: { authorization: `Bearer ${token}` },
    ...init,
  });
}

function pemForPkcs8(bytes) {
  const encoded = Buffer.from(bytes).toString("base64");
  const lines = encoded.match(/.{1,64}/gu) || [];
  return `-----BEGIN PRIVATE KEY-----\n${lines.join("\n")}\n-----END PRIVATE KEY-----\n`;
}

async function presentationFixture() {
  const keys = await crypto.subtle.generateKey(
    {
      name: "RSASSA-PKCS1-v1_5",
      modulusLength: 2048,
      publicExponent: new Uint8Array([1, 0, 1]),
      hash: "SHA-256",
    },
    true,
    ["sign", "verify"],
  );
  const pkcs8 = await crypto.subtle.exportKey("pkcs8", keys.privateKey);
  return {
    env: {
      ...env,
      PROA_PREVIEW_APP_ID: "4541370",
      PROA_PREVIEW_APP_SLUG: "proa-preview",
      PROA_PREVIEW_APP_PRIVATE_KEY: pemForPkcs8(pkcs8),
    },
    keys,
  };
}

function installationPayload(overrides = {}) {
  return {
    id: 987654,
    app_id: 4541370,
    app_slug: "proa-preview",
    repository_selection: "selected",
    target_type: "Organization",
    account: { login: "Proa-Labs" },
    ...overrides,
  };
}

function installationTokenPayload(overrides = {}) {
  const expiresAt = new Date(Date.now() + 60 * 60 * 1000)
    .toISOString()
    .replace(/\.\d{3}Z$/u, "Z");
  return {
    token: `ghs_${"a".repeat(36)}`,
    expires_at: expiresAt,
    repository_selection: "selected",
    permissions: {
      deployments: "write",
      issues: "write",
      metadata: "read",
      pull_requests: "write",
    },
    repositories: [{ name: "proa", full_name: "Proa-Labs/proa" }],
    ...overrides,
  };
}

function statelessInstallationToken() {
  const segment = "Ab9_-stateless.payload-";
  return `ghs_${segment.repeat(24).slice(0, 516)}`;
}

async function withFetch(mock, operation) {
  const original = globalThis.fetch;
  globalThis.fetch = mock;
  try {
    return await operation();
  } finally {
    globalThis.fetch = original;
  }
}

test("authenticated native PR and push events are Actions-owned only for this repository", async () => {
  let calls = 0;
  await withFetch(async () => {
    calls += 1;
    return new Response("unexpected", { status: 500 });
  }, async () => {
    for (const action of ["opened", "synchronize", "ready_for_review", "closed"]) {
      const body = JSON.stringify({
        action,
        repository: { id: 12345, full_name: env.GITHUB_REPOSITORY },
      });
      const response = await worker.fetch(await signedRequest(body), env);
      assert.equal(response.status, 202);
      assert.deepEqual(await response.json(), { accepted: false, reason: "actions_owned" });
    }
    const push = await worker.fetch(
      await signedRequest(
        JSON.stringify({ repository: { id: 12345, full_name: env.GITHUB_REPOSITORY } }),
        "push",
      ),
      env,
    );
    assert.equal(push.status, 202);
    assert.deepEqual(await push.json(), { accepted: false, reason: "actions_owned" });

    const malformed = await worker.fetch(await signedRequest("not-json"), env);
    assert.equal(malformed.status, 400);
  });
  assert.equal(calls, 0);
});

test("native pull-request ownership does not bypass HMAC or App identity", async () => {
  const badSignature = await signedRequest("{}", "pull_request", "wrong-secret");
  assert.equal((await worker.fetch(badSignature, env)).status, 401);

  const badApp = await signedRequest("{}");
  badApp.headers.set("x-github-hook-installation-target-id", "999");
  assert.equal((await worker.fetch(badApp, env)).status, 403);

  const mismatchedRepository = await signedRequest(JSON.stringify({
    repository: { id: 12345, full_name: "Proa-Labs/renamed" },
  }));
  assert.equal((await worker.fetch(mismatchedRepository, env)).status, 409);

  const staleConfiguredId = await signedRequest(JSON.stringify({
    repository: { id: 99999, full_name: env.GITHUB_REPOSITORY },
  }));
  assert.equal((await worker.fetch(staleConfiguredId, env)).status, 409);

  const missingRepository = await signedRequest(JSON.stringify({ action: "opened" }));
  assert.equal((await worker.fetch(missingRepository, env)).status, 400);
});

test("signed lifecycle and other-repository data events still reach Coolify", async () => {
  const forwarded = [];
  await withFetch(async (request) => {
    forwarded.push(request);
    return new Response("pong", { status: 200 });
  }, async () => {
    const ping = await worker.fetch(await signedRequest("{}", "ping"), env);
    assert.equal(ping.status, 200);
    const push = await worker.fetch(
      await signedRequest(
        JSON.stringify({ repository: { id: 99999, full_name: "Proa-Labs/other" } }),
        "push",
      ),
      env,
    );
    assert.equal(push.status, 200);
    const pullRequest = await worker.fetch(
      await signedRequest(
        JSON.stringify({
          action: "opened",
          repository: { id: 99999, full_name: "Proa-Labs/other" },
        }),
        "pull_request",
      ),
      env,
    );
    assert.equal(pullRequest.status, 200);
  });
  assert.equal(forwarded.length, 3);
  assert.ok(forwarded.every((request) => new URL(request.url).host === "coolify.proa.cool"));
});

test("webhook HMAC verification accepts only the exact bytes", async () => {
  const request = await signedRequest('{"action":"opened"}');
  const body = new TextEncoder().encode('{"action":"opened"}');
  assert.equal(await verifyWebhookSignature(request, body, env.GITHUB_WEBHOOK_SECRET), true);
  assert.equal(
    await verifyWebhookSignature(request, new TextEncoder().encode("changed"), env.GITHUB_WEBHOOK_SECRET),
    false,
  );
});

test("origin cookie rewriting preserves distinct Set-Cookie values and Expires commas", async () => {
  await withFetch(async () => {
    const headers = new Headers();
    headers.append(
      "set-cookie",
      "laravel_session=one; Expires=Wed, 21 Oct 2030 07:28:00 GMT; Domain=coolify.proa.cool; Path=/; HttpOnly",
    );
    headers.append("set-cookie", "xsrf=two; Domain=.coolify.proa.cool; Path=/; Secure");
    return new Response("ok", { status: 200, headers });
  }, async () => {
    const response = await worker.fetch(new Request("https://coolify.proa.so/login"), env);
    const cookies = response.headers.getSetCookie();
    assert.equal(cookies.length, 2);
    assert.match(cookies[0], /Expires=Wed, 21 Oct 2030 07:28:00 GMT/u);
    assert.match(cookies[0], /laravel_session=one/u);
    assert.match(cookies[1], /xsrf=two/u);
    assert.ok(cookies.every((cookie) => !/Domain=/iu.test(cookie)));
  });
});

test("Actions sessions require a canonical full expected commit", async () => {
  const claims = claimsFor();
  const token = await sessionFor(claims);
  assert.deepEqual(await verifySessionToken(token, env), claims);
  for (const expectedCommit of [undefined, "short", COMMIT.toUpperCase()]) {
    const invalid = { ...claims };
    if (expectedCommit === undefined) delete invalid.expected_commit;
    else invalid.expected_commit = expectedCommit;
    await assert.rejects(
      verifySessionToken(await sessionFor(invalid), env),
      /session claims/u,
    );
  }
});

test("Actions sessions require an explicit purpose with a purpose-bounded lifetime", async () => {
  const deploy = claimsFor();
  const teardown = teardownClaimsFor();
  const archive = archiveClaimsFor();
  const presentation = presentationClaimsFor();
  assert.deepEqual(await verifySessionToken(await sessionFor(deploy), env), deploy);
  assert.deepEqual(await verifySessionToken(await sessionFor(teardown), env), teardown);
  assert.deepEqual(await verifySessionToken(await sessionFor(archive), env), archive);
  assert.deepEqual(
    await verifySessionToken(await sessionFor(presentation), env),
    presentation,
  );

  for (const invalid of [
    { ...deploy, purpose: undefined },
    { ...deploy, purpose: "admin" },
    { ...deploy, exp: deploy.iat + 4_501 },
    { ...teardown, exp: teardown.iat + 301 },
    { ...archive, exp: archive.iat + 301 },
    { ...presentation, exp: presentation.iat + 301 },
    { ...archive, run_id: 0 },
    { ...archive, run_attempt: "not-an-integer" },
    { ...deploy, iat: deploy.iat + 31, exp: deploy.exp + 31 },
  ]) {
    if (invalid.purpose === undefined) delete invalid.purpose;
    await assert.rejects(
      verifySessionToken(await sessionFor(invalid), env),
      /session claims/u,
    );
  }
});

test("OIDC exchange requires and canonicalizes the full expected commit", async () => {
  const keys = await crypto.subtle.generateKey(
    {
      name: "RSASSA-PKCS1-v1_5",
      modulusLength: 2048,
      publicExponent: new Uint8Array([1, 0, 1]),
      hash: "SHA-256",
    },
    true,
    ["sign", "verify"],
  );
  const jwk = await crypto.subtle.exportKey("jwk", keys.publicKey);
  jwk.kid = "test-key";
  const oidcEnv = {
    ...env,
    GITHUB_WORKFLOW_REF:
      "Proa-Labs/proa/.github/workflows/marketing-preview.yml@refs/heads/main",
    OIDC_AUDIENCE: "https://coolify.proa.so/github-actions",
  };
  const now = Math.floor(Date.now() / 1000);
  const oidcClaims = {
    iss: "https://token.actions.githubusercontent.com",
    aud: oidcEnv.OIDC_AUDIENCE,
    repository: oidcEnv.GITHUB_REPOSITORY,
    repository_id: oidcEnv.GITHUB_REPOSITORY_ID,
    workflow_ref: oidcEnv.GITHUB_WORKFLOW_REF,
    ref: "refs/heads/main",
    event_name: "pull_request_target",
    iat: now,
    nbf: now - 5,
    exp: now + 300,
    run_id: "99",
    run_attempt: "1",
  };
  const oidc = await signedOidc(oidcClaims, keys.privateKey);

  await withFetch(async () => json({ keys: [jwk] }), async () => {
    assert.deepEqual(await verifyGithubOidc(oidc, oidcEnv), oidcClaims);
    const request = new Request("https://coolify.proa.so/_proa/github-actions/token", {
      method: "POST",
      headers: { authorization: `Bearer ${oidc}`, "content-type": "application/json" },
      body: JSON.stringify({
        application_uuid: env.COOLIFY_APPLICATION_UUID,
        pr_number: 14,
        expected_commit: COMMIT.toUpperCase(),
      }),
    });
    const response = await worker.fetch(request, oidcEnv);
    assert.equal(response.status, 200);
    const exchanged = await response.json();
    const session = await verifySessionToken(exchanged.token, oidcEnv);
    assert.equal(session.expected_commit, COMMIT);
    assert.equal(session.purpose, "deploy");
    assert.ok(exchanged.expires_in >= 300 && exchanged.expires_in <= 4_500);

    const teardownRequest = new Request("https://coolify.proa.so/_proa/github-actions/token", {
      method: "POST",
      headers: { authorization: `Bearer ${oidc}`, "content-type": "application/json" },
      body: JSON.stringify({
        application_uuid: env.COOLIFY_APPLICATION_UUID,
        pr_number: 14,
        expected_commit: COMMIT,
        purpose: "teardown",
      }),
    });
    const teardownResponse = await worker.fetch(teardownRequest, oidcEnv);
    assert.equal(teardownResponse.status, 200);
    const teardownExchange = await teardownResponse.json();
    assert.equal(teardownExchange.expires_in, 300);
    const teardownSession = await verifySessionToken(teardownExchange.token, oidcEnv);
    assert.equal(teardownSession.purpose, "teardown");
    assert.equal(teardownSession.exp - teardownSession.iat, 300);

    const archiveRequest = new Request("https://coolify.proa.so/_proa/github-actions/token", {
      method: "POST",
      headers: { authorization: `Bearer ${oidc}`, "content-type": "application/json" },
      body: JSON.stringify({
        application_uuid: env.COOLIFY_APPLICATION_UUID,
        pr_number: 14,
        expected_commit: COMMIT,
        purpose: "archive",
      }),
    });
    const archiveResponse = await worker.fetch(archiveRequest, oidcEnv);
    assert.equal(archiveResponse.status, 200);
    const archiveExchange = await archiveResponse.json();
    assert.equal(archiveExchange.expires_in, 300);
    const archiveSession = await verifySessionToken(archiveExchange.token, oidcEnv);
    assert.equal(archiveSession.purpose, "archive");
    assert.equal(archiveSession.exp - archiveSession.iat, 300);
    assert.equal(archiveSession.run_id, 99);
    assert.equal(archiveSession.run_attempt, 1);

    const presentationRequest = new Request(
      "https://coolify.proa.so/_proa/github-actions/token",
      {
        method: "POST",
        headers: { authorization: `Bearer ${oidc}`, "content-type": "application/json" },
        body: JSON.stringify({
          application_uuid: env.COOLIFY_APPLICATION_UUID,
          pr_number: 14,
          expected_commit: COMMIT,
          purpose: "presentation",
        }),
      },
    );
    const presentationResponse = await worker.fetch(presentationRequest, oidcEnv);
    assert.equal(presentationResponse.status, 200);
    const presentationExchange = await presentationResponse.json();
    assert.equal(presentationExchange.expires_in, 300);
    const presentationSession = await verifySessionToken(
      presentationExchange.token,
      oidcEnv,
    );
    assert.equal(presentationSession.purpose, "presentation");
    assert.equal(presentationSession.exp - presentationSession.iat, 300);

    const invalidPurpose = new Request("https://coolify.proa.so/_proa/github-actions/token", {
      method: "POST",
      headers: { authorization: `Bearer ${oidc}`, "content-type": "application/json" },
      body: JSON.stringify({
        application_uuid: env.COOLIFY_APPLICATION_UUID,
        pr_number: 14,
        expected_commit: COMMIT,
        purpose: "admin",
      }),
    });
    assert.equal((await worker.fetch(invalidPurpose, oidcEnv)).status, 403);

    const missing = new Request("https://coolify.proa.so/_proa/github-actions/token", {
      method: "POST",
      headers: { authorization: `Bearer ${oidc}`, "content-type": "application/json" },
      body: JSON.stringify({ application_uuid: env.COOLIFY_APPLICATION_UUID, pr_number: 14 }),
    });
    assert.equal((await worker.fetch(missing, oidcEnv)).status, 403);

    const invalidClaims = [
      ["issuer", { iss: "https://example.invalid" }],
      ["audience", { aud: "https://example.invalid/actions" }],
      ["repository", { repository: "Proa-Labs/other" }],
      ["repository id", { repository_id: "54321" }],
      ["workflow", { workflow_ref: "Proa-Labs/proa/.github/workflows/other.yml@refs/heads/main" }],
      ["ref", { ref: "refs/heads/release" }],
      ["event", { event_name: "pull_request" }],
      ["expired", { exp: now - 31 }],
      // `now` is sampled once above, while the worker re-reads the clock per
      // request. A margin of exactly one second past the tolerance means a
      // single tick between the two makes the claim legal again, so these two
      // cases sit a full tolerance window outside instead.
      ["future issued-at", { iat: now + 61 }],
      ["old issued-at", { iat: now - 601 }],
      ["future not-before", { nbf: now + 61 }],
      ["run id", { run_id: "0" }],
      ["run attempt", { run_attempt: "0" }],
    ];
    for (const [description, overrides] of invalidClaims) {
      const invalidOidc = await signedOidc(
        { ...oidcClaims, ...overrides },
        keys.privateKey,
      );
      const invalidRequest = new Request(
        "https://coolify.proa.so/_proa/github-actions/token",
        {
          method: "POST",
          headers: {
            authorization: `Bearer ${invalidOidc}`,
            "content-type": "application/json",
          },
          body: JSON.stringify({
            application_uuid: env.COOLIFY_APPLICATION_UUID,
            pr_number: 14,
            expected_commit: COMMIT,
            purpose: "presentation",
          }),
        },
      );
      assert.equal(
        (await worker.fetch(invalidRequest, oidcEnv)).status,
        403,
        `accepted invalid OIDC ${description}`,
      );
    }

    for (const [description, header] of [
      ["algorithm", { alg: "HS256" }],
      ["key id", { kid: "unknown-key" }],
    ]) {
      const invalidOidc = await signedOidc(oidcClaims, keys.privateKey, header);
      const invalidRequest = new Request(
        "https://coolify.proa.so/_proa/github-actions/token",
        {
          method: "POST",
          headers: {
            authorization: `Bearer ${invalidOidc}`,
            "content-type": "application/json",
          },
          body: JSON.stringify({
            application_uuid: env.COOLIFY_APPLICATION_UUID,
            pr_number: 14,
            expected_commit: COMMIT,
          }),
        },
      );
      assert.equal(
        (await worker.fetch(invalidRequest, oidcEnv)).status,
        403,
        `accepted invalid OIDC ${description}`,
      );
    }

    const otherKeys = await crypto.subtle.generateKey(
      {
        name: "RSASSA-PKCS1-v1_5",
        modulusLength: 2048,
        publicExponent: new Uint8Array([1, 0, 1]),
        hash: "SHA-256",
      },
      true,
      ["sign", "verify"],
    );
    const wrongSignature = await signedOidc(oidcClaims, otherKeys.privateKey);
    const wrongSignatureRequest = new Request(
      "https://coolify.proa.so/_proa/github-actions/token",
      {
        method: "POST",
        headers: {
          authorization: `Bearer ${wrongSignature}`,
          "content-type": "application/json",
        },
        body: JSON.stringify({
          application_uuid: env.COOLIFY_APPLICATION_UUID,
          pr_number: 14,
          expected_commit: COMMIT,
        }),
      },
    );
    assert.equal((await worker.fetch(wrongSignatureRequest, oidcEnv)).status, 403);
  });
});

test("Actions API path checks remain application and PR scoped", () => {
  const claims = claimsFor();
  const request = (method, path) => new Request(`https://coolify.proa.so${path}`, { method });
  assert.equal(authorizedApiPath(request("GET", "/api/v1/applications/app-123"), claims), true);
  assert.equal(authorizedApiPath(request("GET", "/api/v1/applications/app-123/envs"), claims), true);
  assert.equal(
    authorizedApiPath(request("GET", "/api/v1/deployments/applications/app-123?take=100"), claims),
    true,
  );
  assert.equal(
    authorizedApiPath(request("DELETE", "/api/v1/applications/app-123/previews/14"), claims),
    true,
  );
  assert.equal(
    authorizedApiPath(request("DELETE", "/api/v1/applications/app-123/previews/15"), claims),
    false,
  );
  assert.equal(authorizedApiPath(request("GET", "/api/v1/applications/other"), claims), false);

  const teardownClaims = teardownClaimsFor();
  assert.equal(
    authorizedApiPath(
      request("DELETE", "/api/v1/applications/app-123/previews/14"),
      teardownClaims,
    ),
    true,
  );
  assert.equal(
    authorizedApiPath(request("GET", "/api/v1/applications/app-123"), teardownClaims),
    false,
  );
  assert.equal(
    authorizedApiPath(
      request("GET", "/api/v1/deployments/applications/app-123"),
      teardownClaims,
    ),
    false,
  );
  assert.equal(
    authorizedApiPath(
      request("DELETE", "/api/v1/applications/app-123/previews/15"),
      teardownClaims,
    ),
    false,
  );

  const archiveClaims = archiveClaimsFor();
  assert.equal(
    authorizedApiPath(request("GET", "/api/v1/applications/app-123"), archiveClaims),
    false,
  );
  assert.equal(
    authorizedApiPath(
      request("DELETE", "/api/v1/applications/app-123/previews/14"),
      archiveClaims,
    ),
    false,
  );

  const presentationClaims = presentationClaimsFor();
  assert.equal(
    authorizedApiPath(
      request("GET", "/api/v1/applications/app-123"),
      presentationClaims,
    ),
    false,
  );
  assert.equal(
    authorizedApiPath(
      request("DELETE", "/api/v1/applications/app-123/previews/14"),
      presentationClaims,
    ),
    false,
  );
});

test("presentation broker signs an App JWT and mints one fixed repository token", async () => {
  const fixture = await presentationFixture();
  const requests = [];
  const tokenPayload = installationTokenPayload();
  await withFetch(async (request) => {
    requests.push(request.clone());
    const url = new URL(request.url);
    if (url.pathname === "/repos/Proa-Labs/proa/installation") {
      assert.equal(request.method, "GET");
      assert.equal(request.redirect, "manual");
      assert.equal(request.headers.get("accept"), "application/vnd.github+json");
      assert.equal(request.headers.get("x-github-api-version"), "2022-11-28");
      assert.equal(request.headers.get("user-agent"), "Proa-Preview-Token-Broker");
      assert.deepEqual([...request.headers.keys()].sort(), [
        "accept",
        "authorization",
        "user-agent",
        "x-github-api-version",
      ]);
      return json(installationPayload());
    }
    assert.equal(url.pathname, "/app/installations/987654/access_tokens");
    assert.equal(request.method, "POST");
    assert.equal(request.redirect, "manual");
    assert.deepEqual(await request.json(), {
      repositories: ["proa"],
      permissions: {
        deployments: "write",
        issues: "write",
        pull_requests: "write",
      },
    });
    return json(tokenPayload, 201);
  }, async () => {
    const response = await worker.fetch(await presentationRequest(), fixture.env);
    assert.equal(response.status, 200);
    assert.equal(response.headers.get("cache-control"), "no-store");
    assert.deepEqual(await response.json(), {
      token: tokenPayload.token,
      expires_at: tokenPayload.expires_at,
      bot_login: "proa-preview[bot]",
    });
  });

  assert.equal(requests.length, 2);
  const authorizations = requests.map((request) => request.headers.get("authorization"));
  assert.equal(authorizations[0], authorizations[1]);
  assert.match(authorizations[0], /^Bearer [A-Za-z0-9_-]+\.[A-Za-z0-9_-]+\.[A-Za-z0-9_-]+$/u);
  const jwt = authorizations[0].slice("Bearer ".length);
  const [encodedHeader, encodedClaims, encodedSignature] = jwt.split(".");
  assert.deepEqual(JSON.parse(Buffer.from(encodedHeader, "base64url")), {
    alg: "RS256",
    typ: "JWT",
  });
  const jwtClaims = JSON.parse(Buffer.from(encodedClaims, "base64url"));
  assert.equal(jwtClaims.iss, 4541370);
  assert.equal(jwtClaims.exp - jwtClaims.iat, 600);
  const now = Math.floor(Date.now() / 1000);
  assert.ok(jwtClaims.iat >= now - 65 && jwtClaims.iat <= now - 55);
  const valid = await crypto.subtle.verify(
    "RSASSA-PKCS1-v1_5",
    fixture.keys.publicKey,
    Buffer.from(encodedSignature, "base64url"),
    new TextEncoder().encode(`${encodedHeader}.${encodedClaims}`),
  );
  assert.equal(valid, true);
});

test("presentation-session replay stays time-bound and repository-scoped", async () => {
  const fixture = await presentationFixture();
  const claims = presentationClaimsFor();
  const session = await sessionFor(claims);
  const requests = [];
  let tokenSequence = 0;

  await withFetch(async (request) => {
    requests.push(request.clone());
    const url = new URL(request.url);
    if (url.pathname === "/repos/Proa-Labs/proa/installation") {
      return json(installationPayload());
    }
    assert.equal(url.pathname, "/app/installations/987654/access_tokens");
    assert.deepEqual(await request.json(), {
      repositories: ["proa"],
      permissions: {
        deployments: "write",
        issues: "write",
        pull_requests: "write",
      },
    });
    tokenSequence += 1;
    return json(
      installationTokenPayload({ token: `ghs_${String(tokenSequence).repeat(36)}` }),
      201,
    );
  }, async () => {
    for (let attempt = 0; attempt < 2; attempt += 1) {
      const response = await worker.fetch(
        new Request(
          "https://coolify.proa.so/_proa/github-actions/presentation-token",
          {
            method: "POST",
            headers: { authorization: `Bearer ${session}` },
          },
        ),
        fixture.env,
      );
      assert.equal(response.status, 200);
      const payload = await response.json();
      assert.equal(payload.bot_login, "proa-preview[bot]");
      assert.match(payload.token, /^ghs_[A-Za-z0-9._-]{36,2044}$/u);
    }
  });

  assert.equal(requests.length, 4);
  assert.equal(claims.exp - claims.iat, 300);
});

test("presentation broker accepts only an empty POST with its five-minute capability", async () => {
  let calls = 0;
  await withFetch(async () => {
    calls += 1;
    return new Response("unexpected", { status: 500 });
  }, async () => {
    const missing = new Request(
      "https://coolify.proa.so/_proa/github-actions/presentation-token",
      { method: "POST" },
    );
    assert.equal((await worker.fetch(missing, env)).status, 401);

    for (const claims of [claimsFor(), archiveClaimsFor(), teardownClaimsFor()]) {
      const response = await worker.fetch(await presentationRequest(claims), env);
      assert.equal(response.status, 403);
    }

    const get = await presentationRequest(presentationClaimsFor(), { method: "GET" });
    assert.equal((await worker.fetch(get, env)).status, 405);

    const body = await presentationRequest(presentationClaimsFor(), {
      headers: {
        authorization: `Bearer ${await sessionFor(presentationClaimsFor())}`,
        "content-type": "application/json",
      },
      body: "{}",
    });
    const bodyResponse = await worker.fetch(body, env);
    assert.equal(bodyResponse.status, 400);
    assert.deepEqual(await bodyResponse.json(), {
      result: "error",
      reason: "invalid_request",
    });

    const unconfigured = await worker.fetch(await presentationRequest(), env);
    assert.equal(unconfigured.status, 503);
    assert.match(await unconfigured.text(), /not configured/u);
  });
  assert.equal(calls, 0);
});

test("presentation broker rejects malformed Worker-only App configuration", async () => {
  const fixture = await presentationFixture();
  const invalidConfigurations = [
    { PROA_PREVIEW_APP_ID: "0" },
    { PROA_PREVIEW_APP_ID: "not-an-id" },
    { PROA_PREVIEW_APP_SLUG: "Proa Preview" },
    { PROA_PREVIEW_APP_PRIVATE_KEY: undefined },
    {
      PROA_PREVIEW_APP_PRIVATE_KEY:
        "-----BEGIN RSA PRIVATE KEY-----\nZmFrZQ==\n-----END RSA PRIVATE KEY-----\n",
    },
    {
      PROA_PREVIEW_APP_PRIVATE_KEY:
        "-----BEGIN PRIVATE KEY-----\nZmFrZQ==\n-----END PRIVATE KEY-----\n",
    },
    { GITHUB_REPOSITORY: "Proa-Labs/other" },
  ];
  let calls = 0;
  await withFetch(async () => {
    calls += 1;
    return new Response("unexpected", { status: 500 });
  }, async () => {
    for (const overrides of invalidConfigurations) {
      const response = await worker.fetch(
        await presentationRequest(),
        { ...fixture.env, ...overrides },
      );
      assert.equal(response.status, 503);
      assert.match(await response.text(), /not configured/u);
    }
  });
  assert.equal(calls, 0);
});

test("presentation broker fails closed on a mismatched App installation", async () => {
  const fixture = await presentationFixture();
  const candidates = [
    installationPayload({ app_id: 7 }),
    installationPayload({ app_slug: "other-app" }),
    installationPayload({ repository_selection: "all" }),
    installationPayload({ target_type: "User" }),
    installationPayload({ account: { login: "SomeoneElse" } }),
    { id: 987654 },
  ];
  for (const candidate of candidates) {
    let calls = 0;
    await withFetch(async () => {
      calls += 1;
      return json(candidate);
    }, async () => {
      const response = await worker.fetch(await presentationRequest(), fixture.env);
      assert.equal(response.status, 502);
      assert.deepEqual(await response.json(), {
        result: "error",
        reason: "github_app_token_unavailable",
      });
    });
    assert.equal(calls, 1);
  }
});

test("presentation broker validates the exact installation-token projection", async () => {
  const fixture = await presentationFixture();
  const tooLate = new Date(Date.now() + 2 * 60 * 60 * 1000)
    .toISOString()
    .replace(/\.\d{3}Z$/u, "Z");
  const candidates = [
    installationTokenPayload({ token: "github_pat_secret-value" }),
    installationTokenPayload({ expires_at: tooLate }),
    installationTokenPayload({ repository_selection: "all" }),
    installationTokenPayload({
      permissions: {
        deployments: "write",
        issues: "write",
        pull_requests: "read",
      },
    }),
    installationTokenPayload({
      permissions: {
        actions: "read",
        deployments: "write",
        issues: "write",
        pull_requests: "write",
      },
    }),
    installationTokenPayload({
      repositories: [{ name: "other", full_name: "Proa-Labs/other" }],
    }),
    installationTokenPayload({ repositories: [] }),
  ];
  for (const candidate of candidates) {
    let calls = 0;
    await withFetch(async (request) => {
      calls += 1;
      if (request.method === "GET") return json(installationPayload());
      return json(candidate, 201);
    }, async () => {
      const response = await worker.fetch(await presentationRequest(), fixture.env);
      assert.equal(response.status, 502);
      const body = await response.text();
      assert.deepEqual(JSON.parse(body), {
        result: "error",
        reason: "github_app_token_unavailable",
      });
      assert.ok(!body.includes("github_pat_secret-value"));
      assert.ok(!body.includes(fixture.env.PROA_PREVIEW_APP_PRIVATE_KEY));
    });
    assert.equal(calls, 2);
  }
});

test("presentation tokens are opaque across classic and stateless GitHub formats", () => {
  const classic = `ghs_${"a".repeat(36)}`;
  const stateless = statelessInstallationToken();
  const maximum = `ghs_${"z".repeat(2044)}`;
  assert.equal(classic.length, 40);
  assert.equal(stateless.length, 520);
  assert.equal(maximum.length, 2048);
  assert.match(stateless.slice(4), /\./u);
  assert.match(stateless.slice(4), /_/u);
  assert.match(stateless.slice(4), /-/u);

  for (const token of [classic, stateless, maximum]) {
    assert.equal(
      projectPresentationToken(
        installationTokenPayload({ token }),
        { appSlug: "proa-preview" },
      ).token,
      token,
    );
  }

  for (const token of [
    `ghs_${"a".repeat(35)}`,
    `ghs_${"a".repeat(2045)}`,
    `ghs_${"a".repeat(35)}!`,
    `ghs_${"a".repeat(35)}\n`,
  ]) {
    assert.throws(
      () => projectPresentationToken(
        installationTokenPayload({ token }),
        { appSlug: "proa-preview" },
      ),
      /token rejected/u,
    );
  }
});

test("presentation broker bounds GitHub failures without exposing response bodies", async () => {
  const fixture = await presentationFixture();
  for (const responseFactory of [
    () => json({ secret: "github-upstream-secret" }, 500),
    () => new Response("not-json", { status: 200 }),
    () => new Response("x".repeat(70_000), { status: 200 }),
  ]) {
    await withFetch(async () => responseFactory(), async () => {
      const response = await worker.fetch(await presentationRequest(), fixture.env);
      assert.equal(response.status, 502);
      const body = await response.text();
      assert.deepEqual(JSON.parse(body), {
        result: "error",
        reason: "github_app_token_unavailable",
      });
      assert.ok(!body.includes("github-upstream-secret"));
      assert.ok(!body.includes(fixture.env.PROA_PREVIEW_APP_PRIVATE_KEY));
    });
  }
});

test("presentation broker bounds installation-token endpoint failures", async () => {
  const fixture = await presentationFixture();
  for (const responseFactory of [
    () => json({ secret: "token-endpoint-secret" }, 500),
    () => new Response("not-json", { status: 201 }),
    () => new Response("x".repeat(70_000), { status: 201 }),
  ]) {
    let calls = 0;
    await withFetch(async (request) => {
      calls += 1;
      if (request.method === "GET") return json(installationPayload());
      return responseFactory();
    }, async () => {
      const response = await worker.fetch(await presentationRequest(), fixture.env);
      assert.equal(response.status, 502);
      const body = await response.text();
      assert.deepEqual(JSON.parse(body), {
        result: "error",
        reason: "github_app_token_unavailable",
      });
      assert.ok(!body.includes("token-endpoint-secret"));
      assert.ok(!body.includes(fixture.env.PROA_PREVIEW_APP_PRIVATE_KEY));
    });
    assert.equal(calls, 2);
  }
});

test("archive host payload derives every authority field from the signed session", () => {
  const claims = archiveClaimsFor(41);
  const payload = archiveHostPayload(
    { operation: "promote", deployment_uuid: DEPLOYMENT },
    claims,
    1_786_291_200,
  );
  assert.deepEqual(payload, {
    v: 1,
    operation: "promote",
    application_uuid: env.COOLIFY_APPLICATION_UUID,
    pr_number: 41,
    expected_commit: COMMIT,
    deployment_uuid: DEPLOYMENT,
    run_id: 10_041,
    run_attempt: 1,
    issued_at: 1_786_291_200,
  });
  for (const clientPayload of [
    { operation: "publish", deployment_uuid: DEPLOYMENT },
    { operation: "promote", deployment_uuid: "short" },
    { operation: "promote", deployment_uuid: DEPLOYMENT, pr_number: 999 },
    { action: "promote", deployment_uuid: DEPLOYMENT },
    ["promote", DEPLOYMENT],
  ]) {
    assert.throws(
      () => archiveHostPayload(clientPayload, claims, 1_786_291_200),
      /archive request/u,
    );
  }
});

test("archive control forwards one signed, claim-derived command to the fixed origin", async () => {
  const claims = archiveClaimsFor(42);
  let outbound = null;
  const calls = [];
  await withFetch(async (request) => {
    const path = new URL(request.url).pathname;
    calls.push(`${request.method} ${path}`);
    if (path === `/api/v1/deployments/${DEPLOYMENT}`) {
      return json(deploymentDetail({
        prNumber: 42,
        commit: COMMIT,
        status: "finished",
        uuid: DEPLOYMENT,
      }));
    }
    outbound = request;
    return json({
      ok: true,
      operation: "promote",
      result: "promoted",
      deployment_uuid: DEPLOYMENT,
    });
  }, async () => {
    const response = await worker.fetch(
      await archiveRequest({ operation: "promote", deployment_uuid: DEPLOYMENT }, claims),
      env,
    );
    assert.equal(response.status, 200);
    assert.deepEqual(await response.json(), {
      result: "promoted",
      deployment_uuid: DEPLOYMENT,
    });
  });

  assert.deepEqual(calls, [
    `GET /api/v1/deployments/${DEPLOYMENT}`,
    "POST /_proa/origin/preview-archive",
  ]);
  assert.ok(outbound instanceof Request);
  assert.equal(outbound.url, "https://coolify.proa.cool/_proa/origin/preview-archive");
  assert.equal(outbound.method, "POST");
  assert.equal(outbound.redirect, "manual");
  assert.equal(outbound.headers.get("authorization"), null);
  assert.equal(outbound.headers.get("x-proa-control-plane"), env.ORIGIN_AUTH);
  assert.equal(outbound.headers.get("x-forwarded-host"), "coolify.proa.so");
  const timestamp = outbound.headers.get("x-proa-archive-timestamp");
  assert.match(timestamp, /^[1-9][0-9]{9}$/u);
  const body = new Uint8Array(await outbound.clone().arrayBuffer());
  const hostPayload = JSON.parse(new TextDecoder().decode(body));
  assert.deepEqual(hostPayload, {
    v: 1,
    operation: "promote",
    application_uuid: env.COOLIFY_APPLICATION_UUID,
    pr_number: 42,
    expected_commit: COMMIT,
    deployment_uuid: DEPLOYMENT,
    run_id: 10_042,
    run_attempt: 1,
    issued_at: Number(timestamp),
  });
  const expectedSignature = await archiveOriginSignature(
    env.ARCHIVE_CONTROL_SECRET,
    Number(timestamp),
    body,
  );
  assert.equal(
    outbound.headers.get("x-proa-archive-signature"),
    `sha256=${expectedSignature}`,
  );
});

test("archive HMAC covers the exact method, origin path, timestamp, and body bytes", async () => {
  const timestamp = 1_786_291_200;
  const body = new TextEncoder().encode('{"v":1}');
  const key = await crypto.subtle.importKey(
    "raw",
    new TextEncoder().encode(env.ARCHIVE_CONTROL_SECRET),
    { name: "HMAC", hash: "SHA-256" },
    false,
    ["sign"],
  );
  const canonical = new TextEncoder().encode(
    `v1\nPOST\n/_proa/origin/preview-archive\n${timestamp}\n{\"v\":1}`,
  );
  const expected = [...new Uint8Array(await crypto.subtle.sign("HMAC", key, canonical))]
    .map((byte) => byte.toString(16).padStart(2, "0"))
    .join("");
  assert.equal(
    await archiveOriginSignature(env.ARCHIVE_CONTROL_SECRET, timestamp, body),
    expected,
  );
});

test("archive control accepts only its short-lived capability and strict client schema", async () => {
  let calls = 0;
  await withFetch(async () => {
    calls += 1;
    return new Response("unexpected", { status: 500 });
  }, async () => {
    const missing = new Request("https://coolify.proa.so/_proa/github-actions/archive", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ operation: "promote", deployment_uuid: DEPLOYMENT }),
    });
    assert.equal((await worker.fetch(missing, env)).status, 401);

    for (const claims of [claimsFor(), teardownClaimsFor()]) {
      const response = await worker.fetch(
        await archiveRequest({ operation: "promote", deployment_uuid: DEPLOYMENT }, claims),
        env,
      );
      assert.equal(response.status, 403);
    }

    for (const payload of [
      { operation: "publish", deployment_uuid: DEPLOYMENT },
      { operation: "promote", deployment_uuid: DEPLOYMENT.toUpperCase() },
      { operation: "promote", deployment_uuid: DEPLOYMENT, revision: COMMIT },
      { action: "promote", deployment_uuid: DEPLOYMENT },
    ]) {
      const response = await worker.fetch(await archiveRequest(payload), env);
      assert.equal(response.status, 400);
      assert.deepEqual(await response.json(), { result: "error", reason: "invalid_request" });
    }

    const wrongType = await archiveRequest(
      { operation: "promote", deployment_uuid: DEPLOYMENT },
      archiveClaimsFor(),
    );
    wrongType.headers.set("content-type", "text/plain");
    assert.equal((await worker.fetch(wrongType, env)).status, 400);

    const get = new Request("https://coolify.proa.so/_proa/github-actions/archive", {
      method: "GET",
      headers: { authorization: `Bearer ${await sessionFor(archiveClaimsFor())}` },
    });
    assert.equal((await worker.fetch(get, env)).status, 405);
  });
  assert.equal(calls, 0);
});

test("archive control rejects oversized input and missing or malformed host configuration", async () => {
  const oversized = await archiveRequest({
    operation: "promote",
    deployment_uuid: DEPLOYMENT,
    padding: "x".repeat(2_000),
  });
  const oversizedResponse = await worker.fetch(oversized, env);
  assert.equal(oversizedResponse.status, 413);
  assert.deepEqual(await oversizedResponse.json(), {
    result: "error",
    reason: "request_too_large",
  });

  for (const secret of [undefined, "short", "A".repeat(64)]) {
    const configured = { ...env, ARCHIVE_CONTROL_SECRET: secret };
    const response = await worker.fetch(
      await archiveRequest({ operation: "promote", deployment_uuid: DEPLOYMENT }),
      configured,
    );
    assert.equal(response.status, 503);
    assert.match(await response.text(), /not configured/u);
  }
});

test("archive control authorizes the exact deployment against scoped Coolify detail", async () => {
  const claims = archiveClaimsFor(43);
  const cases = [
    {
      name: "different deployment UUID",
      detail: deploymentDetail({ prNumber: 43, uuid: "other-deployment" }),
      operation: "promote",
      status: 409,
      reason: "deployment_scope_rejected",
    },
    {
      name: "foreign application",
      detail: deploymentDetail({
        prNumber: 43,
        applicationUuid: "other-app",
      }),
      operation: "promote",
      status: 409,
      reason: "deployment_scope_rejected",
    },
    {
      name: "foreign pull request",
      detail: deploymentDetail({ prNumber: 99 }),
      operation: "promote",
      status: 409,
      reason: "deployment_scope_rejected",
    },
    {
      name: "wrong commit",
      detail: deploymentDetail({
        prNumber: 43,
        commit: OTHER_COMMIT,
        uuid: DEPLOYMENT,
        status: "finished",
      }),
      operation: "promote",
      status: 409,
      reason: "deployment_scope_rejected",
    },
    {
      name: "non-success promotion",
      detail: deploymentDetail({ prNumber: 43, status: "failed" }),
      operation: "promote",
      status: 409,
      reason: "deployment_not_ready",
    },
  ];
  for (const candidate of cases) {
    let originCalls = 0;
    await withFetch(async (request) => {
      if (new URL(request.url).pathname === `/api/v1/deployments/${DEPLOYMENT}`) {
        return json(candidate.detail);
      }
      originCalls += 1;
      return new Response("unexpected", { status: 500 });
    }, async () => {
      const response = await worker.fetch(
        await archiveRequest(
          { operation: candidate.operation, deployment_uuid: DEPLOYMENT },
          claims,
        ),
        env,
      );
      assert.equal(response.status, candidate.status, candidate.name);
      assert.deepEqual(await response.json(), {
        result: "error",
        reason: candidate.reason,
      });
    });
    assert.equal(originCalls, 0, candidate.name);
  }
});

test("archive revoke may target its exact failed deployment", async () => {
  const claims = archiveClaimsFor(44);
  let originCalls = 0;
  await withFetch(async (request) => {
    if (new URL(request.url).pathname === `/api/v1/deployments/${DEPLOYMENT}`) {
      return json(deploymentDetail({
        prNumber: 44,
        uuid: DEPLOYMENT,
        status: "failed",
      }));
    }
    originCalls += 1;
    return json({ result: "revoked", deployment_uuid: DEPLOYMENT });
  }, async () => {
    const response = await worker.fetch(
      await archiveRequest({ operation: "revoke", deployment_uuid: DEPLOYMENT }, claims),
      env,
    );
    assert.equal(response.status, 200);
    assert.deepEqual(await response.json(), {
      result: "revoked",
      deployment_uuid: DEPLOYMENT,
    });
  });
  assert.equal(originCalls, 1);
});

test("archive control fails closed when exact deployment detail is unavailable", async () => {
  for (const detailResponse of [
    () => json({ result: "secret-history-body" }, 503),
    () => new Response("not-json"),
    () => json({
      deployment_uuid: DEPLOYMENT,
      pull_request_id: 14,
      status: "finished",
      application: { uuid: env.COOLIFY_APPLICATION_UUID },
    }),
  ]) {
    let originCalls = 0;
    await withFetch(async (request) => {
      if (new URL(request.url).pathname === `/api/v1/deployments/${DEPLOYMENT}`) {
        return detailResponse();
      }
      originCalls += 1;
      return new Response("unexpected", { status: 500 });
    }, async () => {
      const response = await worker.fetch(
        await archiveRequest({ operation: "promote", deployment_uuid: DEPLOYMENT }),
        env,
      );
      assert.equal(response.status, 502);
      const body = await response.text();
      assert.deepEqual(JSON.parse(body), {
        result: "error",
        reason: "detail_unavailable",
      });
      assert.ok(!body.includes("secret-history-body"));
    });
    assert.equal(originCalls, 0);
  }
});

test("archive control projects bounded host responses and never leaks origin bodies", async () => {
  const cases = [
    [() => json({ result: "error", secret: "host-secret" }, 409), 409],
    [() => json({ result: "promoted", deployment_uuid: "wrong-deployment" }), 502],
    [() => json({ result: "promoted", deployment_uuid: DEPLOYMENT, secret: "host-secret" }), 502],
    [() => new Response("not-json", { status: 200 }), 502],
    [() => new Response("x".repeat(9_000), { status: 200 }), 502],
    [() => json({ result: "error", secret: "host-secret" }, 503), 502],
  ];
  for (const [hostResponse, expectedStatus] of cases) {
    await withFetch(async (request) => {
      if (new URL(request.url).pathname === `/api/v1/deployments/${DEPLOYMENT}`) {
        return json(deploymentDetail({
          prNumber: 14,
          uuid: DEPLOYMENT,
          status: "finished",
        }));
      }
      return hostResponse();
    }, async () => {
      const response = await worker.fetch(
        await archiveRequest({ operation: "promote", deployment_uuid: DEPLOYMENT }),
        env,
      );
      assert.equal(response.status, expectedStatus);
      const body = await response.text();
      assert.ok(!body.includes("host-secret"));
      assert.ok(!body.includes("wrong-deployment"));
      assert.ok(!body.includes(env.ARCHIVE_CONTROL_SECRET));
      assert.match(body, /archive_control_/u);
    });
  }
});

test("origin-only archive paths are never reachable through the public Worker", async () => {
  let calls = 0;
  await withFetch(async () => {
    calls += 1;
    return new Response("unexpected", { status: 500 });
  }, async () => {
    for (const path of [
      "/_proa/origin/preview-archive",
      "/_proa/origin/preview-archive/healthz",
    ]) {
      const response = await worker.fetch(new Request(`https://coolify.proa.so${path}`), env);
      assert.equal(response.status, 404);
    }
  });
  assert.equal(calls, 0);
});

test("history parsing filters other PRs and strips unsafe deployment fields", () => {
  const rows = historyRows({
    deployments: [
      historyRow({ extra: { logs: "secret-log", configuration: { token: "secret" } } }),
      historyRow({ prNumber: 99, uuid: "other-pr", extra: { logs: "other-secret" } }),
    ],
  }, claimsFor());
  assert.equal(rows.length, 1);
  assert.equal(rows[0].deployment_uuid, "deploy-14");
  assert.ok(!JSON.stringify(rows).includes("secret"));
  assert.throws(
    () => historyRows({ deployments: [historyRow(), historyRow()] }, claimsFor()),
    /malformed/u,
  );
  assert.throws(
    () => historyRows({ deployments: [historyRow({ commit: "short" })] }, claimsFor()),
    /malformed/u,
  );
});

test("synthetic pull requests derive every trust-sensitive field from claims", () => {
  const claims = claimsFor(31);
  const payload = syntheticPullRequest(claims, env);
  assert.equal(payload.action, "synchronize");
  assert.equal(payload.number, 31);
  assert.equal(payload.pull_request.head.sha, COMMIT);
  assert.equal(payload.pull_request.base.ref, "main");
  assert.equal(payload.repository.full_name, env.GITHUB_REPOSITORY);
  assert.equal(payload.repository.id, 12345);
  assert.equal(payload.pull_request.head.repo.id, payload.pull_request.base.repo.id);
  assert.equal(payload.pull_request.head.repo.fork, false);
  assert.equal(payload.pull_request.author_association, "OWNER");
});

test("reconcile reuses an exact active deployment without emitting a webhook", async () => {
  const claims = claimsFor(21);
  const requests = [];
  await withFetch(async (request) => {
    requests.push(request);
    return json({ deployments: [historyRow({ prNumber: 21, status: "in_progress" })] });
  }, async () => {
    const response = await worker.fetch(await reconcileRequest(claims), env);
    assert.equal(response.status, 200);
    assert.deepEqual(await response.json(), {
      result: "reused",
      state: "active",
      deployment_uuid: "deploy-21",
    });
  });
  assert.equal(requests.length, 1);
  assert.equal(requests[0].headers.get("authorization"), `Bearer ${env.COOLIFY_API_TOKEN}`);
  assert.equal(requests[0].headers.get("x-proa-control-plane"), env.ORIGIN_AUTH);
  assert.equal(new URL(requests[0].url).pathname, "/api/v1/deployments/applications/app-123");
  assert.equal(new URL(requests[0].url).search, "?skip=0&take=5");
});

test("a stale active deployment cannot wedge same-commit reconciliation", async () => {
  const claims = claimsFor(45);
  let webhook = null;
  await withFetch(async (request) => {
    const url = new URL(request.url);
    if (url.pathname.startsWith("/api/v1/deployments/applications/")) {
      return json({
        deployments: [historyRow({
          prNumber: 45,
          status: "in_progress",
          created_at: new Date(Date.now() - 46 * 60 * 1000).toISOString(),
        })],
      });
    }
    webhook = request;
    return json([{ status: "queued", message: "accepted" }]);
  }, async () => {
    const response = await worker.fetch(await reconcileRequest(claims), env);
    assert.equal(response.status, 200);
    assert.deepEqual(await response.json(), {
      result: "queued",
      baseline_deployment_uuids: ["deploy-45"],
    });
  });
  assert.ok(webhook instanceof Request);
});

test("deployment history is read in bounded pages without losing the target PR", async () => {
  const claims = claimsFor(34);
  const requests = [];
  const largeLog = "projected-history-secret".repeat(12_000);
  const firstPage = Array.from({ length: 5 }, (_, index) => historyRow({
    prNumber: 90 + index,
    uuid: `other-${index}`,
    logs: largeLog,
  }));
  assert.ok(JSON.stringify({ deployments: firstPage }).length > 1024 * 1024);
  await withFetch(async (request) => {
    requests.push(request);
    const skip = new URL(request.url).searchParams.get("skip");
    if (skip === "0") return json({ deployments: firstPage });
    return json({
      deployments: [historyRow({ prNumber: 34, status: "in_progress" })],
    });
  }, async () => {
    const response = await worker.fetch(await reconcileRequest(claims), env);
    assert.equal(response.status, 200);
    assert.deepEqual(await response.json(), {
      result: "reused",
      state: "active",
      deployment_uuid: "deploy-34",
    });
  });
  assert.deepEqual(
    requests.map((request) => new URL(request.url).search),
    ["?skip=0&take=5", "?skip=5&take=5"],
  );
});

test("terminal success is reused only when stable and immutable previews are live", async () => {
  const claims = claimsFor(22);
  const requests = [];
  await withFetch(async (request) => {
    requests.push(request);
    const url = new URL(request.url);
    if (url.pathname.startsWith("/api/v1/deployments/applications/")) {
      return json({
        deployments: [historyRow({
          prNumber: 22,
          status: "finished",
          uuid: DEPLOYMENT,
        })],
      });
    }
    assert.ok([
      "https://pr-22.proa.cool/readyz",
      "https://proa-marketing-dy2vxce7udvt.proa.cool/readyz",
    ].includes(url.href));
    return new Response("ready", {
      status: 200,
      headers: { "x-proa-revision": COMMIT },
    });
  }, async () => {
    const response = await worker.fetch(await reconcileRequest(claims), env);
    assert.deepEqual(await response.json(), {
      result: "reused",
      state: "ready",
      deployment_uuid: DEPLOYMENT,
    });
  });
  assert.equal(requests.length, 3);
});

test("a retired immutable route forces a new deployment for the same commit", async () => {
  const claims = claimsFor(44);
  let webhook = null;
  await withFetch(async (request) => {
    const url = new URL(request.url);
    if (url.pathname.startsWith("/api/v1/deployments/applications/")) {
      return json({
        deployments: [historyRow({
          prNumber: 44,
          status: "finished",
          uuid: DEPLOYMENT,
        })],
      });
    }
    if (url.hostname === "pr-44.proa.cool") {
      return new Response("ready", {
        status: 200,
        headers: { "x-proa-revision": COMMIT },
      });
    }
    if (url.hostname === "proa-marketing-dy2vxce7udvt.proa.cool") {
      return new Response("not found", { status: 404 });
    }
    webhook = request;
    return json([{ status: "queued", message: "accepted" }]);
  }, async () => {
    const response = await worker.fetch(await reconcileRequest(claims), env);
    assert.equal(response.status, 200);
    assert.deepEqual(await response.json(), {
      result: "queued",
      baseline_deployment_uuids: [DEPLOYMENT],
    });
  });
  assert.ok(webhook instanceof Request);
});

test("stale terminal history emits a minimal re-signed synchronize webhook", async () => {
  const claims = claimsFor(23);
  let webhook = null;
  await withFetch(async (request) => {
    const url = new URL(request.url);
    if (url.pathname.startsWith("/api/v1/deployments/applications/")) {
      return json({ deployments: [historyRow({ prNumber: 23, status: "finished" })] });
    }
    if (url.pathname === "/readyz" && url.hostname.endsWith(".proa.cool")) {
      return new Response("not ready", { status: 404 });
    }
    webhook = request;
    return json([{ status: "queued", message: "accepted", upstream_secret: "do-not-return" }]);
  }, async () => {
    const response = await worker.fetch(await reconcileRequest(claims), env);
    assert.equal(response.status, 200);
    assert.deepEqual(await response.json(), {
      result: "queued",
      baseline_deployment_uuids: ["deploy-23"],
    });
  });

  assert.ok(webhook instanceof Request);
  assert.equal(
    webhook.url,
    "https://coolify.proa.cool/webhooks/source/github/events/manual",
  );
  assert.equal(webhook.redirect, "manual");
  assert.equal(webhook.headers.get("authorization"), null);
  assert.equal(webhook.headers.get("x-github-event"), "pull_request");
  assert.equal(webhook.headers.get("x-github-hook-installation-target-id"), null);
  assert.equal(webhook.headers.get("x-proa-control-plane"), env.ORIGIN_AUTH);
  assert.equal(webhook.headers.get("x-forwarded-host"), "coolify.proa.so");
  assert.equal(webhook.headers.get("x-forwarded-proto"), "https");
  assert.match(webhook.headers.get("x-github-delivery"), /^[0-9a-f-]{36}$/u);
  const body = new Uint8Array(await webhook.clone().arrayBuffer());
  assert.equal(
    await verifyWebhookSignature(webhook, body, env.COOLIFY_MANUAL_WEBHOOK_SECRET),
    true,
  );
  const payload = JSON.parse(new TextDecoder().decode(body));
  assert.equal(payload.number, 23);
  assert.equal(payload.pull_request.head.sha, COMMIT);
});

test("history failures and malformed target rows fail closed without body leakage", async () => {
  const claims = claimsFor(24);
  for (const [upstream, reason] of [
    [() => json({ secret: "history-body-secret" }, 503), "history_unavailable"],
    [
      () => json({ deployments: [historyRow({ prNumber: 24, commit: "bad" })] }),
      "history_unavailable",
    ],
    [
      () => json({
        deployments: Array.from({ length: 6 }, (_, index) => historyRow({
          prNumber: 90 + index,
          uuid: `unexpected-page-${index}`,
        })),
      }),
      "history_unavailable",
    ],
    [
      () => new Response("history-body-secret", {
        status: 200,
        headers: { "content-length": String(8 * 1024 * 1024 + 1) },
      }),
      "history_payload_too_large",
    ],
  ]) {
    let calls = 0;
    await withFetch(async () => {
      calls += 1;
      return upstream();
    }, async () => {
      const response = await worker.fetch(await reconcileRequest(claims), env);
      assert.equal(response.status, 502);
      const body = await response.text();
      assert.deepEqual(JSON.parse(body), {
        result: "error",
        reason,
        mutation: "not_attempted",
      });
      assert.ok(!body.includes("history-body-secret"));
      assert.ok(!body.includes(env.COOLIFY_API_TOKEN));
    });
    assert.equal(calls, 1);
  }
});

test("a rejected synthetic webhook is reduced to a fixed gateway error", async () => {
  const claims = claimsFor(25);
  for (const webhookResponse of [
    new Response("upstream webhook secret body", { status: 500 }),
    json([{ status: "queued" }, { status: "queued" }]),
  ]) {
    let calls = 0;
    await withFetch(async (request) => {
      calls += 1;
      const url = new URL(request.url);
      if (url.pathname.startsWith("/api/v1/deployments/applications/")) {
        return json({ deployments: [] });
      }
      return webhookResponse.clone();
    }, async () => {
      const response = await worker.fetch(await reconcileRequest(claims), env);
      assert.equal(response.status, 502);
      const body = await response.text();
      assert.match(body, /webhook_rejected/u);
      assert.ok(!body.includes("upstream webhook secret body"));
    });
    assert.equal(calls, 2);
  }
});

test("concurrent same-revision reconciles share one history check across workflow runs", async () => {
  const claims = claimsFor(26);
  const rerunClaims = {
    ...claims,
    run_id: 999_026,
    run_attempt: 4,
  };
  let historyCalls = 0;
  let webhookCalls = 0;
  await withFetch(async (request) => {
    const url = new URL(request.url);
    if (url.pathname.startsWith("/api/v1/deployments/applications/")) {
      historyCalls += 1;
      await new Promise((resolve) => setTimeout(resolve, 20));
      return json({ deployments: [] });
    }
    webhookCalls += 1;
    return json([{ status: "queued" }]);
  }, async () => {
    const [first, second] = await Promise.all([
      worker.fetch(await reconcileRequest(claims), env),
      worker.fetch(await reconcileRequest(rerunClaims), env),
    ]);
    assert.deepEqual(await first.json(), {
      result: "queued",
      baseline_deployment_uuids: [],
    });
    assert.deepEqual(await second.json(), {
      result: "queued",
      baseline_deployment_uuids: [],
    });
  });
  assert.equal(historyCalls, 1);
  assert.equal(webhookCalls, 1);
});

test("reconcile accepts only POST, a valid session, and an empty body", async () => {
  const claims = claimsFor(27);
  const token = await sessionFor(claims);
  const get = new Request("https://coolify.proa.so/_proa/github-actions/reconcile", {
    method: "GET",
    headers: { authorization: `Bearer ${token}` },
  });
  assert.equal((await worker.fetch(get, env)).status, 405);
  assert.equal(
    (await worker.fetch(
      new Request("https://coolify.proa.so/_proa/github-actions/reconcile", { method: "POST" }),
      env,
    )).status,
    401,
  );
  const nonempty = await reconcileRequest(claims, { body: "{}" });
  const invalid = await worker.fetch(nonempty, env);
  assert.equal(invalid.status, 400);
  assert.deepEqual(await invalid.json(), {
    result: "error",
    reason: "invalid_request",
    mutation: "not_attempted",
  });

  for (const claims of [teardownClaimsFor(27), archiveClaimsFor(27)]) {
    const response = await worker.fetch(await reconcileRequest(claims), env);
    assert.equal(response.status, 403);
    assert.match(await response.text(), /purpose rejected/u);
  }
});

test("application, environment, and storage API responses are safe projections", async () => {
  const claims = claimsFor(28);
  const application = {
    uuid: env.COOLIFY_APPLICATION_UUID,
    git_repository: "Proa-Labs/proa",
    git_branch: "main",
    fqdn: "http://proa.cool",
    build_pack: "dockerfile",
    base_directory: "/",
    dockerfile_location: "/proa_marketing/Dockerfile",
    ports_exposes: "5555",
    ports_mappings: "",
    health_check_enabled: true,
    health_check_path: "/healthz",
    health_check_port: "5555",
    custom_docker_run_options: "--pids-limit=256",
    limits_memory: "1G",
    limits_memory_swap: "1G",
    limits_memory_reservation: "256M",
    limits_cpus: "2",
    destination: { network: "proa-preview", private_key: "destination-secret" },
    settings: { is_auto_deploy_enabled: false, webhook_secret: "settings-secret" },
    manual_webhook_secret_github: env.COOLIFY_MANUAL_WEBHOOK_SECRET,
    watch_paths: "",
  };
  await withFetch(async (request) => {
    const path = new URL(request.url).pathname;
    if (path.endsWith("/envs")) {
      return json([{
        key: "SITE_URL",
        value: "secret-value",
        is_buildtime: false,
        is_runtime: true,
        is_preview: true,
      }]);
    }
    if (path.endsWith("/storages")) {
      return json({
        persistent_storages: [{ uuid: "storage-1", mount_path: "/data", content: "secret" }],
        file_storages: [],
      });
    }
    return json(application);
  }, async () => {
    const app = await (await worker.fetch(
      await apiRequest("/api/v1/applications/app-123", claims), env,
    )).json();
    assert.equal(app.git_branch, "main");
    assert.equal(app.destination.network, "proa-preview");
    assert.ok(!JSON.stringify(app).includes("secret"));

    const environment = await (await worker.fetch(
      await apiRequest("/api/v1/applications/app-123/envs", claims), env,
    )).json();
    assert.deepEqual(environment, [{
      key: "SITE_URL",
      is_buildtime: false,
      is_runtime: true,
      is_preview: true,
    }]);

    const storages = await (await worker.fetch(
      await apiRequest("/api/v1/applications/app-123/storages", claims), env,
    )).json();
    assert.deepEqual(storages.persistent_storages, [{}]);
    assert.ok(!JSON.stringify(storages).includes("secret"));
  });

  await withFetch(
    async () => json({
      ...application,
      manual_webhook_secret_github: "secret-bound-to-a-different-application",
    }),
    async () => {
      const response = await worker.fetch(
        await apiRequest("/api/v1/applications/app-123", claims),
        env,
      );
      assert.equal(response.status, 403);
      assert.match(await response.text(), /scope rejected/u);
    },
  );
});

test("unknown Docker run options fail closed without leaking their raw value", async () => {
  const claims = claimsFor(33);
  const secretOptions = "--env API_TOKEN=projection-secret --pids-limit=999";
  await withFetch(async () => json({
    uuid: env.COOLIFY_APPLICATION_UUID,
    manual_webhook_secret_github: env.COOLIFY_MANUAL_WEBHOOK_SECRET,
    custom_docker_run_options: secretOptions,
  }), async () => {
    const response = await worker.fetch(
      await apiRequest("/api/v1/applications/app-123", claims),
      env,
    );
    assert.equal(response.status, 200);
    const body = await response.text();
    assert.ok(!body.includes("projection-secret"));
    assert.deepEqual(JSON.parse(body), { uuid: env.COOLIFY_APPLICATION_UUID });
  });
});

test("session-visible deployment history is PR-filtered and safe-projected", async () => {
  const claims = claimsFor(29);
  await withFetch(async () => json({
    deployments: [
      historyRow({ prNumber: 29, extra: { logs: "secret-log" } }),
      historyRow({ prNumber: 30, uuid: "other-pr", extra: { logs: "other-secret" } }),
    ],
  }), async () => {
    const response = await worker.fetch(
      await apiRequest("/api/v1/deployments/applications/app-123?take=100", claims),
      env,
    );
    assert.equal(response.status, 200);
    const payload = await response.json();
    assert.equal(payload.count, 1);
    assert.equal(payload.deployments[0].pull_request_id, 29);
    assert.ok(!JSON.stringify(payload).includes("secret"));
  });
});

test("deployment detail is attested and projected without application secrets", async () => {
  const claims = claimsFor(30);
  const detail = {
    deployment_uuid: "deploy-30",
    pull_request_id: 30,
    commit: COMMIT,
    status: "in_progress",
    deployment_url: "/project/application/deployment/deploy-30",
    logs: "secret logs",
    application: {
      uuid: env.COOLIFY_APPLICATION_UUID,
      manual_webhook_secret_github: "secret webhook",
      basic_auth_password: "secret password",
    },
  };
  await withFetch(async () => json(detail), async () => {
    const response = await worker.fetch(
      await apiRequest("/api/v1/deployments/deploy-30", claims),
      env,
    );
    assert.equal(response.status, 200);
    assert.deepEqual(await response.json(), {
      deployment_uuid: "deploy-30",
      status: "in_progress",
      deployment_url: null,
      commit: COMMIT,
      pull_request_id: 30,
      application: { uuid: env.COOLIFY_APPLICATION_UUID },
    });
  });

  assert.throws(
    () => projectDeploymentDetail(
      { ...detail, application: { uuid: "other-app" } },
      "deploy-30",
      claims,
    ),
    /scope/u,
  );
});

test("preview DELETE rejects a session superseded by the newest PR revision", async () => {
  const claims = claimsFor(32);
  const calls = [];
  await withFetch(async (outbound) => {
    calls.push(`${outbound.method} ${new URL(outbound.url).pathname}`);
    return json({
      deployments: [historyRow({ prNumber: 32, commit: OTHER_COMMIT, uuid: "newer-32" })],
    });
  }, async () => {
    const request = await apiRequest("/api/v1/applications/app-123/previews/32", claims, {
      method: "DELETE",
    });
    const response = await worker.fetch(request, env);
    assert.equal(response.status, 412);
    assert.equal(await response.text(), "");
  });
  assert.deepEqual(calls, ["GET /api/v1/deployments/applications/app-123"]);
});

test("teardown sessions bypass revision history but remain DELETE-only and PR-bound", async () => {
  const claims = teardownClaimsFor(32, OTHER_COMMIT);
  const calls = [];
  await withFetch(async (outbound) => {
    calls.push(`${outbound.method} ${new URL(outbound.url).pathname}`);
    return new Response(null, { status: 204 });
  }, async () => {
    const request = await apiRequest("/api/v1/applications/app-123/previews/32", claims, {
      method: "DELETE",
    });
    const response = await worker.fetch(request, env);
    assert.equal(response.status, 204);

    const read = await worker.fetch(
      await apiRequest("/api/v1/deployments/applications/app-123", claims),
      env,
    );
    assert.equal(read.status, 403);

    const otherPr = await worker.fetch(
      await apiRequest("/api/v1/applications/app-123/previews/33", claims, {
        method: "DELETE",
      }),
      env,
    );
    assert.equal(otherPr.status, 403);
  });
  assert.deepEqual(calls, ["DELETE /api/v1/applications/app-123/previews/32"]);
});

test("preview DELETE with no PR history is an idempotent no-op", async () => {
  const claims = claimsFor(32);
  let calls = 0;
  await withFetch(async () => {
    calls += 1;
    return json({ deployments: [] });
  }, async () => {
    const request = await apiRequest("/api/v1/applications/app-123/previews/32", claims, {
      method: "DELETE",
    });
    const response = await worker.fetch(request, env);
    assert.equal(response.status, 404);
    assert.equal(await response.text(), "");
  });
  assert.equal(calls, 1);
});

test("preview DELETE fails closed when PR history is unavailable or malformed", async () => {
  const claims = claimsFor(32);
  for (const historyResponse of [
    () => json({ result: "unavailable" }, 503),
    () => json({ deployments: "not-an-array" }),
  ]) {
    let calls = 0;
    await withFetch(async () => {
      calls += 1;
      return historyResponse();
    }, async () => {
      const request = await apiRequest("/api/v1/applications/app-123/previews/32", claims, {
        method: "DELETE",
      });
      const response = await worker.fetch(request, env);
      assert.equal(response.status, 502);
      assert.deepEqual(await response.json(), {
        result: "error",
        reason: "history_unavailable",
      });
    });
    assert.equal(calls, 1);
  }
});

test("matching preview DELETE responses never proxy the upstream response body", async () => {
  const claims = claimsFor(32);
  const calls = [];
  await withFetch(async (outbound) => {
    calls.push(`${outbound.method} ${new URL(outbound.url).pathname}`);
    if (outbound.method === "GET") {
      return json({ deployments: [historyRow({ prNumber: 32, uuid: "deploy-32" })] });
    }
    return json({ secret: "delete-body-secret" }, 404);
  }, async () => {
    const request = await apiRequest("/api/v1/applications/app-123/previews/32", claims, {
      method: "DELETE",
    });
    const response = await worker.fetch(request, env);
    assert.equal(response.status, 404);
    assert.equal(await response.text(), "");
  });
  assert.deepEqual(calls, [
    "GET /api/v1/deployments/applications/app-123",
    "DELETE /api/v1/applications/app-123/previews/32",
  ]);
});

test("plaintext control-plane requests redirect before proxying", async () => {
  const response = await worker.fetch(new Request("http://coolify.proa.so/login?next=%2F"), {});
  assert.equal(response.status, 308);
  assert.equal(response.headers.get("location"), "https://coolify.proa.so/login?next=%2F");
});
