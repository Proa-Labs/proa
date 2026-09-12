// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

// Client-side hydration entry. Reads the SSR'd seed data from a
// `<script type="application/json" id="__app_data__">` tag, then
// hydrates the existing DOM in place.

import { hydrateRoot } from "react-dom/client";
import { App } from "./page.tsx";
import type { Listing } from "./page.tsx";

const seedEl = document.getElementById("__app_data__");
const listings: Listing[] = JSON.parse(seedEl?.textContent || "[]");
const root = document.getElementById("__app_root__");
if (root) {
    hydrateRoot(root, <App listings={listings} />);
}
