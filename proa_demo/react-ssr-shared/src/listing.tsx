// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

import React from "react";
import { Header, Footer, AirImage } from "./shared";
import { MALIBU_LISTING, ListingData } from "./data";
import {
  STAR_SVG_14,
  HEART_SVG,
  HOUSE_RULES_ICON,
  SAFETY_ICON,
  CANCELLATION_ICON,
  CLEANLINESS_ICON,
  ACCURACY_ICON,
  CHECKIN_ICON,
  COMMUNICATION_ICON,
  LOCATION_ICON,
  VALUE_ICON,
} from "./svg";
import { Calendar } from "./calendar";

export function ListingPage({ data }: { data?: Partial<ListingData> } = {}) {
  const d: ListingData = data ? { ...MALIBU_LISTING, ...data } : MALIBU_LISTING;

  const nightlyTotal = d.nightlyRate * d.booking.nights;
  const total = nightlyTotal + d.cleaningFee + d.serviceFee;

  const reviewDetails: [string, string, string][] = [
    ["Cleanliness", d.reviewScores.cleanliness, CLEANLINESS_ICON],
    ["Accuracy", d.reviewScores.accuracy, ACCURACY_ICON],
    ["Check-in", d.reviewScores.checkIn, CHECKIN_ICON],
    ["Communication", d.reviewScores.communication, COMMUNICATION_ICON],
    ["Location", d.reviewScores.location, LOCATION_ICON],
    ["Value", d.reviewScores.value, VALUE_ICON],
  ];

  const descriptionParagraphs = d.description.split("\n\n");

  return (
    <>
      <Header />

      <main id="site-content" data-airbnb-listing-page="">
        {/* ── Photos section ────────────────────────────────── */}
        <div className="air-shell pt-8">
          <section id="photos" className="pb-8">
            <div className="mb-4 flex flex-col gap-3 md:flex-row md:items-end md:justify-between">
              <div>
                <h1 className="text-[30px] font-semibold leading-[1.15] tracking-[-0.045em] text-[#222] md:text-[34px]">
                  {d.title}
                </h1>
                <div className="mt-3 flex flex-wrap items-center gap-x-2 gap-y-1 text-sm text-[#222]">
                  <span dangerouslySetInnerHTML={{ __html: STAR_SVG_14 }} />
                  <span className="font-semibold">{d.rating}</span>
                  <span>{"\u00b7"}</span>
                  <a href="#reviews" className="underline font-semibold">
                    {d.reviewCount} reviews
                  </a>
                  <span>{"\u00b7"}</span>
                  <span className="font-semibold">Guest favorite</span>
                  <span>{"\u00b7"}</span>
                  <span className="underline">{d.location}</span>
                </div>
              </div>
              <div className="flex items-center gap-2 self-start text-sm font-medium md:self-auto">
                <button
                  data-action="share"
                  className="flex items-center gap-2 rounded-lg px-3 py-2 underline-offset-2 hover:bg-[var(--air-soft)] hover:underline"
                >
                  <svg
                    xmlns="http://www.w3.org/2000/svg"
                    width="16"
                    height="16"
                    viewBox="0 0 24 24"
                    fill="none"
                    stroke="currentColor"
                    strokeWidth="2"
                    strokeLinecap="round"
                    strokeLinejoin="round"
                  >
                    <path d="M4 12v8a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2v-8" />
                    <polyline points="16 6 12 2 8 6" />
                    <line x1="12" y1="2" x2="12" y2="15" />
                  </svg>
                  Share
                </button>
                <button className="heart-btn flex items-center gap-2 rounded-lg px-3 py-2 underline-offset-2 hover:bg-[var(--air-soft)] hover:underline">
                  <svg
                    xmlns="http://www.w3.org/2000/svg"
                    width="16"
                    height="16"
                    viewBox="0 0 24 24"
                    fill="none"
                    stroke="currentColor"
                    strokeWidth="2"
                    strokeLinecap="round"
                    strokeLinejoin="round"
                  >
                    <path d="M20.84 4.61a5.5 5.5 0 0 0-7.78 0L12 5.67l-1.06-1.06a5.5 5.5 0 0 0-7.78 7.78l1.06 1.06L12 21.23l7.78-7.78 1.06-1.06a5.5 5.5 0 0 0 0-7.78z" />
                  </svg>
                  Save
                </button>
              </div>
            </div>

            {/* Photo grid */}
            <div className="relative grid h-[31rem] grid-cols-4 grid-rows-2 gap-2 overflow-hidden rounded-[22px]">
              <button
                className="col-span-2 row-span-2 cursor-pointer overflow-hidden transition-opacity hover:opacity-95"
                data-gallery-open=""
                data-gallery-index="0"
                data-gallery-src={d.photos[0].src}
                data-gallery-alt={d.photos[0].alt}
                type="button"
              >
                <AirImage
                  src={d.photos[0].src}
                  alt={d.photos[0].alt}
                  className="h-full w-full object-cover"
                  loading="eager"
                  fetchPriority="high"
                />
              </button>
              <button
                className="cursor-pointer overflow-hidden transition-opacity hover:opacity-95"
                data-gallery-open=""
                data-gallery-index="1"
                data-gallery-src={d.photos[1].src}
                data-gallery-alt={d.photos[1].alt}
                type="button"
              >
                <AirImage
                  src={d.photos[1].src}
                  alt={d.photos[1].alt}
                  className="h-full w-full object-cover"
                  loading="lazy"
                  fetchPriority="low"
                />
              </button>
              <button
                className="cursor-pointer overflow-hidden transition-opacity hover:opacity-95"
                data-gallery-open=""
                data-gallery-index="2"
                data-gallery-src={d.photos[2].src}
                data-gallery-alt={d.photos[2].alt}
                type="button"
              >
                <AirImage
                  src={d.photos[2].src}
                  alt={d.photos[2].alt}
                  className="h-full w-full object-cover"
                  loading="lazy"
                  fetchPriority="low"
                />
              </button>
              <button
                className="cursor-pointer overflow-hidden transition-opacity hover:opacity-95"
                data-gallery-open=""
                data-gallery-index="3"
                data-gallery-src={d.photos[3].src}
                data-gallery-alt={d.photos[3].alt}
                type="button"
              >
                <AirImage
                  src={d.photos[3].src}
                  alt={d.photos[3].alt}
                  className="h-full w-full object-cover"
                  loading="lazy"
                  fetchPriority="low"
                />
              </button>
              <button
                className="cursor-pointer overflow-hidden transition-opacity hover:opacity-95"
                data-gallery-open=""
                data-gallery-index="4"
                data-gallery-src={d.photos[4].src}
                data-gallery-alt={d.photos[4].alt}
                type="button"
              >
                <AirImage
                  src={d.photos[4].src}
                  alt={d.photos[4].alt}
                  className="h-full w-full object-cover"
                  loading="lazy"
                  fetchPriority="low"
                />
              </button>
              <button
                className="absolute bottom-4 right-4 rounded-xl border border-[#222] bg-white px-4 py-2 text-sm font-medium text-[#222] shadow-sm hover:bg-[var(--air-soft)]"
                data-gallery-open=""
                data-gallery-index="0"
                type="button"
              >
                Show all photos
              </button>
            </div>

            {/* Hidden extra photo spans */}
            {d.photos.slice(5).map((photo, i) => (
              <span
                key={i}
                className="hidden"
                data-gallery-src={photo.src}
                data-gallery-alt={photo.alt}
                data-gallery-index={String(i + 5)}
              ></span>
            ))}
          </section>
        </div>

        {/* ── Sticky nav bar (desktop) ──────────────────────── */}
        <div className="sticky top-[80px] z-30 hidden border-y border-[var(--air-line)] bg-white/95 backdrop-blur lg:block">
          <div className="air-shell flex items-center justify-between gap-8 py-4">
            <nav className="flex items-center gap-6 text-sm font-semibold text-[#222]">
              <a href="#photos" className="air-link">Photos</a>
              <a href="#amenities" className="air-link">Amenities</a>
              <a href="#reviews" className="air-link">Reviews</a>
              <a href="#location" className="air-link">Location</a>
            </nav>
            <div className="flex items-center gap-5">
              <div className="text-right">
                <div className="text-sm font-semibold text-[#222]">
                  <span>${d.nightlyRate}</span>
                  <span className="font-normal text-[var(--air-muted)]"> night</span>
                </div>
                <div className="flex items-center justify-end gap-1 text-xs text-[#222]">
                  <span dangerouslySetInnerHTML={{ __html: STAR_SVG_14 }} />
                  <span>{d.rating} {"\u00b7"} {d.reviewCount} reviews</span>
                </div>
              </div>
              <button className="rounded-xl bg-gradient-to-r from-[#E61E4D] via-[#E31C5F] to-[#D70466] px-5 py-3 text-sm font-semibold text-white">
                Reserve
              </button>
            </div>
          </div>
        </div>

        {/* ── Two-column layout ─────────────────────────────── */}
        <div className="air-shell py-10">
          <div className="grid grid-cols-1 gap-y-12 lg:grid-cols-12 lg:gap-x-16">
            {/* ── Left column (content) ─────────────────────── */}
            <div className="lg:col-span-7">
              {/* Property overview */}
              <section className="pb-8">
                <div className="flex items-start justify-between gap-4">
                  <div>
                    <h2 className="text-[26px] font-semibold tracking-[-0.03em] text-[#222]">
                      {d.propertyLine}
                    </h2>
                    <p className="mt-2 text-[16px] text-[#222]">{d.overviewSummary}</p>
                  </div>
                  <AirImage
                    src={d.host.photo}
                    alt="Host portrait"
                    className="h-14 w-14 rounded-full object-cover"
                    loading="lazy"
                    fetchPriority="low"
                  />
                </div>
              </section>

              <hr className="air-divider" />

              {/* Guest favorite strip */}
              <section className="py-8">
                <div className="airbnb-favorite-strip">
                  <div className="airbnb-favorite-badge">
                    <div className="text-[#222]">
                      <span dangerouslySetInnerHTML={{ __html: STAR_SVG_14 }} />
                    </div>
                    <div>
                      <div className="text-base font-semibold text-[#222]">Guest favorite</div>
                      <div className="text-sm text-[var(--air-muted)]">Highly rated for reliability</div>
                    </div>
                  </div>
                  <div className="airbnb-favorite-copy">
                    One of the most loved homes on Airbnb, according to guests
                  </div>
                  <div className="airbnb-favorite-score">
                    <div className="text-[30px] font-semibold tracking-[-0.04em] text-[#222]">{d.rating}</div>
                    <div className="text-sm font-medium text-[#222]">{d.reviewCount} reviews</div>
                  </div>
                </div>
              </section>

              <hr className="air-divider" />

              {/* Host bio section */}
              <section className="py-8">
                <div className="flex items-start gap-5">
                  <AirImage
                    src={d.host.photo}
                    alt="Host portrait"
                    className="h-16 w-16 rounded-full object-cover"
                    loading="lazy"
                    fetchPriority="low"
                  />
                  <div>
                    <h3 className="text-[22px] font-semibold tracking-[-0.03em] text-[#222]">
                      Hosted by {d.host.name}
                    </h3>
                    <p className="mt-1 text-sm text-[var(--air-muted)]">
                      Superhost {"\u00b7"} {d.host.yearsHosting} years hosting
                    </p>
                    <div className="mt-4 grid grid-cols-2 gap-4 sm:grid-cols-4">
                      <div>
                        <div className="text-lg font-semibold text-[#222]">{d.host.reviewCount}</div>
                        <div className="text-sm text-[var(--air-muted)]">Reviews</div>
                      </div>
                      <div>
                        <div className="text-lg font-semibold text-[#222]">{d.rating}</div>
                        <div className="text-sm text-[var(--air-muted)]">Rating</div>
                      </div>
                      <div>
                        <div className="text-lg font-semibold text-[#222]">{d.host.responseRate}</div>
                        <div className="text-sm text-[var(--air-muted)]">Response rate</div>
                      </div>
                      <div>
                        <div className="text-lg font-semibold text-[#222]">{d.host.responseTime}</div>
                        <div className="text-sm text-[var(--air-muted)]">Response time</div>
                      </div>
                    </div>
                  </div>
                </div>
              </section>

              <hr className="air-divider" />

              {/* Highlights */}
              <section className="space-y-6 py-8">
                {d.highlights.map((highlight, i) => (
                  <div key={i} className="flex gap-4">
                    <div className="flex h-10 w-10 shrink-0 items-center justify-center rounded-xl bg-[var(--air-soft)] text-[#222]">
                      <svg
                        xmlns="http://www.w3.org/2000/svg"
                        width="20"
                        height="20"
                        viewBox="0 0 24 24"
                        fill="none"
                        stroke="currentColor"
                        strokeWidth="1.9"
                        strokeLinecap="round"
                        strokeLinejoin="round"
                      >
                        <path d="M5 12h14" />
                        <path d="M12 5v14" />
                      </svg>
                    </div>
                    <div>
                      <p className="font-medium text-[#222]">{highlight.title}</p>
                      <p className="mt-1 text-[15px] leading-6 text-[var(--air-muted)]">
                        {highlight.description}
                      </p>
                    </div>
                  </div>
                ))}
              </section>

              <hr className="air-divider" />

              {/* About this place */}
              <section className="py-8">
                <h2 className="mb-5 text-[24px] font-semibold tracking-[-0.03em] text-[#222]">
                  About this place
                </h2>
                <div className="space-y-4 text-[16px] leading-7 text-[#222]">
                  {descriptionParagraphs.map((paragraph, i) => (
                    <p key={i}>{paragraph}</p>
                  ))}
                </div>
                <button className="mt-5 inline-flex items-center gap-2 rounded-lg px-1 py-1 text-sm font-semibold underline underline-offset-4">
                  Show more
                </button>
              </section>

              <hr className="air-divider" />

              {/* Amenities */}
              <section id="amenities" className="py-8">
                <div className="mb-6 flex items-end justify-between gap-4">
                  <h2 className="text-[24px] font-semibold tracking-[-0.03em] text-[#222]">
                    What this place offers
                  </h2>
                  <span className="text-sm text-[var(--air-muted)]">
                    Preview of {d.totalAmenityCount} amenities
                  </span>
                </div>
                <div className="grid grid-cols-1 gap-x-10 gap-y-5 sm:grid-cols-2">
                  {d.amenities.map((amenity, i) => (
                    <div
                      key={i}
                      className="flex items-center gap-4 border-b border-[var(--air-line)] pb-4 text-[#222]"
                    >
                      <div className="flex h-10 w-10 shrink-0 items-center justify-center rounded-xl bg-[var(--air-soft)]">
                        <svg
                          xmlns="http://www.w3.org/2000/svg"
                          width="20"
                          height="20"
                          viewBox="0 0 24 24"
                          fill="none"
                          stroke="currentColor"
                          strokeWidth="1.8"
                          strokeLinecap="round"
                          strokeLinejoin="round"
                        >
                          <path d="M4 12h16" />
                          <path d="M12 4v16" />
                        </svg>
                      </div>
                      <span className="text-[15px]">{amenity}</span>
                    </div>
                  ))}
                </div>
                <button className="mt-8 rounded-xl border border-[#222] px-6 py-3 text-sm font-semibold text-[#222] hover:bg-[var(--air-soft)]">
                  Show all {d.totalAmenityCount} amenities
                </button>
              </section>

              <hr className="air-divider" />

              {/* Select dates */}
              <section className="py-8">
                <div className="mb-6 flex items-end justify-between gap-4">
                  <div>
                    <h2 className="text-[24px] font-semibold tracking-[-0.03em] text-[#222]">
                      Select dates
                    </h2>
                    <p className="mt-2 text-[15px] text-[var(--air-muted)]">
                      Add your travel dates for exact pricing
                    </p>
                  </div>
                  <button
                    data-booking-clear-all=""
                    type="button"
                    className="text-sm font-semibold underline underline-offset-4"
                  >
                    Clear dates
                  </button>
                </div>
                <div className="grid gap-5 md:grid-cols-2">
                  <div data-airbnb-synced-calendar="" className="rounded-[22px] border border-[var(--air-line)] p-5">
                    <Calendar
                      month={3}
                      year={2026}
                      mode="range"
                      weekStart="sunday"
                      rangeStart={18}
                      rangeEnd={25}
                      className="air-calendar"
                    />
                  </div>
                  <div data-airbnb-synced-calendar="" className="rounded-[22px] border border-[var(--air-line)] p-5">
                    <Calendar
                      month={4}
                      year={2026}
                      mode="range"
                      weekStart="sunday"
                      className="air-calendar"
                    />
                  </div>
                </div>
              </section>

              <hr className="air-divider" />

              {/* Where you'll be */}
              <section id="location" className="py-8">
                <h2 className="text-[24px] font-semibold tracking-[-0.03em] text-[#222]">
                  Where you'll be
                </h2>
                <p className="mt-2 text-[15px] text-[var(--air-muted)]">{d.location}</p>
                <div className="air-map relative mt-6 h-[360px] overflow-hidden rounded-[26px] border border-[var(--air-line)]">
                  <div className="absolute inset-0 bg-[linear-gradient(135deg,rgba(255,255,255,0.24),transparent_42%)]"></div>
                  <div className="absolute left-[18%] top-[24%] h-28 w-28 rounded-full border border-white/60 bg-white/35 blur-[2px]"></div>
                  <div className="absolute right-[14%] top-[18%] h-20 w-20 rounded-full border border-white/60 bg-white/30 blur-[2px]"></div>
                  <div className="absolute bottom-[18%] left-[52%] h-24 w-24 rounded-full border border-white/60 bg-white/35 blur-[2px]"></div>
                  <div className="air-price-pin left-[47%] top-[44%] bg-[#222] text-white shadow-lg">
                    {d.locationShort}
                  </div>
                </div>
                <p className="mt-5 max-w-2xl text-[15px] leading-6 text-[var(--air-muted)]">
                  This listing's location is verified and the exact location will be provided after
                  booking. Expect quick access to the coast with a quieter canyon setting than many
                  central Malibu homes.
                </p>
              </section>

              <hr className="air-divider" />

              {/* Meet your host */}
              <section className="py-8">
                <div className="grid gap-8 lg:grid-cols-[320px_1fr]">
                  <div className="rounded-[26px] bg-[var(--air-soft)] p-6">
                    <div className="flex items-center gap-4">
                      <AirImage
                        src={d.host.photo}
                        alt="Host portrait"
                        className="h-20 w-20 rounded-full object-cover"
                        loading="lazy"
                        fetchPriority="low"
                      />
                      <div>
                        <h2 className="text-[24px] font-semibold tracking-[-0.03em] text-[#222]">
                          Meet your host
                        </h2>
                        <p className="mt-1 text-sm text-[var(--air-muted)]">
                          {d.host.name} {"\u00b7"} Superhost
                        </p>
                      </div>
                    </div>
                    <div className="mt-6 grid grid-cols-3 gap-4 border-t border-[var(--air-line)] pt-5 text-center">
                      <div>
                        <div className="text-xl font-semibold text-[#222]">{d.host.reviewCount}</div>
                        <div className="text-xs text-[var(--air-muted)]">Reviews</div>
                      </div>
                      <div>
                        <div className="text-xl font-semibold text-[#222]">{d.rating}</div>
                        <div className="text-xs text-[var(--air-muted)]">Rating</div>
                      </div>
                      <div>
                        <div className="text-xl font-semibold text-[#222]">{d.host.yearsHosting}</div>
                        <div className="text-xs text-[var(--air-muted)]">Years hosting</div>
                      </div>
                    </div>
                  </div>
                  <div>
                    <h3 className="text-[22px] font-semibold tracking-[-0.03em] text-[#222]">
                      {d.host.name} is a Superhost
                    </h3>
                    <p className="mt-3 max-w-2xl text-[15px] leading-6 text-[var(--air-muted)]">
                      {d.host.bio} {d.host.name} lives in {d.location}
                      {d.cohost
                        ? ` and cohosts with ${d.cohost.name} for smoother arrivals and faster support.`
                        : "."}
                    </p>
                    <div className="mt-6 flex flex-wrap items-center gap-6">
                      {d.cohost && (
                        <div className="flex items-center gap-3">
                          <AirImage
                            src={d.cohost.photo}
                            alt="Cohost portrait"
                            className="h-12 w-12 rounded-full object-cover"
                            loading="lazy"
                            fetchPriority="low"
                          />
                          <div>
                            <div className="text-sm font-semibold text-[#222]">{d.cohost.name}</div>
                            <div className="text-sm text-[var(--air-muted)]">Co-host</div>
                          </div>
                        </div>
                      )}
                      <div className="text-sm text-[var(--air-muted)]">
                        Response rate: {d.host.responseRate} {"\u00b7"} Responds within{" "}
                        {d.host.responseTime}
                      </div>
                    </div>
                    <div className="mt-6 rounded-[20px] border border-[var(--air-line)] p-5 text-[15px] leading-6 text-[var(--air-muted)]">
                      To help protect your payment, always use Airbnb to send money and communicate
                      with hosts.
                    </div>
                    <button className="mt-6 rounded-xl border border-[#222] px-6 py-3 text-sm font-semibold text-[#222] hover:bg-[var(--air-soft)]">
                      Message host
                    </button>
                  </div>
                </div>
              </section>

              <hr className="air-divider" />

              {/* Things to know */}
              <section className="py-8">
                <h2 className="mb-6 text-[24px] font-semibold tracking-[-0.03em] text-[#222]">
                  Things to know
                </h2>
                <div className="grid gap-6 md:grid-cols-3">
                  <div className="rounded-[22px] border border-[var(--air-line)] p-6">
                    <div className="mb-4 text-[#222]">
                      <span dangerouslySetInnerHTML={{ __html: HOUSE_RULES_ICON }} />
                    </div>
                    <h3 className="text-lg font-semibold text-[#222]">House rules</h3>
                    <div className="mt-4 space-y-3 text-sm text-[var(--air-muted)]">
                      {d.houseRules.map((item, i) => (
                        <p key={i}>{item}</p>
                      ))}
                    </div>
                  </div>
                  <div className="rounded-[22px] border border-[var(--air-line)] p-6">
                    <div className="mb-4 text-[#222]">
                      <span dangerouslySetInnerHTML={{ __html: SAFETY_ICON }} />
                    </div>
                    <h3 className="text-lg font-semibold text-[#222]">Safety & property</h3>
                    <div className="mt-4 space-y-3 text-sm text-[var(--air-muted)]">
                      {d.safetyItems.map((item, i) => (
                        <p key={i}>{item}</p>
                      ))}
                    </div>
                  </div>
                  <div className="rounded-[22px] border border-[var(--air-line)] p-6">
                    <div className="mb-4 text-[#222]">
                      <span dangerouslySetInnerHTML={{ __html: CANCELLATION_ICON }} />
                    </div>
                    <h3 className="text-lg font-semibold text-[#222]">Cancellation policy</h3>
                    <div className="mt-4 space-y-3 text-sm text-[var(--air-muted)]">
                      {d.cancellationItems.map((item, i) => (
                        <p key={i}>{item}</p>
                      ))}
                    </div>
                  </div>
                </div>
              </section>
            </div>

            {/* ── Right column (booking sidebar) ────────────── */}
            <aside className="lg:col-span-4 lg:col-start-9">
              <div
                className="sticky top-28"
                data-airbnb-booking-root=""
                data-initial-checkin={d.booking.checkIn}
                data-initial-checkout={d.booking.checkout}
                data-nightly-rate={String(d.nightlyRate)}
                data-cleaning-fee={String(d.cleaningFee)}
                data-service-fee={String(d.serviceFee)}
              >
                <div className="relative">
                  <div className="mb-4 flex justify-end">
                    <div className="airbnb-booking-pill">
                      <span dangerouslySetInnerHTML={{ __html: STAR_SVG_14 }} />
                      <span>Prices include all fees</span>
                    </div>
                  </div>

                  <div className="air-booking-card airbnb-booking-card p-6">
                    <div className="mb-4">
                      <div className="flex flex-wrap items-baseline gap-x-2 gap-y-1 text-[#222]">
                        <span
                          data-booking-header-total=""
                          className="text-[32px] font-semibold tracking-[-0.04em] text-[#222]"
                        >
                          ${total}
                        </span>
                        <span
                          data-booking-header-context=""
                          className="text-[15px] text-[var(--air-muted)]"
                        >
                          for {d.booking.nights} nights
                        </span>
                      </div>
                      <div className="mt-2 flex items-center gap-1 text-sm text-[#222]">
                        <span dangerouslySetInnerHTML={{ __html: STAR_SVG_14 }} />
                        <span className="font-medium">{d.rating}</span>
                        <span>{"\u00b7"}</span>
                        <a href="#reviews" className="underline">
                          {d.reviewCount} reviews
                        </a>
                      </div>
                    </div>

                    <div className="mb-3 overflow-hidden rounded-2xl border border-[#B0B0B0]">
                      <div className="grid grid-cols-2">
                        <button
                          data-booking-trigger="checkIn"
                          type="button"
                          className="airbnb-booking-trigger border-b border-r border-[#B0B0B0] px-3 py-2.5 text-left transition-colors hover:bg-[var(--air-soft)]"
                        >
                          <span className="block text-[10px] font-bold tracking-wide uppercase text-[#222]">
                            CHECK-IN
                          </span>
                          <span
                            data-booking-display="checkIn"
                            className="airbnb-booking-display mt-0.5 block text-sm text-[#222]"
                          >
                            {d.booking.checkInDisplay}
                          </span>
                        </button>
                        <button
                          data-booking-trigger="checkOut"
                          type="button"
                          className="airbnb-booking-trigger border-b border-[#B0B0B0] px-3 py-2.5 text-left transition-colors hover:bg-[var(--air-soft)]"
                        >
                          <span className="block text-[10px] font-bold tracking-wide uppercase text-[#222]">
                            CHECKOUT
                          </span>
                          <span
                            data-booking-display="checkOut"
                            className="airbnb-booking-display mt-0.5 block text-sm text-[#222]"
                          >
                            {d.booking.checkoutDisplay}
                          </span>
                        </button>
                      </div>
                      <div className="px-3 py-2.5">
                        <label className="block text-[10px] font-bold tracking-wide uppercase text-[#222]">
                          GUESTS
                        </label>
                        <p className="mt-0.5 text-sm text-[#222]">{d.booking.guests}</p>
                      </div>
                    </div>

                    <div className="mb-4 rounded-xl bg-[var(--air-soft)] px-4 py-2.5 text-center text-xs font-medium text-[#222]">
                      Free cancellation before {d.booking.freeCancelDate}
                    </div>

                    <button className="mb-4 w-full rounded-xl bg-gradient-to-r from-[#E61E4D] via-[#E31C5F] to-[#D70466] py-3.5 font-semibold text-white transition-transform hover:scale-[1.01]">
                      Reserve
                    </button>

                    <p className="mb-5 text-center text-sm text-[var(--air-muted)]">
                      You won't be charged yet
                    </p>

                    <div className="space-y-4 text-sm">
                      <div className="flex justify-between">
                        <span data-booking-night-line-label="" className="text-[#222] underline">
                          ${d.nightlyRate} x {d.booking.nights} nights
                        </span>
                        <span data-booking-night-line-value="" className="text-[#222]">
                          ${nightlyTotal}
                        </span>
                      </div>
                      <div className="flex justify-between">
                        <span className="text-[#222] underline">Cleaning fee</span>
                        <span className="text-[#222]">${d.cleaningFee}</span>
                      </div>
                      <div className="flex justify-between">
                        <span className="text-[#222] underline">Airbnb service fee</span>
                        <span className="text-[#222]">${d.serviceFee}</span>
                      </div>
                      <hr className="air-divider" />
                      <div className="flex justify-between font-semibold">
                        <span className="text-[#222]">Total before taxes</span>
                        <span data-booking-total-value="" className="text-[#222]">
                          ${total}
                        </span>
                      </div>
                    </div>
                  </div>

                  {/* Booking popover */}
                  <div
                    data-booking-popover=""
                    aria-hidden="true"
                    className="airbnb-booking-popover hidden"
                  >
                    <div className="airbnb-popover-shell">
                      <div className="airbnb-popover-header">
                        <div className="airbnb-popover-summary">
                          <h3 data-booking-nights="">{d.booking.nights} nights</h3>
                          <p data-booking-range-label="">{d.booking.rangeLabel}</p>
                        </div>
                        <div className="airbnb-popover-fields">
                          <div className="airbnb-popover-field">
                            <button
                              data-booking-trigger="checkIn"
                              type="button"
                              className="block w-full text-left"
                            >
                              <span className="airbnb-popover-field-label">Check-in</span>
                              <span
                                data-booking-popover-display="checkIn"
                                className="airbnb-popover-field-value airbnb-popover-value"
                              >
                                {d.booking.checkInDisplay}
                              </span>
                            </button>
                            <button
                              data-booking-clear-field="checkIn"
                              type="button"
                              className="airbnb-popover-clear"
                              aria-label="Clear dates"
                            >
                              <svg
                                xmlns="http://www.w3.org/2000/svg"
                                viewBox="0 0 32 32"
                                width="12"
                                height="12"
                                aria-hidden="true"
                              >
                                <path
                                  d="m6 6 20 20M26 6 6 26"
                                  fill="none"
                                  stroke="currentColor"
                                  strokeWidth="4"
                                />
                              </svg>
                            </button>
                          </div>
                          <div className="airbnb-popover-field">
                            <button
                              data-booking-trigger="checkOut"
                              type="button"
                              className="block w-full text-left"
                            >
                              <span className="airbnb-popover-field-label">Checkout</span>
                              <span
                                data-booking-popover-display="checkOut"
                                className="airbnb-popover-field-value airbnb-popover-value"
                              >
                                {d.booking.checkoutDisplay}
                              </span>
                            </button>
                            <button
                              data-booking-clear-field="checkOut"
                              type="button"
                              className="airbnb-popover-clear"
                              aria-label="Clear checkout date"
                            >
                              <svg
                                xmlns="http://www.w3.org/2000/svg"
                                viewBox="0 0 32 32"
                                width="12"
                                height="12"
                                aria-hidden="true"
                              >
                                <path
                                  d="m6 6 20 20M26 6 6 26"
                                  fill="none"
                                  stroke="currentColor"
                                  strokeWidth="4"
                                />
                              </svg>
                            </button>
                          </div>
                        </div>
                      </div>

                      <div className="airbnb-popover-calendars">
                        <div data-airbnb-synced-calendar="" className="airbnb-popover-calendar">
                          <Calendar
                            month={3}
                            year={2026}
                            mode="range"
                            weekStart="sunday"
                            rangeStart={18}
                            rangeEnd={25}
                            className="air-calendar"
                          />
                        </div>
                        <div data-airbnb-synced-calendar="" className="airbnb-popover-calendar">
                          <Calendar
                            month={4}
                            year={2026}
                            mode="range"
                            weekStart="sunday"
                            className="air-calendar"
                          />
                        </div>
                      </div>

                      <div className="airbnb-popover-footer">
                        <button
                          data-booking-clear-all=""
                          type="button"
                          className="airbnb-link-button"
                        >
                          Clear dates
                        </button>
                        <button
                          data-booking-close=""
                          type="button"
                          className="airbnb-close-button"
                        >
                          Close
                        </button>
                      </div>
                    </div>
                  </div>
                </div>

                {/* Report listing */}
                <button className="mt-6 inline-flex items-center gap-2 text-sm font-semibold text-[#222] underline underline-offset-4">
                  <svg
                    xmlns="http://www.w3.org/2000/svg"
                    width="16"
                    height="16"
                    viewBox="0 0 24 24"
                    fill="none"
                    stroke="currentColor"
                    strokeWidth="1.9"
                    strokeLinecap="round"
                    strokeLinejoin="round"
                  >
                    <path d="M12 9v4" />
                    <path d="M12 17h.01" />
                    <path d="M10.29 3.86 1.82 18a2 2 0 0 0 1.71 3h16.94a2 2 0 0 0 1.71-3L13.71 3.86a2 2 0 0 0-3.42 0z" />
                  </svg>
                  Report this listing
                </button>
              </div>
            </aside>
          </div>
        </div>

        {/* ── Full-width divider ────────────────────────────── */}
        <hr
          className="air-divider mx-auto"
          style={{ maxWidth: "var(--air-shell-max)" }}
        />

        {/* ── Reviews section ───────────────────────────────── */}
        <section id="reviews" className="air-shell py-10">
          {/* Guest favorite badge */}
          <div className="airbnb-guest-fav-badge">
            <div className="airbnb-guest-fav-laurels">
              <AirImage
                src="https://a0.muscache.com/im/pictures/airbnb-platform-assets/AirbnbPlatformAssets-GuestFavorite/original/78b7687c-5acf-4ef8-a5ea-eda732ae3b2f.png?im_w=720"
                alt="Laurel left"
                className="airbnb-guest-fav-laurel"
                loading="lazy"
                fetchPriority="low"
              />
              <div className="airbnb-guest-fav-score">
                <span className="sr-only">
                  Rated {d.rating} out of 5 from {d.reviewCount} reviews.
                </span>
                <div aria-hidden="true">{d.rating}</div>
              </div>
              <AirImage
                src="https://a0.muscache.com/im/pictures/airbnb-platform-assets/AirbnbPlatformAssets-GuestFavorite/original/b4005b30-79ff-4287-860c-67829ecd7412.png?im_w=720"
                alt="Laurel right"
                className="airbnb-guest-fav-laurel"
                loading="lazy"
                fetchPriority="low"
              />
            </div>
            <div className="airbnb-guest-fav-title">Guest favorite</div>
            <div className="airbnb-guest-fav-desc">
              This home is a guest favorite based on ratings, reviews, and reliability
            </div>
          </div>

          <div className="mb-8 flex items-center gap-2">
            <span dangerouslySetInnerHTML={{ __html: STAR_SVG_14 }} />
            <h2 className="text-[24px] font-semibold tracking-[-0.03em] text-[#222]">
              {d.rating} {"\u00b7"} {d.reviewCount} reviews
            </h2>
          </div>

          {/* Rating cards row */}
          <div className="airbnb-ratings-row">
            {/* Overall rating card */}
            <div className="airbnb-ratings-card airbnb-ratings-card-overall">
              <h3 className="text-base font-semibold text-[#222]">Overall rating</h3>
              <div className="mt-4 airbnb-rating-bars">
                {d.reviewDistribution.map(([label, value], i) => (
                  <div key={i} className="airbnb-rating-bar">
                    <div className="airbnb-rating-bar-label">{label}</div>
                    <div className="airbnb-rating-bar-track">
                      <div
                        className="airbnb-rating-bar-fill"
                        style={{ width: `${value}%`, minWidth: "2px" }}
                      ></div>
                    </div>
                  </div>
                ))}
              </div>
            </div>
            {/* Category cards */}
            {reviewDetails.map(([name, score, icon], i) => (
              <div key={i} className="airbnb-ratings-card">
                <span className="sr-only">
                  Rated {score} out of 5 stars for {name}
                </span>
                <div className="airbnb-ratings-card-label">{name}</div>
                <div className="airbnb-ratings-card-score">{score}</div>
                <div className="airbnb-ratings-card-icon">
                  <span dangerouslySetInnerHTML={{ __html: icon }} />
                </div>
              </div>
            ))}
          </div>

          {/* Review cards */}
          <div className="mt-12 grid grid-cols-1 gap-x-10 gap-y-12 md:grid-cols-2">
            {d.reviews.map((review, i) => (
              <div
                key={i}
                data-review-card=""
                data-expanded="false"
                className="airbnb-review-card"
              >
                <div className="mb-4 flex items-start justify-between gap-4">
                  <div>
                    <div className="flex items-center gap-3">
                      <AirImage
                        src={review.avatar}
                        alt={review.name}
                        className="airbnb-review-avatar"
                        loading="lazy"
                        fetchPriority="low"
                      />
                      <div>
                        <p className="font-medium text-sm text-[#222]">{review.name}</p>
                        <p className="text-xs text-[var(--air-muted)]">{review.location}</p>
                      </div>
                    </div>
                    <div className="mt-4 flex flex-wrap items-center gap-2 text-xs text-[var(--air-muted)]">
                      <div className="airbnb-review-stars">
                        {[0, 1, 2, 3, 4].map((s) => (
                          <span key={s} aria-hidden="true">
                            <span dangerouslySetInnerHTML={{ __html: STAR_SVG_14 }} />
                          </span>
                        ))}
                      </div>
                      <span>{review.date}</span>
                      <span>{"\u00b7"}</span>
                      <span>{review.duration}</span>
                    </div>
                  </div>
                </div>
                <p data-review-body="" className="text-[15px] leading-6 text-[#222]">
                  {review.body}
                </p>
                <button
                  data-review-toggle=""
                  type="button"
                  className="mt-3 text-sm font-semibold underline underline-offset-4 text-[#222]"
                >
                  Show more
                </button>
              </div>
            ))}
          </div>

          <div className="mt-10 flex flex-wrap items-center gap-4">
            <button className="rounded-xl border border-[#222] px-6 py-3 text-sm font-semibold text-[#222] hover:bg-[var(--air-soft)]">
              Show all {d.reviewCount} reviews
            </button>
            <button
              type="button"
              className="text-sm font-semibold underline underline-offset-4 text-[#222]"
            >
              How reviews work
            </button>
          </div>
        </section>

        {/* ── Explore nearby section ────────────────────────── */}
        <section className="mt-8 bg-[var(--air-soft)] py-12">
          <div className="air-shell">
            <h2 className="text-[24px] font-semibold tracking-[-0.03em] text-[#222]">
              Explore other options in and around {d.locationShort}
            </h2>
            <div className="mt-6 grid gap-4 sm:grid-cols-2 lg:grid-cols-4">
              {d.nearbyLinks.map((link, i) => (
                <a
                  key={i}
                  href="#explore"
                  className="rounded-[20px] border border-[var(--air-line)] bg-white px-5 py-4 text-sm font-medium text-[#222] transition-colors hover:bg-[#fcfcfc]"
                >
                  {link}
                </a>
              ))}
            </div>
          </div>
        </section>
      </main>

      {/* ── Gallery modal ───────────────────────────────────── */}
      <div className="gallery-modal hidden" id="gallery-modal" hidden>
        <div
          className="gallery-dialog"
          role="dialog"
          aria-modal="true"
          aria-label="Photo gallery"
        >
          {/* Header: [Close] [counter centered] [Share + Save] */}
          <div className="gallery-header">
            <div className="gallery-header-left">
              <button
                className="gallery-header-btn"
                data-gallery-close=""
                type="button"
              >
                <span className="gallery-header-btn-icon">
                  <svg
                    viewBox="0 0 12 12"
                    aria-hidden="true"
                    focusable="false"
                    style={{
                      display: "block",
                      fill: "currentcolor",
                      height: "12px",
                      width: "12px",
                    }}
                  >
                    <path
                      d="m11.5 10.5c.3.3.3.8 0 1.1s-.8.3-1.1 0l-4.4-4.5-4.5 4.5c-.3.3-.8.3-1.1 0s-.3-.8 0-1.1l4.5-4.5-4.4-4.5c-.3-.3-.3-.8 0-1.1s.8-.3 1.1 0l4.4 4.5 4.5-4.5c.3-.3.8-.3 1.1 0s.3.8 0 1.1l-4.5 4.5z"
                      fillRule="evenodd"
                    />
                  </svg>
                </span>
                <span>Close</span>
              </button>
            </div>
            <div className="gallery-counter">
              <span
                className="sr-only"
                id="gallery-counter-sr"
                aria-live="polite"
                aria-atomic="true"
              >
                Showing photo 1 of 8
              </span>
              <span id="gallery-counter" aria-hidden="true">
                1 / 8
              </span>
            </div>
            <div className="gallery-header-right">
              <button
                className="gallery-icon-btn"
                type="button"
                aria-label="Share"
              >
                <svg
                  viewBox="0 0 32 32"
                  xmlns="http://www.w3.org/2000/svg"
                  aria-hidden="true"
                  focusable="false"
                  style={{
                    display: "block",
                    fill: "none",
                    height: "16px",
                    width: "16px",
                    stroke: "currentcolor",
                    strokeWidth: 2,
                    overflow: "visible",
                  }}
                >
                  <path d="m27 18v9c0 1.1046-.8954 2-2 2h-18c-1.10457 0-2-.8954-2-2v-9m11-15v21m-10-11 9.2929-9.29289c.3905-.39053 1.0237-.39053 1.4142 0l9.2929 9.29289" />
                </svg>
              </button>
              <button
                className="gallery-icon-btn"
                type="button"
                aria-label="Save"
              >
                <svg
                  viewBox="0 0 32 32"
                  xmlns="http://www.w3.org/2000/svg"
                  aria-hidden="true"
                  focusable="false"
                  style={{
                    display: "block",
                    fill: "none",
                    height: "16px",
                    width: "16px",
                    stroke: "currentcolor",
                    strokeWidth: 2,
                    overflow: "visible",
                  }}
                >
                  <path d="m15.9998 28.6668c7.1667-4.8847 14.3334-10.8844 14.3334-18.1088 0-1.84951-.6993-3.69794-2.0988-5.10877-1.3996-1.4098-3.2332-2.11573-5.0679-2.11573-1.8336 0-3.6683.70593-5.0668 2.11573l-2.0999 2.11677-2.0988-2.11677c-1.3995-1.4098-3.2332-2.11573-5.06783-2.11573-1.83364 0-3.66831.70593-5.06683 2.11573-1.39955 1.41083-2.09984 3.25926-2.09984 5.10877 0 7.2244 7.16667 13.2241 14.3333 18.1088z" />
                </svg>
              </button>
            </div>
          </div>
          {/* Carousel: [prev] [image] [next] */}
          <div className="gallery-stage">
            <div className="gallery-nav-slot">
              <button
                className="gallery-nav-btn"
                data-gallery-prev=""
                type="button"
                aria-label="Previous"
                disabled
              >
                <svg
                  xmlns="http://www.w3.org/2000/svg"
                  viewBox="0 0 32 32"
                  aria-hidden="true"
                  focusable="false"
                  style={{
                    display: "block",
                    fill: "none",
                    height: "12px",
                    width: "12px",
                    stroke: "currentcolor",
                    strokeWidth: 4,
                    overflow: "visible",
                  }}
                >
                  <path d="M20 28 8.7 16.7a1 1 0 0 1 0-1.4L20 4" />
                </svg>
              </button>
            </div>
            <div className="gallery-image-wrap">
              <AirImage
                id="gallery-image"
                src={d.photos[0].src}
                alt={d.photos[0].alt}
                className="gallery-image"
                loading="eager"
                fetchPriority="auto"
              />
            </div>
            <div className="gallery-nav-slot">
              <button
                className="gallery-nav-btn"
                data-gallery-next=""
                type="button"
                aria-label="Next"
              >
                <svg
                  xmlns="http://www.w3.org/2000/svg"
                  viewBox="0 0 32 32"
                  aria-hidden="true"
                  focusable="false"
                  style={{
                    display: "block",
                    fill: "none",
                    height: "12px",
                    width: "12px",
                    stroke: "currentcolor",
                    strokeWidth: 4,
                    overflow: "visible",
                  }}
                >
                  <path d="m12 4 11.3 11.3a1 1 0 0 1 0 1.4L12 28" />
                </svg>
              </button>
            </div>
          </div>
        </div>
      </div>

      <Footer />
    </>
  );
}
