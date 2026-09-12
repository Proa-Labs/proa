// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

// Home page component, React equivalent of the Proa Airbnb home showcase.
//
// Produces HTML with the same structure, data attributes, and class strings
// as the Proa server-rendered Airbnb home page. Target: ~159 KB HTML.

import React from "react";
import { AirImage, Footer } from "./shared";
import {
  LISTINGS,
  HOME_IMAGES,
  HOUSE_POSTER,
  BALLOON_POSTER,
  SERVICE_POSTER,
  HOMES_SECTIONS,
  EXPERIENCES_SECTIONS,
  SERVICES_SECTIONS,
  type Listing,
} from "./data";

import {
  AirbnbLogoSvg,
  GlobeSvg,
  HeartSvg,
  StarSvg12,
} from "./svg-elements";

export interface HomePageData {
  listings: Listing[];
  homesSections: [string, string, number[]][];
  experiencesSections: [string, string, number[]][];
  servicesSections: [string, string, number[]][];
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

function railCardCopy(
  mode: string,
  listing: Listing,
): [string, string] {
  switch (mode) {
    case "experiences":
      return [
        "Hosted experience",
        `From $${Math.floor(listing.pricePerNight / 2) + 42} per guest`,
      ];
    case "services":
      return [
        "Professional service",
        `From $${Math.floor(listing.pricePerNight / 2) + 65} per visit`,
      ];
    default:
      return [listing.propertyType, `$${listing.pricePerNight * 2} for 2 nights`];
  }
}

// ---------------------------------------------------------------------------
// Arrow SVGs (inline, used in section headers and rail scroll buttons)
// ---------------------------------------------------------------------------

const ARROW_RIGHT_SECTION_SVG = (
  <svg
    xmlns="http://www.w3.org/2000/svg"
    width="14"
    height="14"
    viewBox="0 0 24 24"
    fill="none"
    stroke="currentColor"
    strokeWidth="2.2"
    strokeLinecap="round"
    strokeLinejoin="round"
  >
    <line x1="5" y1="12" x2="19" y2="12" />
    <line x1="12" y1="5" x2="19" y2="12" />
    <line x1="12" y1="19" x2="19" y2="12" />
  </svg>
);

const CHEVRON_LEFT_SVG = (
  <svg
    xmlns="http://www.w3.org/2000/svg"
    width="14"
    height="14"
    viewBox="0 0 24 24"
    fill="none"
    stroke="currentColor"
    strokeWidth="2.4"
    strokeLinecap="round"
    strokeLinejoin="round"
  >
    <polyline points="15 18 9 12 15 6" />
  </svg>
);

const CHEVRON_RIGHT_SVG = (
  <svg
    xmlns="http://www.w3.org/2000/svg"
    width="14"
    height="14"
    viewBox="0 0 24 24"
    fill="none"
    stroke="currentColor"
    strokeWidth="2.4"
    strokeLinecap="round"
    strokeLinejoin="round"
  >
    <polyline points="9 18 15 12 9 6" />
  </svg>
);

// ---------------------------------------------------------------------------
// ModeTab — a single tab for Homes / Experiences / Services
// ---------------------------------------------------------------------------

function ModeTab({
  mode,
  label,
  posterSrc,
  active,
  badge,
  role,
  ariaSelected,
}: {
  mode: string;
  label: string;
  posterSrc: string;
  active: boolean;
  badge?: string;
  role?: string;
  ariaSelected?: boolean;
}) {
  return (
    <button
      className={`home-mode-tab${active ? " active" : ""}`}
      data-home-mode={mode}
      {...(role ? { role } : {})}
      {...(ariaSelected !== undefined ? { "aria-selected": ariaSelected } : {})}
      type="button"
    >
      <span className={badge ? "home-mode-icon relative" : "home-mode-icon"}>
        <AirImage
          src={posterSrc}
          alt=""
          className="home-mode-media"
          loading="eager"
          fetchPriority="low"
        />
        {badge && <span className="home-mode-badge">{badge}</span>}
      </span>
      <span>{label}</span>
    </button>
  );
}

// ---------------------------------------------------------------------------
// RailCard — a single card in a content rail
// ---------------------------------------------------------------------------

function RailCard({
  mode,
  listingIndex,
  listings,
}: {
  mode: string;
  listingIndex: number;
  listings: Listing[];
}) {
  const listing = listings[listingIndex % listings.length];
  const image = HOME_IMAGES[listingIndex % HOME_IMAGES.length];
  const [label, pricing] = railCardCopy(mode, listing);

  let badge: string;
  let title: string;
  let rating: string;
  let href: string;

  switch (mode) {
    case "experiences":
      badge = "Most loved";
      title = `Design walk inspired by ${listing.location}`;
      rating = "4.95";
      href = "/showcase/airbnb/proa/search";
      break;
    case "services":
      badge = "Top rated";
      title = `Private service for ${listing.title}`;
      rating = "5.0";
      href = "/showcase/airbnb/proa/search";
      break;
    default:
      badge = "Guest favorite";
      title = listing.title;
      rating = listing.rating;
      href = "/showcase/airbnb/proa/listing";
      break;
  }

  return (
    <a href={href} className="home-rail-card group">
      <div className="air-card-image relative aspect-[1.03/1] bg-[var(--air-soft)]">
        <AirImage
          src={image}
          alt={listing.title}
          className="h-full w-full object-cover"
          loading="lazy"
          fetchPriority="low"
        />
        <span className="absolute left-3 top-3 rounded-full bg-white/95 px-3 py-1 text-xs font-semibold text-[#222] shadow-sm">
          {badge}
        </span>
        <button
          className="heart-btn absolute right-3 top-3"
          aria-label="Save to favorites"
        >
          <HeartSvg />
        </button>
      </div>
      <div className="mt-3">
        <p className="truncate text-[17px] font-medium leading-5 text-[#222]">
          {title}
        </p>
        <p className="mt-1 text-[15px] leading-5 text-[var(--air-muted)]">
          {label}
        </p>
        <p className="flex items-center gap-1 text-[15px] leading-5 text-[var(--air-muted)]">
          <span>{pricing}</span>
          <StarSvg12 />
          <span>{rating}</span>
        </p>
      </div>
    </a>
  );
}

// ---------------------------------------------------------------------------
// RailSection — a section with title, subtitle, arrows, and a scrolling rail
// ---------------------------------------------------------------------------

function RailSection({
  mode,
  sectionIndex,
  title,
  subtitle,
  indexes,
  alwaysShowSubtitle,
  listings,
}: {
  mode: string;
  sectionIndex: number;
  title: string;
  subtitle: string;
  indexes: number[];
  alwaysShowSubtitle: boolean;
  listings: Listing[];
}) {
  const railId = `${mode}-rail-${sectionIndex}`;

  return (
    <section className="home-rail-section">
      <div className="mb-4 flex items-center justify-between gap-4">
        <div>
          {mode === "homes" ? (
            <>
              <div className="flex items-center gap-2">
                <h2 className="text-[28px] font-semibold tracking-[-0.03em] text-[#222]">
                  {title}
                </h2>
                <span className="hidden md:inline-flex h-7 w-7 items-center justify-center rounded-full bg-[var(--air-soft)] text-[#222]">
                  {ARROW_RIGHT_SECTION_SVG}
                </span>
              </div>
              {(alwaysShowSubtitle || subtitle) && subtitle !== "" && (
                <p className="mt-1 text-[15px] text-[var(--air-muted)]">
                  {subtitle}
                </p>
              )}
            </>
          ) : (
            <>
              <h2 className="text-[28px] font-semibold tracking-[-0.03em] text-[#222]">
                {title}
              </h2>
              <p className="mt-1 text-[15px] text-[var(--air-muted)]">
                {subtitle}
              </p>
            </>
          )}
        </div>
        <div className="hidden items-center gap-2 md:flex">
          <button
            className="home-rail-arrow"
            data-rail-prev={railId}
            type="button"
            aria-label="Scroll left"
          >
            {CHEVRON_LEFT_SVG}
          </button>
          <button
            className="home-rail-arrow"
            data-rail-next={railId}
            type="button"
            aria-label="Scroll right"
          >
            {CHEVRON_RIGHT_SVG}
          </button>
        </div>
      </div>
      <div className="home-rail" id={railId}>
        {indexes.map((listingIndex, i) => (
          <RailCard
            key={`${railId}-${i}`}
            mode={mode}
            listingIndex={listingIndex}
            listings={listings}
          />
        ))}
      </div>
    </section>
  );
}

// ---------------------------------------------------------------------------
// ModePanel — a panel containing multiple RailSections
// ---------------------------------------------------------------------------

function ModePanel({
  mode,
  active,
  sections,
  alwaysShowSubtitle,
  listings,
}: {
  mode: string;
  active: boolean;
  sections: [string, string, number[]][];
  alwaysShowSubtitle: boolean;
  listings: Listing[];
}) {
  return (
    <div
      className={`home-mode-panel${active ? " active" : ""}`}
      data-home-panel={mode}
    >
      <div className="air-shell py-10 md:py-12">
        {sections.map(([title, subtitle, indexes], sectionIndex) => (
          <RailSection
            key={`${mode}-section-${sectionIndex}`}
            mode={mode}
            sectionIndex={sectionIndex}
            title={title}
            subtitle={subtitle}
            indexes={indexes}
            alwaysShowSubtitle={alwaysShowSubtitle}
            listings={listings}
          />
        ))}
      </div>
    </div>
  );
}

// ---------------------------------------------------------------------------
// HomePage — full page component
// ---------------------------------------------------------------------------

export function HomePage({ data }: { data?: Partial<HomePageData> } = {}) {
  const listings = data?.listings ?? LISTINGS;
  const homesSections = data?.homesSections ?? HOMES_SECTIONS;
  const experiencesSections = data?.experiencesSections ?? EXPERIENCES_SECTIONS;
  const servicesSections = data?.servicesSections ?? SERVICES_SECTIONS;

  return (
    <>
      {/* Custom header (NOT the shared Header) */}
      <header className="air-topbar">
        <div className="air-shell pt-5 pb-6 md:pt-6 md:pb-7">
          {/* Top row: logo, desktop nav, profile */}
          <div className="grid grid-cols-[auto_1fr_auto] items-center gap-4 md:gap-8">
            {/* Logo */}
            <a
              href="/showcase/airbnb/proa"
              className="flex items-center gap-2.5 text-[#FF385C]"
            >
              <AirbnbLogoSvg />
              <span className="text-[30px] font-semibold leading-none tracking-[-0.02em]">
                airbnb
              </span>
            </a>

            {/* Desktop mode tabs */}
            <nav
              className="hidden md:flex items-center justify-center gap-8 lg:gap-10"
              role="tablist"
              aria-label="Search modes"
            >
              <ModeTab
                mode="homes"
                label="Homes"
                posterSrc={HOUSE_POSTER}
                active={true}
                role="tab"
                ariaSelected={true}
              />
              <ModeTab
                mode="experiences"
                label="Experiences"
                posterSrc={BALLOON_POSTER}
                active={false}
                badge="NEW"
                role="tab"
                ariaSelected={false}
              />
              <ModeTab
                mode="services"
                label="Services"
                posterSrc={SERVICE_POSTER}
                active={false}
                badge="NEW"
                role="tab"
                ariaSelected={false}
              />
            </nav>

            {/* Profile buttons */}
            <div className="flex items-center justify-end gap-2 md:gap-3">
              <a
                href="/showcase/airbnb/proa"
                className="hidden rounded-full px-4 py-3 text-sm font-semibold hover:bg-[var(--air-soft)] lg:block"
              >
                Become a host
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
                  >
                    <path d="M12 12c2.209 0 4-1.791 4-4S14.209 4 12 4s-4 1.791-4 4 1.791 4 4 4zm0 2c-2.673 0-8 1.327-8 3.963V20h16v-2.037C20 15.327 14.673 14 12 14z" />
                  </svg>
                </div>
              </button>
            </div>
          </div>

          {/* Mobile mode tabs */}
          <div className="mt-5 md:hidden">
            <div className="flex items-center gap-4 overflow-x-auto pb-1">
              <ModeTab
                mode="homes"
                label="Homes"
                posterSrc={HOUSE_POSTER}
                active={true}
              />
              <ModeTab
                mode="experiences"
                label="Experiences"
                posterSrc={BALLOON_POSTER}
                active={false}
                badge="NEW"
              />
              <ModeTab
                mode="services"
                label="Services"
                posterSrc={SERVICE_POSTER}
                active={false}
                badge="NEW"
              />
            </div>
          </div>

          {/* Search bar */}
          <div className="mt-6 flex justify-center">
            <div className="home-search-shell w-full max-w-[850px]">
              <div className="home-search-bar">
                <button
                  className="home-search-segment active"
                  data-search-field="where"
                  data-default="Search destinations"
                  type="button"
                >
                  <span className="home-search-label">Where</span>
                  <span className="home-search-value">
                    Search destinations
                  </span>
                </button>
                <span className="home-search-divider"></span>
                <button
                  className="home-search-segment"
                  data-search-field="when"
                  data-default="Add dates"
                  type="button"
                >
                  <span className="home-search-label">When</span>
                  <span className="home-search-value">Add dates</span>
                </button>
                <span className="home-search-divider"></span>
                <button
                  className="home-search-segment"
                  data-search-field="who"
                  data-default="Add guests"
                  type="button"
                >
                  <span className="home-search-label">Who</span>
                  <span className="home-search-value">Add guests</span>
                </button>
                <button
                  className="home-search-submit"
                  data-home-search-submit=""
                  type="button"
                  aria-label="Search"
                >
                  <svg
                    viewBox="0 0 32 32"
                    xmlns="http://www.w3.org/2000/svg"
                    style={{
                      display: "block",
                      fill: "none",
                      height: "16px",
                      width: "16px",
                      stroke: "currentColor",
                      strokeWidth: 4,
                      overflow: "visible",
                    }}
                    aria-hidden="true"
                    role="presentation"
                    focusable="false"
                  >
                    <path d="m13 24c6.0751 0 11-4.9249 11-11 0-6.0751-4.9249-11-11-11-6.0751 0-11 4.9249-11 11 0 6.0751 4.9249 11 11 11zm8-3 10 10" />
                  </svg>
                </button>
              </div>

              {/* Search popover */}
              <div className="home-search-popover" id="home-search-popover">
                <div className="mb-3 flex items-center justify-between gap-3">
                  <div>
                    <p
                      className="text-sm font-semibold text-[#222]"
                      data-popover-title=""
                    >
                      Where do you want to go?
                    </p>
                    <p
                      className="text-sm text-[var(--air-muted)]"
                      data-popover-subtitle=""
                    >
                      Choose a preset to update the search bar.
                    </p>
                  </div>
                  <button
                    className="rounded-full px-3 py-1.5 text-sm font-medium hover:bg-[var(--air-soft)]"
                    data-home-search-close=""
                    type="button"
                  >
                    Done
                  </button>
                </div>
                <div
                  className="flex flex-wrap gap-2"
                  data-popover-options=""
                ></div>
              </div>
            </div>
          </div>
        </div>
      </header>

      {/* Main content with mode panels */}
      <main className="bg-white">
        <ModePanel
          mode="homes"
          active={true}
          sections={homesSections}
          alwaysShowSubtitle={false}
          listings={listings}
        />
        <ModePanel
          mode="experiences"
          active={false}
          sections={experiencesSections}
          alwaysShowSubtitle={true}
          listings={listings}
        />
        <ModePanel
          mode="services"
          active={false}
          sections={servicesSections}
          alwaysShowSubtitle={true}
          listings={listings}
        />
      </main>

      <Footer />
    </>
  );
}

export default HomePage;
