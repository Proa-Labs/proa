// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

// Lean React component: a single bench grid of N WatchHeart toggles.
// Mirrors the proa-lean and marko-lean shapes — one button per item,
// one boolean state, two heart glyphs, no third-party JS.

import { useState } from "react";
import type { ReactNode } from "react";

function WatchHeart({ ariaLabel }: { ariaLabel: string }): ReactNode {
    const [watching, setWatching] = useState(false);
    return (
        <button
            type="button"
            className="lean-watch"
            aria-label={ariaLabel}
            onClick={() => setWatching((w) => !w)}
        >
            <span className={watching ? "heart heart--on" : "heart heart--off"} aria-hidden="true">
                {watching ? "♥" : "♡"}
            </span>
        </button>
    );
}

export function App({ n }: { n: number }): ReactNode {
    const items = Array.from({ length: n }, (_, i) => i);
    return (
        <>
            {items.map((i) => (
                <WatchHeart key={i} ariaLabel={`watch ${i}`} />
            ))}
        </>
    );
}
