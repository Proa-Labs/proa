// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

// Import the inert home-rail-section rendered by proa-ui/marketing/src/pages/home.rs.
// Run against a locally built Proa UI marketing server:
//   node scripts/import-proa-ui-rail.mjs http://127.0.0.1:3333
// Only this refresh step needs the source application; Proa builds independently.
import { mkdir, writeFile } from "node:fs/promises";
import { chromium } from "playwright";

const source = process.argv[2];
if (!source) throw new Error("Pass the URL of the Proa UI marketing server.");
const browser = await chromium.launch({ headless: true });
try {
  const page = await browser.newPage();
  await page.goto(source, { waitUntil: "networkidle" });
  const result = await page.evaluate(() => {
    const rail = document.querySelector(".home-rail-section");
    if (!rail) throw new Error("The source page has no home-rail-section.");
    const scope = "#proa-ui-components .home-rail-section";
    const buckets = new Map();
    const keyframes = new Set();
    const globals = new Set([":root", ":root.dark", ":host", "html", "body", ".dark", ".home-page", ".dark .home-page"]);

    function selectors(value) {
      const parts = [];
      let depth = 0, quote = "", start = 0;
      for (let i = 0; i < value.length; i++) {
        const ch = value[i];
        if (ch === "\\") { i++; continue; }
        if (quote) { if (ch === quote) quote = ""; continue; }
        if (ch === '"' || ch === "'") quote = ch;
        else if (ch === "(" || ch === "[") depth++;
        else if (ch === ")" || ch === "]") depth--;
        else if (ch === "," && depth === 0) { parts.push(value.slice(start, i).trim()); start = i + 1; }
      }
      parts.push(value.slice(start).trim());
      return parts;
    }

    function applies(selector) {
      if (globals.has(selector)) return true;
      const probe = selector
        .replace(/::[\w-]+(?:\([^)]*\))?/g, "")
        .replace(/:(?:hover|focus-visible|focus-within|focus|active|visited|disabled|enabled|checked|indeterminate|open)\b/g, "") || "*";
      try {
        return [...document.querySelectorAll(probe)].some(el => el === rail || rail.contains(el));
      } catch { return false; }
    }

    function scoped(selector) {
      if (globals.has(selector)) return selector.includes(".dark") ? `.dark ${scope}` : scope;
      if (selector === ".home-rail-section") return scope;
      if (selector.startsWith(".dark ")) return `.dark ${scope} ${selector.slice(6)}`;
      return `${scope} ${selector}`;
    }

    function walk(rules, layer = "", wrappers = []) {
      for (const rule of rules) {
        const kind = rule.constructor.name;
        if (kind === "CSSLayerBlockRule") {
          walk(rule.cssRules, rule.name, wrappers);
          continue;
        }
        if (kind === "CSSLayerStatementRule" || kind === "CSSFontFaceRule") continue;
        let css;
        if (kind === "CSSStyleRule") {
          const selected = selectors(rule.selectorText).filter(applies);
          if (!selected.length) continue;
          css = selected.map(scoped).join(", ") + rule.cssText.slice(rule.cssText.indexOf(" {"));
        } else if (kind === "CSSKeyframesRule" || kind === "CSSPropertyRule") {
          if (kind === "CSSKeyframesRule") keyframes.add(rule.name);
          css = rule.cssText;
        } else if (rule.cssRules) {
          walk(rule.cssRules, layer, [...wrappers, rule.cssText.slice(0, rule.cssText.indexOf("{"))]);
          continue;
        } else continue;
        for (const wrapper of [...wrappers].reverse()) css = `${wrapper}{\n${css}\n}`;
        if (!buckets.has(layer)) buckets.set(layer, new Set());
        buckets.get(layer).add(css);
      }
    }

    // Capture both themes, retaining nested hover/focus rules and every breakpoint.
    for (const dark of [false, true]) {
      document.documentElement.classList.toggle("dark", dark);
      for (const sheet of document.styleSheets) walk(sheet.cssRules);
    }
    // Flatten the source layers in cascade order so the host's unlayered
    // Tailwind v3 rules cannot override the imported Tailwind v4 utilities.
    let css = ["properties", "theme", "base", "proa-compat", "components", "utilities", ""]
      .flatMap(layer => [...(buckets.get(layer) || [])]).join("\n");
    css = css.replaceAll("--tw-", "--ui-rail-tw-");
    for (const name of keyframes) {
      css = css.replace(new RegExp(`(?<![\\w-])${name.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")}(?![\\w-])`, "g"), `ui-rail-${name}`);
    }

    const copy = rail.cloneNode(true);
    // SVG <title>s give no tooltip or accessible name inside the inert rail,
    // and crawlers that read every <title> mistake them for the page title.
    copy.querySelectorAll("script, style, svg title").forEach(el => el.remove());
    for (const el of copy.querySelectorAll("*")) {
      for (const attr of [...el.attributes]) {
        if (attr.name.startsWith("data-rsjs-") || attr.name.startsWith("on")) el.removeAttribute(attr.name);
      }
    }
    for (const link of copy.querySelectorAll("a[href]")) {
      link.setAttribute("href", new URL(link.getAttribute("href"), "https://ui.proa.so").href);
      link.setAttribute("target", "_blank");
      link.setAttribute("rel", "noopener noreferrer");
    }
    if (copy.querySelector("img, video, iframe, link, script")) {
      throw new Error("The rail gained a resource that needs to be imported explicitly.");
    }
    return { html: copy.outerHTML, css: css.replace(/[ \t]+$/gm, ""), tiles: copy.querySelectorAll(".home-rail-item").length };
  });
  const base = new URL("../proa_marketing/", import.meta.url);
  await mkdir(new URL("src/pages/home/", base), { recursive: true });
  await writeFile(new URL("src/pages/home/proa_ui_rail.html", base),
    "<!-- Proa UI marketing home-rail-section. Refresh with scripts/import-proa-ui-rail.mjs. -->\n" + result.html + "\n");
  await writeFile(new URL("public/proa-ui-rail.css", base),
    "/* Proa UI rail styles, scoped for this homepage. Generated by scripts/import-proa-ui-rail.mjs. */\n" +
    result.css + "\n" +
    "/* Show the host section background and match the rail's edge fade to its theme. */\n" +
    "#proa-ui-components .home-rail-section { --home-page-bg: #fff; background: transparent; }\n" +
    ".dark #proa-ui-components .home-rail-section { --home-page-bg: #0b0b0d; background: transparent; }\n");
  console.log(`Imported ${result.tiles} rail tiles and their scoped styles.`);
} finally {
  await browser.close();
}
