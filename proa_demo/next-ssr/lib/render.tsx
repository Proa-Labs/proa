// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

import { buildEbayListingsFromProps } from "./ebay";
import { renderEbayDocumentWithTimings } from "./ebay-render";
import { renderNikeCatalogWithTimings, renderNikePdpWithTimings } from "./nike-render";

export function nowUs(): number {
  return Number(process.hrtime.bigint()) / 1000;
}

export type RenderTimings = {
  html: string;
  renderUs: number;
  docUs: number;
};

export async function renderPageWithTimings(
  page: string,
  props: unknown,
): Promise<RenderTimings> {
  switch (page) {
    case "ebay-srp":
    case "ebay-srp-rsjs":
      return renderEbayDocumentWithTimings(await buildEbayListingsFromProps(props));
    case "nike-catalog":
      return renderNikeCatalogWithTimings(props);
    case "nike-pdp":
      return renderNikePdpWithTimings(props);
    default:
      throw new Error(`Unknown page: ${page}`);
  }
}

export async function renderPageToHtml(page: string, props: unknown): Promise<string> {
  return (await renderPageWithTimings(page, props)).html;
}
