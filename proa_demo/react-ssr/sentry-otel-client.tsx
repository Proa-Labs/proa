// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

import { hydrateRoot } from "react-dom/client";
import { SentryOtelDocument, type SentryOtelProps } from "../react-ssr-shared/src/sentry-otel";

const css = document.querySelector("#sentry-otel-css")?.textContent ?? "";
const seed = document.querySelector("#sentry-otel-props")?.textContent ?? "{}";
const props = JSON.parse(seed) as SentryOtelProps;

hydrateRoot(document, <SentryOtelDocument
  css={css}
  {...props}
/>);
