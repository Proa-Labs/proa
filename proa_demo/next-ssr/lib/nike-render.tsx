// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

import { createElement } from "react";
import { NikeCatalogPage } from "@shared/nike-catalog";
import { NikePdpPage } from "@shared/nike-pdp";
import { nikeComponentProps, wrapInNikeShell } from "./nike";

function nowUs(): number {
  return Number(process.hrtime.bigint()) / 1000;
}

export type TimedNikeHtml = {
  html: string;
  renderUs: number;
  docUs: number;
};

export async function renderNikeCatalogWithTimings(props: unknown): Promise<TimedNikeHtml> {
  const { renderToString } = await import("react-dom/server");
  const docStart = nowUs();
  const renderStart = nowUs();
  const bodyHtml = renderToString(createElement(NikeCatalogPage as any, nikeComponentProps(props)));
  const renderUs = nowUs() - renderStart;
  const html = wrapInNikeShell("Men's Shoes & Sneakers. Nike.com", bodyHtml);
  const docUs = nowUs() - docStart;
  return { html, renderUs, docUs };
}

export async function renderNikePdpWithTimings(props: unknown): Promise<TimedNikeHtml> {
  const { renderToString } = await import("react-dom/server");
  const docStart = nowUs();
  const renderStart = nowUs();
  const bodyHtml = renderToString(createElement(NikePdpPage as any, nikeComponentProps(props)));
  const renderUs = nowUs() - renderStart;
  const html = wrapInNikeShell("Nike Alphafly 3. Nike.com", bodyHtml);
  const docUs = nowUs() - docStart;
  return { html, renderUs, docUs };
}

export async function renderNikeCatalogHtml(props: unknown): Promise<string> {
  return (await renderNikeCatalogWithTimings(props)).html;
}

export async function renderNikePdpHtml(props: unknown): Promise<string> {
  return (await renderNikePdpWithTimings(props)).html;
}
