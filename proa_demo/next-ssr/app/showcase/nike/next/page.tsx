// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

import type { Metadata } from "next";
import { NikeCatalogPage } from "@shared/nike-catalog";
import { NIKE_NEXT_CSS, nikeComponentProps } from "@lib/nike";
import { loadDefaultProps } from "@lib/props";

export const dynamic = "force-dynamic";
export const runtime = "nodejs";

export const metadata: Metadata = {
  title: "Men's Shoes & Sneakers. Nike.com",
};

export default function NikeCatalogNextPage() {
  const props = nikeComponentProps(loadDefaultProps()["nike-catalog"]);

  return (
    <>
      <style dangerouslySetInnerHTML={{ __html: NIKE_NEXT_CSS }} />
      <NikeCatalogPage {...(props as any)} />
    </>
  );
}
