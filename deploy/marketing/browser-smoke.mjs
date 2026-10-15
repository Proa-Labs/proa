#!/usr/bin/env node
// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

import assert from "node:assert/strict";
const { chromium } = await import(process.env.PLAYWRIGHT_MODULE || "playwright");
const base = process.argv[2] || "http://127.0.0.1:5000";
const browser = await chromium.launch({ headless: true });
try {
  const page = await browser.newPage();
  const errors = [];
  page.on("pageerror", (error) => errors.push(error.message));
  await page.goto(base, { waitUntil: "networkidle" });
  await page.locator("#benchmark-tab-http").click();
  await page.locator("#benchmark-panel-http").waitFor({ state: "visible" });
  assert.equal(await page.locator("#benchmark-tab-http").getAttribute("aria-selected"), "true");
  assert.equal(await page.locator("#benchmark-panel-render").isVisible(), false);
  await page.locator("#benchmark-tab-http").press("Home");
  await page.locator("#benchmark-panel-render").waitFor({ state: "visible" });
  await page.goto(`${base}/showcase/espn/proa`, { waitUntil: "networkidle" });
  const box = page.getByRole("button", { name: "Box Score", exact: true });
  await box.click();
  await page.locator('[data-panel="box-score"]').waitFor({ state: "visible" });
  assert.equal(await box.getAttribute("aria-selected"), "true");
  assert.equal(await page.locator('[data-panel="gamecast"]').isVisible(), false);
  await page.getByRole("button", { name: "Gamecast", exact: true }).click();
  await page.locator('[data-panel="gamecast"]').waitFor({ state: "visible" });
  assert.equal(await page.locator('[data-panel="box-score"]').isVisible(), false);
  assert.deepEqual(errors, [], "packaged RSJS assets must hydrate without runtime errors");
  console.log("Browser smoke passed: homepage keyboard navigation and ESPN panels hydrate correctly.");
} finally {
  await browser.close();
}
