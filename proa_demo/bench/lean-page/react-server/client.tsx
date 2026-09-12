// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

// Client-side hydration entry. Reads `n` from a JSON seed tag and
// hydrates the existing DOM in place.

import { hydrateRoot } from "react-dom/client";
import { App } from "./page.tsx";

const seedEl = document.getElementById("__lean_data__");
const n: number = JSON.parse(seedEl?.textContent || "60");
const root = document.getElementById("__lean_root__");
if (root) {
    hydrateRoot(root, <App n={n} />);
}
