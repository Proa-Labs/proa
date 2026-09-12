// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

import type { Metadata } from "next";
import { NikePdpPage } from "@shared/nike-pdp";
import { NIKE_NEXT_CSS, nikeComponentProps } from "@lib/nike";
import { loadDefaultProps } from "@lib/props";

export const dynamic = "force-dynamic";
export const runtime = "nodejs";

export const metadata: Metadata = {
  title: "Nike Alphafly 3. Nike.com",
};

export default function NikePdpNextPage() {
  const props = nikeComponentProps(loadDefaultProps()["nike-pdp"]);

  return (
    <>
      <style dangerouslySetInnerHTML={{ __html: NIKE_NEXT_CSS }} />
      <NikePdpPage {...(props as any)} />
    </>
  );
}
