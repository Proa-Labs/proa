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
  const chartLink = page.getByRole("link", { name: "Open full-size SVG" });
  assert.equal(await chartLink.getAttribute("href"),
    await page.locator("#benchmark-panel-http img").getAttribute("src"));
  await page.locator("#benchmark-tab-http").press("Home");
  await page.locator("#benchmark-panel-render").waitFor({ state: "visible" });
  assert.equal(await chartLink.getAttribute("href"),
    await page.locator("#benchmark-panel-render img").getAttribute("src"));
  await page.locator("[data-search-open]:visible").first().click();
  await page.locator("#search-input").fill("streaming");
  await page.locator("[data-search-hit]").first().waitFor({ state: "visible" });

  await page.goto(`${base}/showcase/espn/proa`, { waitUntil: "networkidle" });
  const box = page.getByRole("button", { name: "Box Score", exact: true });
  await box.click();
  await page.locator('[data-panel="box-score"]').waitFor({ state: "visible" });
  assert.equal(await box.getAttribute("aria-selected"), "true");
  assert.equal(await page.locator('[data-panel="gamecast"]').isVisible(), false);
  await page.getByRole("button", { name: "Gamecast", exact: true }).click();
  await page.locator('[data-panel="gamecast"]').waitFor({ state: "visible" });
  assert.equal(await page.locator('[data-panel="box-score"]').isVisible(), false);
  await page.goto(`${base}/showcase/airbnb/proa/listing`, { waitUntil: "domcontentloaded" });
  const dates = page.locator("[data-listing-date-picker]");
  const booking = page.locator("[data-airbnb-booking-root]");
  const popover = booking.locator("[data-booking-popover]");
  const day = (calendar, month, date) =>
    calendar.locator(`[data-month="${month}"] [data-calendar-day="${date}"]`);
  const displayedDate = (field) => booking.locator(`[data-booking-display="${field}"]`);
  const total = booking.locator("[data-booking-total-value]");
  assert.equal(await displayedDate("checkIn").innerText(), "Add date");
  await day(dates, 3, 27).click();
  await booking.locator('[data-booking-trigger="checkOut"]').click();
  await popover.waitFor({ state: "visible" });
  await day(popover, 4, 3).click();
  assert.equal(await displayedDate("checkIn").innerText(), "3/27/2026");
  assert.equal(await displayedDate("checkOut").innerText(), "4/3/2026");
  assert.equal(await day(dates, 4, 3).getAttribute("aria-pressed"), "true");
  assert.equal(await day(popover, 3, 28).getAttribute("data-in-range"), "true");
  assert.equal((await total.innerText()).replaceAll(",", ""), "$8214");

  await popover.locator('[data-booking-clear-field="checkOut"]').click();
  assert.equal(await displayedDate("checkIn").innerText(), "3/27/2026");
  assert.equal(await displayedDate("checkOut").innerText(), "Add date");
  assert.equal(await total.innerText(), "$0");
  await day(popover, 3, 25).click();
  await day(popover, 3, 26).click();
  assert.equal((await total.innerText()).replaceAll(",", ""), "$1962");
  await page.keyboard.press("Escape");
  await popover.waitFor({ state: "hidden" });
  await dates.locator("[data-booking-clear-all]").click();
  assert.equal(await popover.isVisible(), false, "Clearing dates in the listing calendar should keep the popover closed");
  assert.equal(await displayedDate("checkIn").innerText(), "Add date");
  assert.equal(await displayedDate("checkOut").innerText(), "Add date");
  assert.equal(await total.innerText(), "$0");

  const mobile = await browser.newPage({ viewport: { width: 390, height: 844 }, isMobile: true, hasTouch: true });
  mobile.on("pageerror", (error) => errors.push(error.message));
  await mobile.goto(`${base}/showcase/airbnb/proa/listing`, { waitUntil: "domcontentloaded" });
  await mobile.locator('[data-booking-trigger="checkIn"]').tap();
  const mobilePopover = mobile.locator("[data-booking-popover]");
  await mobilePopover.waitFor({ state: "visible" });
  const bounds = await mobilePopover.boundingBox();
  assert.ok(bounds.x >= 0 && bounds.y >= 0 && bounds.x + bounds.width <= 390 && bounds.y + bounds.height <= 844);
  await day(mobilePopover, 4, 14).tap();
  await day(mobilePopover, 4, 16).tap();
  assert.equal((await mobile.locator("[data-booking-total-value]").innerText()).replaceAll(",", ""), "$3004");
  await mobilePopover.locator("[data-booking-close]").tap();
  await mobilePopover.waitFor({ state: "hidden" });
  await mobile.close();

  assert.deepEqual(errors, [], "packaged RSJS assets must hydrate without runtime errors");
  console.log("Browser smoke passed: homepage navigation, ESPN panels, and Airbnb booking controls hydrate correctly.");
} finally {
  await browser.close();
}
