// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

import { readFileSync } from "node:fs";
import { createElement } from "react";
import { renderToString } from "react-dom/server";

import {
	buildDefaultSentryLogs,
	SentryOtelDocument,
} from "./src/sentry-otel";

const css = readFileSync(
	new URL(
		"../../proa_showcase/src/sentry_otel/sentry_otel.css",
		import.meta.url,
	),
	"utf8",
);
const element = createElement(SentryOtelDocument, {
	css,
	logs: buildDefaultSentryLogs(400),
	hydrate: false,
});

process.stdout.write(`<!DOCTYPE html>${renderToString(element)}`);
