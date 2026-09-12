// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

import { createElement } from "react";
import { renderToString } from "react-dom/server";

import { NikePdpPage } from "./src/nike-pdp";
import { NIKE_PDP_TITLE, wrapInNikeShell } from "./src/nike-shell";

const body = renderToString(createElement(NikePdpPage));
process.stdout.write(wrapInNikeShell(NIKE_PDP_TITLE, body));
