// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

// Fully dynamic eBay SRP chrome: global header, search, mega-nav, refine
// rail, result controls, pagination, related searches, footer. Every element
// is rendered per request (no pre-rendered shell strings), with refinement
// counts derived from the listing dataset so the chrome does real work.
//
// Constant tables and the meta derivation live in
// `../marko-server/helpers_srp_real.mjs` (single JS source of truth shared
// with the Marko engine); `proa_showcase/src/ebay/srp_chrome.rs` mirrors both
// the tables and this markup element-for-element. Keep them in sync or the
// cross-engine byte/element parity check fails.
//
// Components here are hook-free so Next.js can render them as server
// components; react-ssr renders them inside SrpDocument.

import type { ReactNode } from "react";
import type { Listing } from "./page.tsx";
import * as shared from "../marko-server/helpers_srp_real.mjs";

export const DEFAULT_QUERY: string = shared.DEFAULT_QUERY;
export const PER_PAGE_OPTIONS: number[] = shared.PER_PAGE_OPTIONS;
export const BODY_CLASS = "s-page no-touch skin-large srp--list-view srp-mag-ui-variant2 srp-item-compare";

const {
  fmtCount,
  TOPBAR_LEFT,
  TOPBAR_RIGHT,
  SEARCH_CATS,
  NAV_CATS,
  MEGANAV,
  RAIL_CATS,
  RELATED_SEARCHES,
  FOOTER_COLS,
  FOOTER_LEGAL,
  SORT_OPTIONS,
} = shared;

export interface SrpMeta {
  total: number;
  cartCount: number;
  conditionCounts: Array<[string, number]>;
  formatCounts: Array<[string, number]>;
  priceCounts: Array<[string, number]>;
  deliveryCounts: Array<[string, number]>;
  locationCounts: Array<[string, number]>;
  ratingCounts: Array<[string, number]>;
}

export function deriveSrpMeta(listings: Listing[]): SrpMeta {
  return shared.deriveSrpMeta(listings) as SrpMeta;
}

// ---------------------------------------------------------------------------
// Components
// ---------------------------------------------------------------------------

function BellIcon() {
  return (
    <svg className="gh-icon" viewBox="0 0 24 24" aria-hidden="true" focusable="false">
      <path d="M12 22a2.4 2.4 0 0 0 2.4-2.4H9.6A2.4 2.4 0 0 0 12 22zm7-5.2v-1l-1.6-1.6v-4.6a5.4 5.4 0 0 0-4-5.5V3.4a1.4 1.4 0 1 0-2.8 0v.7a5.4 5.4 0 0 0-4 5.5v4.6L5 15.8v1z" />
    </svg>
  );
}

function CartIcon() {
  return (
    <svg className="gh-icon" viewBox="0 0 24 24" aria-hidden="true" focusable="false">
      <path d="M7 20a1.6 1.6 0 1 0 0-3.2A1.6 1.6 0 0 0 7 20zm10 0a1.6 1.6 0 1 0 0-3.2 1.6 1.6 0 0 0 0 3.2zM3 3h2.2l2.5 10.4a2 2 0 0 0 2 1.6h7.6a2 2 0 0 0 2-1.5L21 6H6.2" />
    </svg>
  );
}

function SearchIcon() {
  return (
    <svg className="gh-search__icon-svg" viewBox="0 0 24 24" aria-hidden="true" focusable="false">
      <path d="M10.5 3a7.5 7.5 0 1 0 4.7 13.4l4.7 4.7 1.4-1.4-4.7-4.7A7.5 7.5 0 0 0 10.5 3z" />
    </svg>
  );
}

export function SrpHeader({ query, meta }: { query: string; meta: SrpMeta }) {
  return (
    <header className="gh">
      <nav className="gh-topbar" aria-label="Account and shopping shortcuts">
        <ul className="gh-topbar__nav">
          {TOPBAR_LEFT.map(([label, href]: [string, string]) => (
            <li key={label} className="gh-topbar__item">
              <a href={href}>{label}</a>
            </li>
          ))}
        </ul>
        <ul className="gh-topbar__nav gh-topbar__nav--right">
          {TOPBAR_RIGHT.map(([label, href]: [string, string]) => (
            <li key={label} className="gh-topbar__item">
              <a href={href}>{label}</a>
            </li>
          ))}
          <li className="gh-topbar__item gh-topbar__item--icon">
            <a href="#notifications" aria-label="Notifications">
              <BellIcon />
            </a>
          </li>
          <li className="gh-topbar__item gh-topbar__item--icon">
            <a href="#cart" aria-label="Your shopping cart">
              <CartIcon />
              <span className="gh-cart-badge">{meta.cartCount}</span>
            </a>
          </li>
        </ul>
      </nav>
      <div className="gh-main">
        <a className="gh-logo" href="#home" aria-label="eBay Home">
          <svg viewBox="0 0 122 48" aria-hidden="true" focusable="false">
            <path className="gh-logo__e" d="M9 18c-5 0-9 3-9 8 0 6 4 9 9 9 4 0 7-2 8-5h-5c-1 1-2 2-3 2-3 0-4-2-4-4h12v-2c0-5-3-8-8-8z" />
            <path className="gh-logo__b" d="M21 8h5v12c1-2 3-3 6-3 5 0 8 4 8 9s-3 9-8 9c-3 0-5-1-6-3v2h-5z" />
            <path className="gh-logo__a" d="M50 17c-5 0-8 2-8 6h5c0-2 1-2 3-2s3 1 3 3v1h-3c-5 0-9 2-9 6 0 3 3 5 7 5 3 0 4-1 6-3v2h5V23c0-4-3-6-9-6z" />
            <path className="gh-logo__y" d="M61 18h6l4 11 4-11h6l-9 22h-6l3-7z" />
          </svg>
        </a>
        <div className="gh-search">
          <form className="gh-search__form" action="#search" method="get">
            <span className="gh-search__icon">
              <SearchIcon />
            </span>
            <input
              className="gh-search__input"
              type="text"
              name="q"
              defaultValue={query}
              placeholder="Search for anything"
              aria-label="Search for anything"
            />
            <select className="gh-search__category" aria-label="Select a category for search" defaultValue={SEARCH_CATS[1]}>
              {SEARCH_CATS.map((cat: string) => (
                <option key={cat} value={cat}>
                  {cat}
                </option>
              ))}
            </select>
            <button className="gh-search__btn" type="submit">
              Search
            </button>
          </form>
          <a className="gh-search__advanced" href="#advanced">
            Advanced
          </a>
        </div>
      </div>
      <nav className="gh-cats" aria-label="Categories">
        <ul>
          {NAV_CATS.map(([label, href]: [string, string]) => (
            <li key={label}>
              <a href={href}>{label}</a>
            </li>
          ))}
        </ul>
      </nav>
      <div className="gh-meganav" aria-hidden="false">
        {MEGANAV.map(([title, links]: [string, string[]]) => (
          <section key={title} className="gh-meganav__section">
            <h3>{title}</h3>
            <ul>
              {links.map((link) => (
                <li key={link}>
                  <a href="#category">{link}</a>
                </li>
              ))}
            </ul>
          </section>
        ))}
      </div>
    </header>
  );
}

export function SrpControls({ query, meta, perPage }: { query: string; meta: SrpMeta; perPage: number }) {
  return (
    <div className="srp-controls">
      <nav className="srp-controls__breadcrumb" aria-label="Breadcrumb">
        <ol>
          <li>
            <a href="#home">eBay</a>
          </li>
          <li>
            <a href="#cellphones">Cell Phones & Accessories</a>
          </li>
          <li>
            <a href="#smartphones">Cell Phones & Smartphones</a>
          </li>
        </ol>
      </nav>
      <h1 className="srp-controls__count">
        <span className="srp-controls__count-bold">{fmtCount(meta.total)}</span> results for{" "}
        <span className="srp-controls__count-bold">{query}</span>
      </h1>
      <div className="srp-controls__row">
        <button type="button" className="srp-controls__save">
          <svg className="gh-icon" viewBox="0 0 24 24" aria-hidden="true" focusable="false">
            <path d="M12 21s-7.5-4.8-10-9.3C.5 8 2.6 4.5 6.2 4.5c2.2 0 3.9 1.2 4.8 3 .9-1.8 2.6-3 4.8-3 3.6 0 5.7 3.5 4.2 7.2C19.5 16.2 12 21 12 21z" />
          </svg>
          <span>Save this search</span>
        </button>
        <label className="srp-controls__sort">
          <span>Sort:</span>
          <select defaultValue={SORT_OPTIONS[0]} aria-label="Sort selector">
            {SORT_OPTIONS.map((opt: string) => (
              <option key={opt} value={opt}>
                {opt}
              </option>
            ))}
          </select>
        </label>
        <label className="srp-controls__perpage">
          <span>Items per page:</span>
          <select defaultValue={String(perPage)} aria-label="Items per page">
            {PER_PAGE_OPTIONS.map((opt) => (
              <option key={opt} value={String(opt)}>
                {opt}
              </option>
            ))}
          </select>
        </label>
      </div>
    </div>
  );
}

function RefineGroup({ title, rows }: { title: string; rows: Array<[string, number]> }) {
  return (
    <div className="x-refine__group">
      <h3 className="x-refine__title">{title}</h3>
      <ul>
        {rows.map(([label, count]) => (
          <li key={label} className="x-refine__item">
            <label className="x-refine__label">
              <input type="checkbox" className="x-refine__input" />
              <span className="x-refine__text">{label}</span>
              <span className="x-refine__count">({fmtCount(count)})</span>
            </label>
          </li>
        ))}
      </ul>
    </div>
  );
}

export function SrpRail({ meta }: { meta: SrpMeta }) {
  const n = meta.formatCounts[0][1];
  return (
    <aside className="srp-rail" aria-label="Refine results">
      <h2 className="srp-rail__heading">Shop by category</h2>
      <ul className="srp-rail__cats">
        {RAIL_CATS.map(([label, weight]: [string, number]) => (
          <li key={label}>
            <a href="#refine-category">{label}</a>
            <span className="x-refine__count">({fmtCount(weight * n)})</span>
          </li>
        ))}
      </ul>
      <RefineGroup title="Buying Format" rows={meta.formatCounts} />
      <RefineGroup title="Condition" rows={meta.conditionCounts} />
      <RefineGroup title="Price" rows={meta.priceCounts} />
      <RefineGroup title="Delivery Options" rows={meta.deliveryCounts} />
      <RefineGroup title="Item Location" rows={meta.locationCounts} />
      <RefineGroup title="Seller Rating" rows={meta.ratingCounts} />
      <div className="srp-rail__cta">
        <a href="#completed">Completed items</a>
        <a href="#sold">Sold items</a>
        <a href="#deals">Deals & savings</a>
      </div>
    </aside>
  );
}

export function SrpPagination({ meta, perPage }: { meta: SrpMeta; perPage: number }) {
  const pages = Math.max(1, Math.ceil(meta.total / perPage));
  const shown = Math.min(7, pages);
  const pageItems = [];
  for (let p = 1; p <= shown; p++) pageItems.push(p);
  return (
    <nav className="srp-pagination" aria-label="Result pagination">
      <a className="srp-pagination__nav srp-pagination__nav--prev" href="#prev" aria-label="Previous page">
        <svg className="gh-icon" viewBox="0 0 24 24" aria-hidden="true" focusable="false">
          <path d="M15 4l-8 8 8 8" />
        </svg>
      </a>
      {pageItems.map((p) => (
        <a key={p} className={p === 1 ? "srp-pagination__page srp-pagination__page--active" : "srp-pagination__page"} href="#page">
          {p}
        </a>
      ))}
      <span className="srp-pagination__ellipsis">…</span>
      <a className="srp-pagination__page" href="#page-last">
        {fmtCount(pages)}
      </a>
      <a className="srp-pagination__nav srp-pagination__nav--next" href="#next" aria-label="Next page">
        <svg className="gh-icon" viewBox="0 0 24 24" aria-hidden="true" focusable="false">
          <path d="M9 4l8 8-8 8" />
        </svg>
      </a>
    </nav>
  );
}

export function SrpRelated() {
  return (
    <section className="srp-related" aria-label="Related searches">
      <h2>Related searches</h2>
      <ul>
        {RELATED_SEARCHES.map((term: string) => (
          <li key={term}>
            <a href="#related">{term}</a>
          </li>
        ))}
      </ul>
    </section>
  );
}

export function SrpFooter() {
  return (
    <footer className="gh-footer">
      <div className="gh-footer__cols">
        {FOOTER_COLS.map(([title, links]: [string, string[]]) => (
          <div key={title} className="gh-footer__col">
            <h3>{title}</h3>
            <ul>
              {links.map((link) => (
                <li key={link}>
                  <a href="#footer-link">{link}</a>
                </li>
              ))}
            </ul>
          </div>
        ))}
      </div>
      <div className="gh-footer__legal">
        {FOOTER_LEGAL.map((label: string) => (
          <a key={label} href="#legal">
            {label}
          </a>
        ))}
        <span>Copyright © 1995-2026 eBay Inc. All Rights Reserved.</span>
      </div>
    </footer>
  );
}

/// Body-level page skeleton shared by react-ssr and next-ssr: chrome around
/// the interactive river region (filter chips + listing cards), which the
/// caller passes as children so each app controls its own hydration boundary.
export function SrpBody({
  query,
  meta,
  perPage,
  children,
}: {
  query: string;
  meta: SrpMeta;
  perPage: number;
  children: ReactNode;
}) {
  return (
    <div className="srp-page">
      <SrpHeader query={query} meta={meta} />
      <main className="srp">
        <SrpControls query={query} meta={meta} perPage={perPage} />
        <div className="srp-layout">
          <SrpRail meta={meta} />
          <section className="srp-river">{children}</section>
        </div>
        <SrpPagination meta={meta} perPage={perPage} />
        <SrpRelated />
      </main>
      <SrpFooter />
    </div>
  );
}
