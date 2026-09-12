// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

"use client";

import { App } from "@react-ssr/page";
import type { Listing } from "@lib/ebay";

export function EbayClient({ listings }: { listings: Listing[] }) {
  return <App listings={listings} />;
}
