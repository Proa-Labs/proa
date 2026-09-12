// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

// Shared components for the React SSR benchmark.
// Mirrors the Proa Airbnb showcase shared image and layout helpers.
// so that renderToString() produces structurally identical HTML to Proa.

import React from "react";
import { SHARED_CSS } from "./css";
import { DEFAULT_FALLBACK_SRC } from "./data";
import { AirbnbLogoSvg, GlobeSvg } from "./svg-elements";

// ---------------------------------------------------------------------------
// DocumentShell — wraps body HTML in the full HTML document (string concat,
// matching Proa's approach which also concatenates static strings)
// ---------------------------------------------------------------------------

const DOCUMENT_START = `<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="UTF-8"/>
<meta name="viewport" content="width=device-width, initial-scale=1.0"/>
<meta name="turbo-refresh-method" content="morph"/>
<meta name="turbo-refresh-scroll" content="preserve"/>
<script type="importmap">{"imports":{"react":"https://esm.sh/react@19","react/jsx-runtime":"https://esm.sh/react@19/jsx-runtime","react-dom":"https://esm.sh/react-dom@19","react-dom/client":"https://esm.sh/react-dom@19/client"}}</script>
<link rel="stylesheet" href="/proa.css"/>
<script src="/proa.js" defer></script>
<script type="module" src="/assets/airbnb-showcase.js"></script>

<script type="module">import * as Turbo from "https://esm.sh/@hotwired/turbo@8.0.12";</script>
<script src="https://cdn.tailwindcss.com"></script>
`;

const BODY_OPEN = `</head><body class="bg-white text-[#222] air-demo-page">`;
const DOCUMENT_END = "</body></html>";

export function wrapInDocumentShell(title: string, bodyHtml: string): string {
  return (
    DOCUMENT_START +
    "<title>" + title + "</title>\n" +
    "<style>" + SHARED_CSS + "</style>\n" +
    BODY_OPEN +
    bodyHtml +
    DOCUMENT_END
  );
}

// ---------------------------------------------------------------------------
// Header
// ---------------------------------------------------------------------------

export function Header() {
  return (
    <header className="air-topbar">
      <div className="air-shell">
        <div className="air-topbar-inner grid grid-cols-[auto_1fr_auto] items-center gap-5">
          <a
            href="/showcase/airbnb/proa"
            className="flex items-center gap-2.5 text-[#FF385C] font-semibold text-[28px] leading-none tracking-[-0.02em]"
          >
            <AirbnbLogoSvg />
            <span className="text-[30px] leading-none">airbnb</span>
          </a>

          <div className="flex justify-center">
            <a
              href="/showcase/airbnb/proa/search"
              className="air-search-pill hidden md:flex items-center h-12 rounded-full border border-[var(--air-line)] bg-white px-2"
            >
              <span className="px-4 text-sm font-semibold">Anywhere</span>
              <span className="h-6 w-px bg-[var(--air-line)]"></span>
              <span className="px-4 text-sm font-semibold">Any week</span>
              <span className="h-6 w-px bg-[var(--air-line)]"></span>
              <span className="px-4 text-sm text-[var(--air-muted)]">
                Add guests
              </span>
              <span className="ml-1 flex h-8 w-8 items-center justify-center rounded-full bg-[#FF385C] text-white">
                <svg
                  xmlns="http://www.w3.org/2000/svg"
                  width="15"
                  height="15"
                  viewBox="0 0 24 24"
                  fill="none"
                  stroke="currentColor"
                  strokeWidth="2.6"
                  strokeLinecap="round"
                  strokeLinejoin="round"
                  aria-hidden="true"
                >
                  <circle cx="11" cy="11" r="7.25" />
                  <line x1="20" y1="20" x2="16.65" y2="16.65" />
                </svg>
              </span>
            </a>
            <a
              href="/showcase/airbnb/proa/search"
              className="air-search-pill flex md:hidden items-center gap-3 rounded-full border border-[var(--air-line)] bg-white px-4 py-3"
            >
              <span className="flex h-8 w-8 items-center justify-center rounded-full bg-[#FF385C] text-white">
                <svg
                  xmlns="http://www.w3.org/2000/svg"
                  width="15"
                  height="15"
                  viewBox="0 0 24 24"
                  fill="none"
                  stroke="currentColor"
                  strokeWidth="2.6"
                  strokeLinecap="round"
                  strokeLinejoin="round"
                  aria-hidden="true"
                >
                  <circle cx="11" cy="11" r="7.25" />
                  <line x1="20" y1="20" x2="16.65" y2="16.65" />
                </svg>
              </span>
              <span className="text-sm font-semibold">Start your search</span>
            </a>
          </div>

          <div className="flex items-center justify-end gap-2">
            <a
              href="/showcase/airbnb/proa"
              className="hidden md:block rounded-full px-4 py-3 text-sm font-semibold hover:bg-[var(--air-soft)]"
            >
              Airbnb your home
            </a>
            <button
              className="hidden md:flex h-10 w-10 items-center justify-center rounded-full hover:bg-[var(--air-soft)]"
              aria-label="Language"
            >
              <GlobeSvg />
            </button>
            <button className="air-search-pill flex items-center gap-3 rounded-full border border-[var(--air-line)] bg-white pl-3 pr-2.5 py-1.5">
              <svg
                xmlns="http://www.w3.org/2000/svg"
                width="16"
                height="16"
                viewBox="0 0 32 32"
                fill="none"
                stroke="currentColor"
                strokeWidth="3"
                strokeLinecap="round"
                aria-hidden="true"
              >
                <line x1="6" y1="9" x2="26" y2="9" />
                <line x1="6" y1="16" x2="26" y2="16" />
                <line x1="6" y1="23" x2="26" y2="23" />
              </svg>
              <div className="flex h-8 w-8 items-center justify-center rounded-full bg-[#6A6A6A] text-white">
                <svg
                  xmlns="http://www.w3.org/2000/svg"
                  width="15"
                  height="15"
                  viewBox="0 0 24 24"
                  fill="currentColor"
                  aria-hidden="true"
                >
                  <path d="M12 12c2.209 0 4-1.791 4-4S14.209 4 12 4s-4 1.791-4 4 1.791 4 4 4zm0 2c-2.673 0-8 1.327-8 3.963V20h16v-2.037C20 15.327 14.673 14 12 14z" />
                </svg>
              </div>
            </button>
          </div>
        </div>
      </div>
    </header>
  );
}

// ---------------------------------------------------------------------------
// Footer
// ---------------------------------------------------------------------------

export function Footer() {
  return (
    <>
      <div className="border-t border-b border-[#e2c56b] bg-[#fef9eb]">
        <div className="air-shell">
          <p className="py-3 text-xs text-[#7a6520] leading-relaxed text-center">
            <strong>Disclaimer:</strong> This is an unofficial technology demo
            built with{" "}
            <a href="/" className="underline font-medium">
              Proa UI
            </a>{" "}
            to showcase server-rendered UI components. It is{" "}
            <strong>
              not affiliated with, endorsed by, or connected to Airbnb, Inc.
            </strong>{" "}
            in any way. All Airbnb trademarks, logos, and design elements belong
            to their respective owners and are used here solely for illustrative
            purposes.
          </p>
        </div>
      </div>
      <footer className="border-t border-[var(--air-line)] bg-[var(--air-soft)]">
        <div className="air-shell">
          <div className="grid grid-cols-1 gap-8 py-12 text-sm md:grid-cols-3">
            <div>
              <p className="mb-3 font-semibold">Support</p>
              <ul className="space-y-2 text-[var(--air-muted)]">
                <li>
                  <a href="#support" className="hover:underline">
                    Help Center
                  </a>
                </li>
                <li>
                  <a href="#support" className="hover:underline">
                    AirCover
                  </a>
                </li>
                <li>
                  <a href="#support" className="hover:underline">
                    Cancellation options
                  </a>
                </li>
              </ul>
            </div>
            <div>
              <p className="mb-3 font-semibold">Hosting</p>
              <ul className="space-y-2 text-[var(--air-muted)]">
                <li>
                  <a href="#hosting" className="hover:underline">
                    Airbnb your home
                  </a>
                </li>
                <li>
                  <a href="#hosting" className="hover:underline">
                    Hosting resources
                  </a>
                </li>
                <li>
                  <a href="#hosting" className="hover:underline">
                    Community forum
                  </a>
                </li>
              </ul>
            </div>
            <div>
              <p className="mb-3 font-semibold">Airbnb</p>
              <ul className="space-y-2 text-[var(--air-muted)]">
                <li>
                  <a href="#about" className="hover:underline">
                    Newsroom
                  </a>
                </li>
                <li>
                  <a href="#about" className="hover:underline">
                    Careers
                  </a>
                </li>
                <li>
                  <a href="#about" className="hover:underline">
                    Investors
                  </a>
                </li>
              </ul>
            </div>
          </div>
          <div className="flex flex-col gap-4 border-t border-[var(--air-line)] py-6 text-sm text-[var(--air-muted)] md:flex-row md:items-center md:justify-between">
            <p>
              © 2026 Proa Labs, Inc. · This is a demo, not a real Airbnb site · Airbnb®
              is a registered trademark of Airbnb, Inc.
            </p>
            <p>
              Built with{" "}
              <a href="/" className="underline">
                Proa UI
              </a>{" "}
              (showcase demo)
            </p>
          </div>
        </div>
      </footer>
    </>
  );
}

// ---------------------------------------------------------------------------
// AirImage
// ---------------------------------------------------------------------------

export function AirImage({
  id,
  src,
  alt,
  className,
  loading = "lazy",
  fetchPriority = "auto",
  fallbackSrc,
}: {
  id?: string;
  src: string;
  alt: string;
  className?: string;
  loading?: "eager" | "lazy";
  fetchPriority?: "high" | "auto" | "low";
  fallbackSrc?: string;
}) {
  return (
    <img
      data-air-image=""
      {...(id !== undefined ? { id } : {})}
      src={src}
      alt={alt}
      className={className ?? ""}
      loading={loading}
      decoding="async"
      fetchPriority={fetchPriority}
      data-fallback-src={fallbackSrc ?? DEFAULT_FALLBACK_SRC}
    />
  );
}
