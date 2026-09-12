// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

import React from "react";
import { Header, Footer, AirImage } from "./shared";
import { LISTINGS, FILTERS, SEARCH_IMAGES, type Listing } from "./data";
import { STAR_SVG_14, HEART_SVG } from "./svg";

export interface SearchPageData {
  listings: Listing[];
}

export function SearchPage({ data }: { data?: Partial<SearchPageData> } = {}) {
  const listings = data?.listings ?? LISTINGS;
  const stayCount = `${listings.length} stays`;
  return (
    <>
      <Header />

      {/* Title + filters */}
      <div className="air-shell pt-8 pb-5">
        <p className="mb-2 text-sm font-medium text-[var(--air-muted)]">
          Entire homes and unique stays
        </p>
        <h1 className="mb-1 text-[32px] font-semibold tracking-[-0.04em] text-[#222]">
          Malibu {"\u2014"} Vacation Rentals
        </h1>
        <p className="mb-5 text-sm text-[var(--air-muted)]">
          {stayCount} {"\u00b7"} Feb 15{"\u2013"}22 {"\u00b7"} 2 guests
        </p>
        <div className="flex flex-wrap gap-2">
          {FILTERS.map((filter, i) => (
            <button
              key={i}
              className="filter-pill rounded-full border border-[var(--air-line-strong)] bg-white px-4 py-2.5 text-sm text-[#222] transition-colors hover:border-[#222]"
            >
              {filter}
            </button>
          ))}
        </div>
      </div>

      {/* Split layout: listings + map */}
      <div className="air-shell pb-8">
        <div className="grid grid-cols-1 gap-8 xl:grid-cols-[minmax(0,58%)_minmax(360px,42%)]">
          {/* Listings column */}
          <div className="divide-y divide-[var(--air-line)]">
            {listings.map((listing, i) => {
              const price = `$${listing.pricePerNight}`;
              const reviews = `(${listing.reviewsCount} reviews)`;
              const bedsStr = `${listing.beds} bed${listing.beds !== 1 ? "s" : ""}`;
              const bathsStr = `${listing.baths} bath${listing.baths !== 1 ? "s" : ""}`;
              const imgSrc = SEARCH_IMAGES[i % SEARCH_IMAGES.length];
              const badgeText = i % 2 === 0 ? "Guest favorite" : "";
              const badgeClass =
                i % 2 === 0
                  ? "absolute left-3 top-3 rounded-full bg-white/95 px-3 py-1 text-xs font-semibold text-[#222] shadow-sm"
                  : "hidden";
              return (
                <a
                  key={i}
                  href="/showcase/airbnb/proa/listing"
                  className="search-result-card group grid grid-cols-1 gap-5 rounded-[20px] py-6 transition-colors md:grid-cols-[315px_minmax(0,1fr)]"
                  data-map-index={String(i)}
                >
                  <div className="air-card-image relative md:h-[212px]">
                    <AirImage
                      src={imgSrc}
                      alt={listing.title}
                      className="h-full w-full object-cover"
                      loading="lazy"
                      fetchPriority="low"
                    />
                    <span className={badgeClass}>{badgeText}</span>
                    <button className="heart-btn absolute right-3 top-3">
                      <span dangerouslySetInnerHTML={{ __html: HEART_SVG }} />
                    </button>
                  </div>
                  <div className="flex min-w-0 flex-col justify-between">
                    <div>
                      <p className="text-sm text-[var(--air-muted)]">
                        {listing.propertyType} {"\u00b7"} {bedsStr} {"\u00b7"} {bathsStr}
                      </p>
                      <h3 className="mt-2 text-[22px] font-medium leading-7 tracking-[-0.02em] text-[#222]">
                        {listing.title}
                      </h3>
                      <div className="my-4 h-px w-10 bg-[var(--air-line)]"></div>
                      <p className="text-[15px] leading-5 text-[var(--air-muted)]">
                        {listing.location}
                      </p>
                      <p className="mt-1 text-[15px] leading-5 text-[var(--air-muted)]">
                        Free cancellation for 48 hours {"\u00b7"} Ocean-facing deck
                      </p>
                    </div>
                    <div className="mt-6 flex items-end justify-between gap-4">
                      <div className="flex items-center gap-1 text-sm text-[#222]">
                        <span dangerouslySetInnerHTML={{ __html: STAR_SVG_14 }} />
                        <span>{listing.rating}</span>
                        <span className="text-[var(--air-muted)]">{reviews}</span>
                      </div>
                      <p className="text-right text-[15px] leading-5 text-[#222]">
                        <span className="font-semibold">{price}</span> night
                        <span className="block text-[var(--air-muted)]">
                          7 nights total available
                        </span>
                      </p>
                    </div>
                  </div>
                </a>
              );
            })}
          </div>

          {/* Map placeholder */}
          <div className="hidden xl:block">
            <div className="air-map sticky top-24 h-[calc(100vh-8rem)] overflow-hidden rounded-[24px] border border-[var(--air-line)]">
              <div className="absolute left-[18%] top-[18%] h-36 w-52 rounded-full bg-white/45 blur-3xl"></div>
              <div className="absolute inset-0 opacity-40">
                <svg
                  viewBox="0 0 600 900"
                  className="h-full w-full"
                  xmlns="http://www.w3.org/2000/svg"
                  aria-hidden="true"
                >
                  <path
                    d="M32 160C170 70 300 112 372 184c84 84 172 108 196 186 30 98-34 184-110 232-122 76-230 34-316 92-80 54-118 138-118 138"
                    fill="none"
                    stroke="#d7d2c7"
                    strokeWidth="20"
                    strokeLinecap="round"
                  />
                  <path
                    d="M110 94c56 46 116 66 166 62 74-6 98 18 158 74"
                    fill="none"
                    stroke="#cfd7cb"
                    strokeWidth="16"
                    strokeLinecap="round"
                  />
                  <path
                    d="M420 530c18 48 18 106-4 162"
                    fill="none"
                    stroke="#d7d2c7"
                    strokeWidth="14"
                    strokeLinecap="round"
                  />
                </svg>
              </div>
              <button
                className="air-price-pin left-[20%] top-[24%]"
                data-map-pin="0"
                type="button"
              >
                $350
              </button>
              <button
                className="air-price-pin left-[58%] top-[19%]"
                data-map-pin="5"
                type="button"
              >
                $295
              </button>
              <button
                className="air-price-pin left-[50%] top-[41%]"
                data-map-pin="1"
                type="button"
              >
                $185
              </button>
              <button
                className="air-price-pin left-[26%] top-[56%]"
                data-map-pin="2"
                type="button"
              >
                $520
              </button>
              <button
                className="air-price-pin left-[64%] top-[63%]"
                data-map-pin="4"
                type="button"
              >
                $275
              </button>
              <div className="absolute bottom-5 left-5 rounded-full bg-[#222] px-5 py-3 text-sm font-semibold text-white shadow-lg">
                Show map
              </div>
            </div>
          </div>
        </div>
      </div>

      <Footer />
    </>
  );
}
