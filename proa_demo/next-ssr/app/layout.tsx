// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

import type { Metadata } from "next";
import type { ReactNode } from "react";
import { BODY_CLASS } from "@react-ssr/chrome";

export const metadata: Metadata = {
  title: "Proa Next.js SSR Demo",
  robots: {
    index: false,
    follow: false,
  },
};

export default function RootLayout({ children }: { children: ReactNode }) {
  return (
    <html lang="en">
      <body className={BODY_CLASS}>{children}</body>
    </html>
  );
}
