// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

import { createElement } from "react";
import { renderToString } from "react-dom/server";

import { HomePage } from "./src/home";
import { wrapInDocumentShell } from "./src/shared";

const title = "Airbnb - Vacation Rentals, Cabins, Beach Houses";
const body = renderToString(createElement(HomePage));
process.stdout.write(wrapInDocumentShell(title, body));
