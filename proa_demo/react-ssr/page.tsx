// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

// React eBay-shaped SRP demo: listing cards (watch-heart toggles) plus a
// filter chip toolbar with three independent toggles.
//
// Card markup mirrors `proa_showcase/src/ebay/srp.rs::Card` element-for-element
// (the proa card mirrors production eBay markup byte-for-byte, so the React
// tree must render the same DOM for the cross-engine comparison to be
// apples-to-apples). The only intended divergence is the watch-heart click
// target: React hydrates a <button>, proa renders an rsjs island.

import { useState } from "react";
import type { ReactNode } from "react";
import {
    CAPTION_CLASSES,
    CAPTION_LABELS,
    fmtAbbrev,
    fmtCount,
    fmtCurrency,
    fmtFeedbackPct,
    emptyStarCount,
    itemUrl,
} from "../marko-server/helpers_srp_real.mjs";

// Mirrors the shape produced by `marko-server/helpers_srp_real.mjs`'s
// `buildListings`. Only a subset of fields is rendered here; the rest
// are kept in the type so the seed data round-trips identically across
// SSR and hydration without React 19 complaining about prop drift.
export type Listing = {
    itemId: number;
    imageHash: string;
    title: string;
    subtitleParts?: string[];
    caption?: string | null;
    starsFull?: number;
    hasHalfStar?: boolean;
    reviewCount?: number;
    priceCents: number;
    originalPriceCents?: number | null;
    format?: string;
    deliveryCents: number;
    location: string;
    watcherCount: number;
    couponPct?: number | null;
    sellerName?: string;
    sellerFeedbackPctX10?: number;
    sellerFeedbackCount?: number;
};

// Mirrors `ListingFormat::slug()` in ebay_srp.rs: the per-card
// `data-format` attribute the URL-driven filter CSS matches against.
function formatSlug(label: string | undefined): string {
    if (label === "or Best Offer") return "auction";
    if (label === "Best Offer") return "bestoffer";
    return "buyitnow";
}

function WatchHeart({ ariaLabel }: { ariaLabel: string }) {
    const [watching, setWatching] = useState(false);
    // eBay's stylesheet has a rule that hides a lone `<svg class="icon">`
    // inside `.s-card__watchheart-icon`. Conditionally rendering one
    // svg makes it always invisible (and the proa rsjs island works
    // around the same constraint by emitting two sibling icon-spans
    // with inline `display:none` toggling). Mirror that shape here so
    // the heart sprite actually renders, and toggle visibility via
    // inline style — no DOM swap, just a style flip.
    return (
        <button
            type="button"
            className="s-card__watchheart-click"
            aria-label={ariaLabel}
            onClick={() => setWatching((w) => !w)}
        >
            <span className="s-card__watchheart-icon" aria-hidden="true">
                <svg
                    className="icon icon--16"
                    focusable="false"
                    aria-hidden="true"
                    style={{ display: watching ? "inline-block" : "none" }}
                >
                    <use href="#icon-heart-filled-16" />
                </svg>
            </span>
            <span className="s-card__watchheart-icon" aria-hidden="true">
                <svg
                    className="icon icon--16"
                    focusable="false"
                    aria-hidden="true"
                    style={{ display: watching ? "none" : "inline-block" }}
                >
                    <use href="#icon-heart-16" />
                </svg>
            </span>
        </button>
    );
}

function FilterChip({ label }: { label: string }) {
    const [active, setActive] = useState(false);
    return (
        <button type="button" className="filter-chip" onClick={() => setActive((v) => !v)}>
            <span className={active ? "filter-chip-label filter-chip-label--active" : "filter-chip-label"}>
                {active ? `☑ ${label}` : `☐ ${label}`}
            </span>
        </button>
    );
}

function FilterChips() {
    return (
        <div className="srp-filter-chips" role="toolbar" aria-label="Listing filters">
            <FilterChip label="Buy It Now" />
            <FilterChip label="Auction" />
            <FilterChip label="Free shipping" />
        </div>
    );
}

function CaptionBadge({ caption }: { caption: string | null | undefined }) {
    if (!caption) return null;
    const label = (CAPTION_LABELS as Record<string, string>)[caption];
    const classes = (CAPTION_CLASSES as Record<string, string>)[caption];
    if (!label) return null;
    return (
        <span className={classes} aria-label={label}>
            {label}
        </span>
    );
}

function StarRow({ full, half }: { full: number; half: boolean }) {
    const empty = emptyStarCount(full, half);
    const stars = [];
    for (let i = 0; i < full; i++) {
        stars.push(
            <svg key={`f${i}`} className="icon icon--16" focusable="false" aria-hidden="true">
                <use href="#icon-star-filled-16" />
            </svg>,
        );
    }
    if (half) {
        stars.push(
            <svg key="h" className="icon icon--16-colored" focusable="false" aria-hidden="true">
                <use href="#icon-star-half-16-colored" />
            </svg>,
        );
    }
    for (let i = 0; i < empty; i++) {
        stars.push(
            <svg key={`e${i}`} className="icon icon--16" focusable="false" aria-hidden="true">
                <use href="#icon-star-empty-16" />
            </svg>,
        );
    }
    return <>{stars}</>;
}

function Card({ listing }: { listing: Listing }) {
    const url = itemUrl(listing.itemId);
    const img = `https://i.ebayimg.com/images/g/${listing.imageHash}/s-l500.webp`;
    const parts = listing.subtitleParts ?? [];
    const subtitleJoined: ReactNode[] = [];
    parts.forEach((part, idx) => {
        subtitleJoined.push(
            <span key={idx} className="su-styled-text secondary default">
                {part}
            </span>,
        );
        if (idx !== parts.length - 1) subtitleJoined.push(" · ");
    });
    return (
        <li className="s-card s-card--horizontal">
            <div
                id={`item${listing.itemId}`}
                data-listingid={listing.itemId}
                data-format={formatSlug(listing.format)}
                data-free-shipping={listing.deliveryCents === 0 ? "1" : "0"}
                className="su-card-container su-card-container--horizontal"
            >
                <div className="su-card-container__media">
                    <div className="su-media-container s-card__media-wrapper">
                        <div className="su-media-multi-image">
                            <div className="su-media-multi-image__primary">
                                <div className="su-media-container">
                                    <div className="su-image">
                                        <a
                                            className="s-card__link image-treatment"
                                            href={url}
                                            target="_blank"
                                            rel="noreferrer"
                                            style={{ "--aspect-ratio": "1" } as React.CSSProperties}
                                        >
                                            <img
                                                className="s-card__image"
                                                fetchPriority="high"
                                                loading="eager"
                                                src={img}
                                                alt={listing.title}
                                                style={{ width: "auto" }}
                                            />
                                        </a>
                                    </div>
                                </div>
                            </div>
                        </div>
                        <div className="su-media__action su-media__action--top-right">
                            <span className="s-card__watchheart on-image s-card__watchheart--watch">
                                <WatchHeart ariaLabel={`watch ${listing.title}`} />
                            </span>
                        </div>
                    </div>
                </div>
                <div className="su-card-container__content">
                    <div className="su-card-container__header">
                        <div className="s-card__caption">
                            <CaptionBadge caption={listing.caption} />
                        </div>
                        <a className="s-card__link" target="_blank" rel="noreferrer" tabIndex={-1} href={url}>
                            <div role="heading" aria-level={3} className="s-card__title">
                                <span className="su-styled-text primary default">{listing.title}</span>
                            </div>
                        </a>
                        <div className="s-card__subtitle-row">
                            <div className="s-card__subtitle">{subtitleJoined}</div>
                        </div>
                        <div className="s-card__product-reviews">
                            <div className="s-card__reviews">
                                <a href="#UserReviews">
                                    <div className="x-star-rating">
                                        <StarRow full={listing.starsFull ?? 0} half={listing.hasHalfStar ?? false} />
                                    </div>
                                    <span className="s-card__reviews-count">
                                        <span aria-hidden="false">
                                            {fmtCount(listing.reviewCount ?? 0)} product ratings
                                        </span>
                                    </span>
                                </a>
                            </div>
                        </div>
                    </div>
                    <div className="su-card-container__attributes su-card-container__attributes--has-secondary">
                        <div className="su-card-container__attributes__primary">
                            <div className="s-card__attribute-row">
                                <span className="su-styled-text primary bold large-1 s-card__price">
                                    {fmtCurrency(listing.priceCents)}
                                </span>
                                {listing.originalPriceCents != null ? (
                                    <>
                                        <span className="su-spacer --spacing-100"></span>
                                        <span className="su-styled-text secondary strikethrough large">
                                            {fmtCurrency(listing.originalPriceCents)}
                                        </span>
                                    </>
                                ) : null}
                            </div>
                            <div className="s-card__attribute-row">
                                <span className="su-styled-text secondary large">{listing.format ?? "Buy It Now"}</span>
                            </div>
                            <div className="s-card__attribute-row">
                                <span className="su-styled-text secondary large">
                                    {listing.deliveryCents === 0
                                        ? "Free delivery"
                                        : `+${fmtCurrency(listing.deliveryCents)} delivery`}
                                </span>
                            </div>
                            <div className="s-card__attribute-row">
                                <span className="su-styled-text secondary large">Located in {listing.location}</span>
                            </div>
                            <div className="s-card__attribute-row">
                                <span className="su-styled-text primary bold large">
                                    {fmtCount(listing.watcherCount)} watchers
                                </span>
                            </div>
                            {listing.couponPct != null ? (
                                <div className="s-card__attribute-row">
                                    <span className="su-styled-text negative bold large">
                                        Extra {listing.couponPct}% off with coupon
                                    </span>
                                </div>
                            ) : null}
                        </div>
                        <div className="su-card-container__attributes__secondary">
                            <div className="s-card__attribute-row">
                                <span className="su-styled-text primary large">{listing.sellerName} </span>
                                <span className="su-styled-text primary large">
                                    {fmtFeedbackPct(listing.sellerFeedbackPctX10 ?? 0)} positive (
                                    {fmtAbbrev(listing.sellerFeedbackCount ?? 0)})
                                </span>
                            </div>
                        </div>
                    </div>
                </div>
            </div>
        </li>
    );
}

export function App({ listings }: { listings: Listing[] }): ReactNode {
    return (
        <>
            <FilterChips />
            <ul className="srp-results srp-list clearfix">
                {listings.map((l) => (
                    <Card key={l.itemId} listing={l} />
                ))}
            </ul>
        </>
    );
}
