#!/usr/bin/env node
// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

import { execFileSync } from "node:child_process";
import { createHash } from "node:crypto";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import { chromium } from "playwright";

const scriptDir = dirname(fileURLToPath(import.meta.url));
const workspaceDir = resolve(scriptDir, "../..");
const pageSpecs = {
	"airbnb-home": {
		label: "Airbnb Home",
		rustBin: "airbnb_home_dump",
		reactDump: "dump-home.ts",
	},
	"nike-pdp": {
		label: "Nike PDP",
		rustBin: "nike_pdp_dump",
		reactDump: "dump-nike-pdp.ts",
	},
	"sentry-otel": {
		label: "Sentry OpenTelemetry Logs",
		rustBin: "sentry_otel_dump",
		reactDump: "dump-sentry-otel.ts",
		rustFeatures: "sentry-otel",
	},
};
const requestedPage = process.argv[2] ?? "airbnb-home";
const pageSpec = pageSpecs[requestedPage];

if (!pageSpec) {
	throw new Error(
		`unknown parity page ${requestedPage}; expected one of ${Object.keys(pageSpecs).join(", ")}`,
	);
}
const tsx = join(
	scriptDir,
	"node_modules",
	".bin",
	process.platform === "win32" ? "tsx.cmd" : "tsx",
);

function run(executable, args, options = {}) {
	return execFileSync(executable, args, {
		cwd: workspaceDir,
		encoding: "utf8",
		maxBuffer: 64 * 1024 * 1024,
		...options,
	});
}

function sha256(value) {
	return createHash("sha256").update(value).digest("hex");
}

function firstDifference(expected, actual) {
	const limit = Math.min(expected.length, actual.length);
	let index = 0;
	while (index < limit && expected[index] === actual[index]) index += 1;
	if (index === limit && expected.length === actual.length) return null;
	const start = Math.max(0, index - 120);
	const end = index + 120;
	return {
		index,
		expected: expected.slice(start, end),
		actual: actual.slice(start, end),
	};
}

const rustArgs = [
	"run",
	"--quiet",
	"--release",
	"--locked",
	"-p",
	"proa_showcase",
	"--no-default-features",
	...(pageSpec.rustFeatures ? ["--features", pageSpec.rustFeatures] : []),
	"--bin",
	pageSpec.rustBin,
];
const proaHtml = run("cargo", rustArgs);
const reactHtml = run(tsx, [pageSpec.reactDump], {
	cwd: scriptDir,
	env: { ...process.env, NODE_ENV: "production" },
});

const browser = await chromium.launch({ headless: true });
const canonical = new Map();

try {
	const context = await browser.newContext({ javaScriptEnabled: false });
	await context.route("**/*", (route) => route.abort());

	for (const [renderer, html] of [
		["proa", proaHtml],
		["react", reactHtml],
	]) {
		const page = await context.newPage();
		await page.setContent(html, { waitUntil: "domcontentloaded" });
		const document = await page.evaluate(() => {
			const preserveWhitespaceTags = new Set([
				"PRE",
				"SCRIPT",
				"STYLE",
				"TEXTAREA",
			]);
			const preserveWhitespaceModes = new Set([
				"pre",
				"pre-line",
				"pre-wrap",
				"break-spaces",
			]);
			// biome-ignore lint/suspicious/noControlCharactersInRegex: these are exactly HTML's collapsible ASCII whitespace characters.
			const collapsibleWhitespace = /[\u0009\u000a\u000c\u000d\u0020]+/gu;

			function normalizedText(value, preserve) {
				if (preserve) return value;
				return value
					.replace(collapsibleWhitespace, " ")
					.replace(/^ /u, "")
					.replace(/ $/u, "");
			}

			function compareAttribute(
				[leftName, leftValue],
				[rightName, rightValue],
			) {
				if (leftName < rightName) return -1;
				if (leftName > rightName) return 1;
				if (leftValue < rightValue) return -1;
				if (leftValue > rightValue) return 1;
				return 0;
			}

			function elementNode(element, inheritedPreserve = false) {
				const whiteSpace = getComputedStyle(element).whiteSpace;
				const preserve =
					inheritedPreserve ||
					preserveWhitespaceTags.has(element.tagName) ||
					preserveWhitespaceModes.has(whiteSpace);
				const attributes = [...element.attributes]
					.map(({ name, value }) => [name, value])
					.sort(compareAttribute);
				const children = [];
				let text = "";

				const flushText = () => {
					const value = normalizedText(text, preserve);
					text = "";
					if (value.length > 0) children.push(["#text", value]);
				};

				for (const child of element.childNodes) {
					if (child.nodeType === Node.COMMENT_NODE) continue;
					if (child.nodeType === Node.TEXT_NODE) {
						text += child.nodeValue ?? "";
						continue;
					}
					if (child.nodeType === Node.ELEMENT_NODE) {
						flushText();
						children.push(elementNode(child, preserve));
					}
				}
				flushText();

				const renderedText =
					element instanceof HTMLElement &&
					!["HEAD", "SCRIPT", "STYLE", "TEMPLATE"].includes(element.tagName)
						? normalizedText(element.innerText, preserve)
						: null;
				return [
					element.tagName.toLowerCase(),
					attributes,
					renderedText,
					children,
				];
			}

			const doctype = document.doctype
				? {
						name: document.doctype.name,
						publicId: document.doctype.publicId,
						systemId: document.doctype.systemId,
					}
				: null;
			return {
				compatMode: document.compatMode,
				doctype,
				tree: elementNode(document.documentElement),
			};
		});
		await page.close();
		canonical.set(renderer, JSON.stringify(document));
	}
	await context.close();
} finally {
	await browser.close();
}

const proaDom = canonical.get("proa");
const reactDom = canonical.get("react");
const report = {
	proa: {
		raw_bytes: Buffer.byteLength(proaHtml),
		raw_sha256: sha256(proaHtml),
		normalized_dom_sha256: sha256(proaDom),
	},
	react: {
		raw_bytes: Buffer.byteLength(reactHtml),
		raw_sha256: sha256(reactHtml),
		normalized_dom_sha256: sha256(reactDom),
	},
};

if (proaDom !== reactDom) {
	const difference = firstDifference(proaDom, reactDom);
	process.stderr.write(
		`${pageSpec.label} normalized DOM differs at ${difference.index}\n` +
			`Proa: ${difference.expected}\n` +
			`React: ${difference.actual}\n` +
			`${JSON.stringify(report, null, 2)}\n`,
	);
	process.exitCode = 1;
} else {
	process.stdout.write(
		`${pageSpec.label} normalized DOM parity passed\n${JSON.stringify(report, null, 2)}\n`,
	);
}
