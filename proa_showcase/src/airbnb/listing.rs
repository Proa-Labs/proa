// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Airbnb Showcase — Listing Detail
//!
//! Fuller Airbnb-style PDP modeled on the Malibu reference listing.
#![allow(unused_must_use)]

use super::calendar::AirbnbCalendar;
use super::image::{AirImage, AirImageFetchPriority, AirImageLoading};
use super::shared;
use proa_core::shared::attr_value::AttrValue;
use proa_core::{RenderBuffer, StaticRaw, UnsafeRaw, WebContext, WebRenderSync, WriteBuf};
use proa_macros::{html_sync, md_sync};

/// Pre-rendered React island HTML strings for interactive sections.
pub struct ListingIslands {
    /// "Select dates" section with two interactive calendars
    pub date_picker: String,
    /// Booking sidebar card with pricing, popover, and calendars
    pub booking_card: String,
    /// React bootstrap script (import + hydration)
    pub bootstrap: String,
}

/// Zero-allocation style attribute for `width:NN%;min-width:2px`.
struct WidthPct(u8);

impl AttrValue for WidthPct {
    #[inline(always)]
    fn should_render(&self) -> bool {
        true
    }
    #[inline(always)]
    fn render_attr_value<B: WriteBuf, L: proa_core::DataLoader>(
        &self,
        cx: &mut WebContext<L, B>,
    ) -> Result<(), proa_core::WriteError> {
        cx.out.extend(b"width:")?;
        self.0.render_attr_value(cx)?;
        cx.out.extend_static(b"%;min-width:2px")
    }
}

const STAR_SVG: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" width="14" height="14" viewBox="0 0 24 24" fill="#FF385C"><path d="M12 2l3.09 6.26L22 9.27l-5 4.87 1.18 6.88L12 17.77l-6.18 3.25L7 14.14 2 9.27l6.91-1.01L12 2z"/></svg>"##;
const HOUSE_RULES_ICON: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><path d="M9 11l3 3L22 4"/><path d="M21 12v7a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h11"/></svg>"##;
const SAFETY_ICON: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><path d="M12 3l7 4v5c0 5-3.5 7.5-7 9-3.5-1.5-7-4-7-9V7l7-4z"/><path d="M12 8v4"/><path d="M12 16h.01"/></svg>"##;
const CANCELLATION_ICON: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><rect x="3" y="4" width="18" height="18" rx="2"/><path d="M16 2v4"/><path d="M8 2v4"/><path d="M3 10h18"/><path d="M10 15l2 2 4-4"/></svg>"##;
const CLEANLINESS_ICON: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 32 32" width="32" height="32" fill="currentColor"><path d="M24 0v6h-4.3c.13 1.4.67 2.72 1.52 3.78l.2.22-1.5 1.33a9.05 9.05 0 0 1-2.2-5.08c-.83.38-1.32 1.14-1.38 2.2v4.46l4.14 4.02a5 5 0 0 1 1.5 3.09l.01.25.01.25v8.63a3 3 0 0 1-2.64 2.98l-.18.01-.21.01-12-.13A3 3 0 0 1 4 29.2L4 29.02v-8.3a5 5 0 0 1 1.38-3.45l.19-.18L10 12.9V8.85l-4.01-3.4.02-.7A5 5 0 0 1 10.78 0H11zm-5.03 25.69a8.98 8.98 0 0 1-6.13-2.41l-.23-.23A6.97 6.97 0 0 0 6 21.2v7.82c0 .51.38.93.87 1H7l11.96.13h.13a1 1 0 0 0 .91-.88l.01-.12v-3.52c-.34.04-.69.06-1.03.06zM17.67 2H11a3 3 0 0 0-2.92 2.3l-.04.18-.01.08 3.67 3.1h2.72l.02-.1a4.29 4.29 0 0 1 3.23-3.4zM30 4a1 1 0 1 1 0 2 1 1 0 0 1 0-2zm-3-2a1 1 0 1 1 0 2 1 1 0 0 1 0-2zm-5 0h-2.33v2H22zm8-2a1 1 0 1 1 0 2 1 1 0 0 1 0-2zM20 20.52a3 3 0 0 0-.77-2l-.14-.15-4.76-4.61v-4.1H12v4.1l-5.06 4.78a3 3 0 0 0-.45.53 9.03 9.03 0 0 1 7.3 2.34l.23.23A6.98 6.98 0 0 0 20 23.6z"/></svg>"##;
const ACCURACY_ICON: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 32 32" width="32" height="32" fill="currentColor"><path d="M16 1a15 15 0 1 1 0 30 15 15 0 0 1 0-30zm0 2a13 13 0 1 0 0 26 13 13 0 0 0 0-26zm7 7.59L24.41 12 13.5 22.91 7.59 17 9 15.59l4.5 4.5z"/></svg>"##;
const CHECKIN_ICON: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 32 32" width="32" height="32" fill="currentColor"><path d="M16.84 27.16v-3.4l-.26.09c-.98.32-2.03.51-3.11.55h-.7A11.34 11.34 0 0 1 1.72 13.36v-.59A11.34 11.34 0 0 1 12.77 1.72h.59c6.03.16 10.89 5.02 11.04 11.05V13.45a11.3 11.3 0 0 1-.9 4.04l-.13.3 7.91 7.9v5.6H25.7l-4.13-4.13zM10.31 7.22a3.1 3.1 0 1 1 0 6.19 3.1 3.1 0 0 1 0-6.2zm0 2.06a1.03 1.03 0 1 0 0 2.06 1.03 1.03 0 0 0 0-2.06zM22.43 25.1l4.12 4.13h2.67v-2.67l-8.37-8.37.37-.68.16-.3c.56-1.15.9-2.42.96-3.77v-.64a9.28 9.28 0 0 0-9-9h-.55a9.28 9.28 0 0 0-9 9v.54a9.28 9.28 0 0 0 13.3 8.1l.3-.16 1.52-.8v4.62z"/></svg>"##;
const COMMUNICATION_ICON: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 32 32" width="32" height="32" fill="none" stroke="currentColor" stroke-width="2"><path d="m25.5 3.5c2.2091 0 4 1.79086 4 4v13.8333c0 2.2092-1.7909 4-4 4h-5.8192l-3.6808 4.5-3.6832-4.5h-5.8168c-2.20914 0-4-1.7908-4-4v-13.8333c0-2.20914 1.79086-4 4-4z"/></svg>"##;
const LOCATION_ICON: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 32 32" width="32" height="32" fill="currentColor"><path d="M30.95 3.81a2 2 0 0 0-2.38-1.52l-7.58 1.69-10-2-8.42 1.87A1.99 1.99 0 0 0 1 5.8v21.95a1.96 1.96 0 0 0 .05.44 2 2 0 0 0 2.38 1.52l7.58-1.69 10 2 8.42-1.87A1.99 1.99 0 0 0 31 26.2V4.25a1.99 1.99 0 0 0-.05-.44zM12 4.22l8 1.6v21.96l-8-1.6zM3 27.75V5.8l7-1.55V26.2zm26-1.55-7 1.55V5.8l7-1.55z"/></svg>"##;
const VALUE_ICON: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 32 32" width="32" height="32" fill="currentColor"><path d="M16.17 2a3 3 0 0 1 1.98.74l.14.14 11 11a3 3 0 0 1 .14 4.1l-.14.14L18.12 29.3a3 3 0 0 1-4.1.14l-.14-.14-11-11A3 3 0 0 1 2 16.37l-.01-.2V5a3 3 0 0 1 2.82-3h11.35zm0 2H5a1 1 0 0 0-1 .88v11.29a1 1 0 0 0 .2.61l.1.1 11 11a1 1 0 0 0 1.31.08l.1-.08L27.88 16.7a1 1 0 0 0 .08-1.32l-.08-.1-11-11a1 1 0 0 0-.58-.28L16.17 4zM9 6a3 3 0 1 1 0 6 3 3 0 0 1 0-6zm0 2a1 1 0 1 0 0 2 1 1 0 0 0 0-2z"/></svg>"##;

// ---------------------------------------------------------------------------
// Data types — everything listing-specific is passed through these structs
// ---------------------------------------------------------------------------

pub struct ListingPhoto<'a> {
    pub src: &'a str,
    pub alt: &'a str,
}

pub struct ListingHighlight<'a> {
    pub title: &'a str,
    pub description: &'a str,
}

pub struct ListingReview<'a> {
    pub name: &'a str,
    pub location: &'a str,
    pub date: &'a str,
    pub duration: &'a str,
    pub body: &'a str,
    pub avatar: &'a str,
}

pub struct ReviewScores<'a> {
    pub cleanliness: &'a str,
    pub accuracy: &'a str,
    pub check_in: &'a str,
    pub communication: &'a str,
    pub location: &'a str,
    pub value: &'a str,
}

pub struct HostInfo<'a> {
    pub name: &'a str,
    pub photo: &'a str,
    pub review_count: u32,
    pub years_hosting: u32,
    pub response_rate: &'a str,
    pub response_time: &'a str,
    pub bio: &'a str,
}

pub struct CohostInfo<'a> {
    pub name: &'a str,
    pub photo: &'a str,
}

pub struct BookingDefaults<'a> {
    pub check_in: &'a str,
    pub checkout: &'a str,
    pub check_in_display: &'a str,
    pub checkout_display: &'a str,
    pub range_label: &'a str,
    pub guests: &'a str,
    pub free_cancel_date: &'a str,
    pub nights: u32,
}

pub struct ListingData<'a> {
    pub title: &'a str,
    pub location: &'a str,
    pub location_short: &'a str,
    pub property_line: &'a str,
    pub rating: &'a str,
    pub review_count: u32,
    pub overview_summary: &'a str,
    pub description: &'a str,
    pub nightly_rate: u32,
    pub cleaning_fee: u32,
    pub service_fee: u32,
    pub total_amenity_count: u32,
    pub photos: &'a [ListingPhoto<'a>],
    pub highlights: &'a [ListingHighlight<'a>],
    pub amenities: &'a [&'a str],
    pub review_distribution: &'a [(&'a str, u8)],
    pub review_scores: ReviewScores<'a>,
    pub reviews: &'a [ListingReview<'a>],
    pub house_rules: &'a [&'a str],
    pub safety_items: &'a [&'a str],
    pub cancellation_items: &'a [&'a str],
    pub nearby_links: &'a [&'a str],
    pub host: HostInfo<'a>,
    pub cohost: Option<CohostInfo<'a>>,
    pub booking: BookingDefaults<'a>,
}

// ---------------------------------------------------------------------------
// Sample data — Malibu listing
// ---------------------------------------------------------------------------

impl ListingData<'static> {
    pub fn malibu() -> Self {
        static PHOTOS: &[ListingPhoto<'static>] = &[
            ListingPhoto {
                src: "/images/airbnb/external/8db0ac0c-2124-4588-8331-e3d7b619bc0d.jpeg",
                alt: "Main living room with glass walls and ocean view",
            },
            ListingPhoto {
                src: "/images/airbnb/external/10e86c22-a721-4299-82fd-b1787f71ea22.jpeg",
                alt: "Terrace overlooking canyon and ocean",
            },
            ListingPhoto {
                src: "/images/airbnb/external/cd18a7b0-de22-453b-a61a-0a641a44833f.jpeg",
                alt: "Warm-toned sitting area with canyon light",
            },
            ListingPhoto {
                src: "/images/airbnb/external/52635eec-5de9-443c-acf9-04a4177061b0.jpeg",
                alt: "Bedroom with sliding doors to the deck",
            },
            ListingPhoto {
                src: "/images/airbnb/external/d09a38d0-28a7-4c4f-a142-55c93a0047ee.jpeg",
                alt: "Kitchen and dining area framed by large windows",
            },
            ListingPhoto {
                src: "/images/airbnb/external/a0b82a02-af0c-4c95-a8d9-8f7c8eea0604.jpeg",
                alt: "Outdoor dining setup with canyon backdrop",
            },
            ListingPhoto {
                src: "/images/airbnb/external/68840de4-241a-4e9e-bbeb-9b193bd6a213.jpeg",
                alt: "Bathroom with sculptural tub and warm light",
            },
            ListingPhoto {
                src: "/images/airbnb/external/c893e5ee-7f1f-40c9-8e86-04dc49bf4919.jpeg",
                alt: "Exterior showing the house perched above the canyon",
            },
        ];
        static HIGHLIGHTS: &[ListingHighlight<'static>] = &[
            ListingHighlight {
                title: "Exceptional check-in experience",
                description: "Recent guests gave the check-in process a 5-star rating.",
            },
            ListingHighlight {
                title: "Beautiful area",
                description: "Guests love this home's scenic location and sense of privacy.",
            },
            ListingHighlight {
                title: "Ocean and mountain views",
                description: "Guests consistently call out the canyon light and coastal panorama.",
            },
        ];
        static AMENITIES: &[&str] = &[
            "Ocean view",
            "Valley view",
            "Shared beach access",
            "Kitchen",
            "Wifi",
            "Free parking on premises",
            "HDTV with premium cable, Chromecast",
            "Free washer - In unit",
            "Free dryer - In unit",
            "Air conditioning",
        ];
        static REVIEW_DIST: &[(&str, u8)] = &[("5", 96), ("4", 1), ("3", 1), ("2", 1), ("1", 0)];
        static REVIEWS: &[ListingReview<'static>] = &[
            ListingReview { name: "Jessy", location: "San Francisco, California", date: "DATE", duration: "Stayed a few nights", body: "Thanks so much for a lovely visit. We so enjoyed the gorgeous view, the fire pit at sunset and the great views from inside too. There were just 3 of us so there was lots of room to spread out. The kitchen was completely stocked which was a really nice bonus since we like to cook. Will definitely come again!", avatar: "/images/airbnb/external/avatar-jessy.jpg" },
            ListingReview { name: "Calvin", location: "New York, New York", date: "DATE", duration: "Stayed about a week", body: "Incredible views. Really pleasant and helpful owner. The house is something out of the ordinary, stepping down from the cliff with cozy balconies and multiple places to work or relax. We're coming back tomorrow. It is quite the undiscovered gem.", avatar: "/images/airbnb/external/avatar-calvin.jpeg" },
            ListingReview { name: "Adriana", location: "Virginia Beach, Virginia", date: "DATE", duration: "Stayed a few nights", body: "The hosts could not have made us feel more at home, helpful and friendly while still making it feel like our own place. The home is beautiful, spacious, and the outdoor rooms are perfect in every way.", avatar: "/images/airbnb/external/avatar-reviewer-1.jpeg" },
            ListingReview { name: "Tony", location: "Orlando, Florida", date: "DATE", duration: "Stayed a few nights", body: "The home exceeded our expectations. The beautiful and relaxing views made it a perfect place for peace and quiet. Matt, our host, was incredibly helpful throughout our stay. Highly recommend.", avatar: "/images/airbnb/external/avatar-host-small.jpg" },
        ];
        static HOUSE_RULES: &[&str] = &[
            "Check-in after 4:00 PM",
            "Checkout before 11:00 AM",
            "6 guests maximum",
            "No parties or events",
        ];
        static SAFETY: &[&str] = &[
            "Exterior security cameras on property",
            "Smoke alarm",
            "Carbon monoxide alarm",
            "Free parking on premises",
        ];
        static CANCEL: &[&str] = &[
            "Free cancellation before March 13.",
            "Cancel before check-in on March 18 for a partial refund.",
            "Review the full cancellation policy before you book.",
        ];
        static NEARBY: &[&str] = &[
            "Southern California vacation rentals",
            "Los Angeles vacation rentals",
            "Palm Springs vacation rentals",
            "San Diego vacation rentals",
            "Malibu monthly stays",
            "House vacation rentals in Malibu",
            "Beach house vacation rentals in California",
            "House vacation rentals in United States",
        ];

        Self {
            title: "Breathtaking Malibu Ocean View Sanctuary",
            location: "Malibu, California, United States",
            location_short: "Malibu",
            property_line: "Entire home in Malibu, California",
            rating: "4.92",
            review_count: 75,
            overview_summary: "6 guests \u{00b7} 3 bedrooms \u{00b7} 3 beds \u{00b7} 3.5 baths",
            description: "Experience panoramic ocean and canyon views at this one-of-a-kind Malibu home, overlooking one of the most pristine canyons in Malibu.\n\nThis stunning 3 BD / 3.5 BA home was recently renovated into a modern sanctuary to reset in nature while staying close to the coast. The architecture frames sunrise, marine layer, and sunset with the kind of calm that makes you slow down immediately.\n\nFurniture may differ slightly from older photos, but the overall atmosphere remains a warm modern retreat with strong indoor-outdoor flow.",
            nightly_rate: 1042,
            cleaning_fee: 375,
            service_fee: 545,
            total_amenity_count: 46,
            photos: PHOTOS,
            highlights: HIGHLIGHTS,
            amenities: AMENITIES,
            review_distribution: REVIEW_DIST,
            review_scores: ReviewScores {
                cleanliness: "4.9",
                accuracy: "4.9",
                check_in: "5.0",
                communication: "5.0",
                location: "4.9",
                value: "4.8",
            },
            reviews: REVIEWS,
            house_rules: HOUSE_RULES,
            safety_items: SAFETY,
            cancellation_items: CANCEL,
            nearby_links: NEARBY,
            host: HostInfo {
                name: "Darya",
                photo: "/images/airbnb/external/host-darya.jpg",
                review_count: 76,
                years_hosting: 7,
                response_rate: "100%",
                response_time: "1 hr",
                bio: "Superhosts are experienced, highly rated hosts who are committed to providing great stays for guests.",
            },
            cohost: Some(CohostInfo {
                name: "Matt",
                photo: "/images/airbnb/external/cohost-matt.jpg",
            }),
            booking: BookingDefaults {
                check_in: "DATE",
                checkout: "DATE",
                check_in_display: "DATE",
                checkout_display: "DATE",
                range_label: "DATE - DATE",
                guests: "2 guests",
                free_cancel_date: "March 13",
                nights: 7,
            },
        }
    }
}

/// Pre-render the React islands for the listing page.
/// In production these would be cached per-request.
pub fn build_islands(data: &ListingData<'_>) -> ListingIslands {
    let booking_key = format!(
        "airbnb-listing:{}:{}:{}",
        data.title, data.booking.check_in, data.booking.checkout
    );
    let date_picker_module = shared::versioned_asset_path("/assets/DatePickerIsland.js");
    let booking_card_module = shared::versioned_asset_path("/assets/BookingCardIsland.js");

    // Pre-render React islands for interactive calendar + booking card
    use proa_core::ctx::Ctx;
    use proa_react::{
        react_bootstrap, HydrationStrategy, Island, ReactBootstrapConfig, ReactModuleDescriptor,
    };

    let mut dp_cx = Ctx::new();
    Island::new("DatePicker")
        .id("date-picker")
        .prop("bookingKey", booking_key.as_str())
        .prop("initialCheckIn", data.booking.check_in)
        .prop("initialCheckOut", data.booking.checkout)
        .prop("month1", 3u32)
        .prop("year1", 2026u32)
        .prop("month2", 4u32)
        .prop("year2", 2026u32)
        .size_hint(340)
        .render(&mut dp_cx)
        .unwrap();
    let date_picker = dp_cx.into_output().into_string();

    let mut bc_cx = Ctx::new();
    Island::new("BookingCard")
        .id("booking-card")
        .prop("bookingKey", booking_key.as_str())
        .prop("nightlyRate", data.nightly_rate)
        .prop("cleaningFee", data.cleaning_fee)
        .prop("serviceFee", data.service_fee)
        .prop("rating", data.rating)
        .prop("reviewCount", data.review_count)
        .prop("guests", data.booking.guests)
        .prop("freeCancelDate", data.booking.free_cancel_date)
        .prop("initialCheckIn", data.booking.check_in)
        .prop("initialCheckOut", data.booking.checkout)
        .prop("month1", 3u32)
        .prop("year1", 2026u32)
        .prop("month2", 4u32)
        .prop("year2", 2026u32)
        .size_hint(500)
        .render(&mut bc_cx)
        .unwrap();
    let booking_card = bc_cx.into_output().into_string();

    let mut bs_cx = Ctx::new();
    let config = ReactBootstrapConfig {
        hydrate_fn: "createRoot",
        ..Default::default()
    };
    proa_core::Render::render(
        react_bootstrap(
            &[
                ReactModuleDescriptor {
                    island: "DatePicker",
                    module_path: date_picker_module.as_str(),
                    export: None,
                    strategy: HydrationStrategy::Immediate,
                },
                ReactModuleDescriptor {
                    island: "BookingCard",
                    module_path: booking_card_module.as_str(),
                    export: None,
                    strategy: HydrationStrategy::Immediate,
                },
            ],
            &config,
        ),
        &mut bs_cx,
    )
    .unwrap();
    let bootstrap = bs_cx.into_output().into_string();

    ListingIslands {
        date_picker,
        booking_card,
        bootstrap,
    }
}

pub fn render() -> String {
    let data = ListingData::malibu();
    let islands = build_islands(&data);
    let mut buf = RenderBuffer::new();
    render_into(
        &mut WebContext::with_buffer(&mut buf),
        &data,
        Some(&islands),
    )
    .unwrap();
    buf.into_string()
}

pub fn render_into<B: proa_core::WriteBuf, L: ::proa_core::DataLoader>(
    cx: &mut WebContext<L, B>,
    data: &ListingData<'_>,
    islands: Option<&ListingIslands>,
) -> Result<(), proa_core::WriteError> {
    shared::render_document_start(cx, &[data.title, " | Airbnb"]);
    render_content(cx, data, islands)?;
    shared::render_document_end(cx);
    Ok(())
}

/// Render just the page content (no document shell). Used by benchmarks.
pub fn render_content<B: proa_core::WriteBuf, L: ::proa_core::DataLoader>(
    cx: &mut WebContext<L, B>,
    data: &ListingData<'_>,
    islands: Option<&ListingIslands>,
) -> Result<(), proa_core::WriteError> {
    let nightly_total = data.nightly_rate * data.booking.nights;
    let total = nightly_total + data.cleaning_fee + data.service_fee;
    let review_details: &[(&str, &str, &str)] = &[
        (
            "Cleanliness",
            data.review_scores.cleanliness,
            CLEANLINESS_ICON,
        ),
        ("Accuracy", data.review_scores.accuracy, ACCURACY_ICON),
        ("Check-in", data.review_scores.check_in, CHECKIN_ICON),
        (
            "Communication",
            data.review_scores.communication,
            COMMUNICATION_ICON,
        ),
        ("Location", data.review_scores.location, LOCATION_ICON),
        ("Value", data.review_scores.value, VALUE_ICON),
    ];
    shared::render_header(cx);

    html_sync! {
        <main id="site-content" data-airbnb-listing-page>
            <div class="air-shell pt-8">
                <section id="photos" class="pb-8">
                    <div class="mb-4 flex flex-col gap-3 md:flex-row md:items-end md:justify-between">
                        <div>
                            <h1 class="text-[30px] font-semibold leading-[1.15] tracking-[-0.045em] text-[#222] md:text-[34px]">
                                {data.title}
                            </h1>
                            <div class="mt-3 flex flex-wrap items-center gap-x-2 gap-y-1 text-sm text-[#222]">
                                {StaticRaw(STAR_SVG)}
                                <span class="font-semibold">{data.rating}</span>
                                <span>"·"</span>
                                <a href="#reviews" class="underline font-semibold">{data.review_count}" reviews"</a>
                                <span>"·"</span>
                                <span class="font-semibold">"Guest favorite"</span>
                                <span>"·"</span>
                                <span class="underline">{data.location}</span>
                            </div>
                        </div>
                        <div class="flex items-center gap-2 self-start text-sm font-medium md:self-auto">
                            <button data-action="share" class="flex items-center gap-2 rounded-lg px-3 py-2 underline-offset-2 hover:bg-[var(--air-soft)] hover:underline">
                                <svg viewBox="0 0 32 32" xmlns="http://www.w3.org/2000/svg" aria-hidden="true" role="presentation" focusable="false" style="display:block;fill:none;height:16px;width:16px;stroke:currentcolor;stroke-width:2;overflow:visible"><path d="m27 18v9c0 1.1046-.8954 2-2 2h-18c-1.10457 0-2-.8954-2-2v-9m11-15v21m-10-11 9.2929-9.29289c.3905-.39053 1.0237-.39053 1.4142 0l9.2929 9.29289" fill="none"/></svg>
                                "Share"
                            </button>
                            <button class="heart-btn flex items-center gap-2 rounded-lg px-3 py-2 underline-offset-2 hover:bg-[var(--air-soft)] hover:underline">
                                <svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M20.84 4.61a5.5 5.5 0 0 0-7.78 0L12 5.67l-1.06-1.06a5.5 5.5 0 0 0-7.78 7.78l1.06 1.06L12 21.23l7.78-7.78 1.06-1.06a5.5 5.5 0 0 0 0-7.78z"/></svg>
                                "Save"
                            </button>
                        </div>
                    </div>

                    <div class="relative grid h-[31rem] grid-cols-4 grid-rows-2 gap-2 overflow-hidden rounded-[22px]">
                        <button class="col-span-2 row-span-2 cursor-pointer overflow-hidden transition-opacity hover:opacity-95" data-gallery-open data-gallery-index="0" data-gallery-src={data.photos[0].src} data-gallery-alt={data.photos[0].alt} type="button">
                            {AirImage {
                                id: None,
                                src: data.photos[0].src,
                                alt: data.photos[0].alt,
                                class: Some("h-full w-full object-cover"),
                                loading: AirImageLoading::Eager,
                                fetch_priority: AirImageFetchPriority::High,
                                fallback_src: None,
                            }}
                        </button>
                        <button class="cursor-pointer overflow-hidden transition-opacity hover:opacity-95" data-gallery-open data-gallery-index="1" data-gallery-src={data.photos[1].src} data-gallery-alt={data.photos[1].alt} type="button">
                            {AirImage {
                                id: None,
                                src: data.photos[1].src,
                                alt: data.photos[1].alt,
                                class: Some("h-full w-full object-cover"),
                                loading: AirImageLoading::Lazy,
                                fetch_priority: AirImageFetchPriority::Low,
                                fallback_src: None,
                            }}
                        </button>
                        <button class="cursor-pointer overflow-hidden transition-opacity hover:opacity-95" data-gallery-open data-gallery-index="2" data-gallery-src={data.photos[2].src} data-gallery-alt={data.photos[2].alt} type="button">
                            {AirImage {
                                id: None,
                                src: data.photos[2].src,
                                alt: data.photos[2].alt,
                                class: Some("h-full w-full object-cover"),
                                loading: AirImageLoading::Lazy,
                                fetch_priority: AirImageFetchPriority::Low,
                                fallback_src: None,
                            }}
                        </button>
                        <button class="cursor-pointer overflow-hidden transition-opacity hover:opacity-95" data-gallery-open data-gallery-index="3" data-gallery-src={data.photos[3].src} data-gallery-alt={data.photos[3].alt} type="button">
                            {AirImage {
                                id: None,
                                src: data.photos[3].src,
                                alt: data.photos[3].alt,
                                class: Some("h-full w-full object-cover"),
                                loading: AirImageLoading::Lazy,
                                fetch_priority: AirImageFetchPriority::Low,
                                fallback_src: None,
                            }}
                        </button>
                        <button class="cursor-pointer overflow-hidden transition-opacity hover:opacity-95" data-gallery-open data-gallery-index="4" data-gallery-src={data.photos[4].src} data-gallery-alt={data.photos[4].alt} type="button">
                            {AirImage {
                                id: None,
                                src: data.photos[4].src,
                                alt: data.photos[4].alt,
                                class: Some("h-full w-full object-cover"),
                                loading: AirImageLoading::Lazy,
                                fetch_priority: AirImageFetchPriority::Low,
                                fallback_src: None,
                            }}
                        </button>
                        <button class="absolute bottom-4 right-4 rounded-xl border border-[#222] bg-white px-4 py-2 text-sm font-medium text-[#222] shadow-sm hover:bg-[var(--air-soft)]" data-gallery-open data-gallery-index="0" type="button">
                            "Show all photos"
                        </button>
                    </div>

                    {for (index, photo) in data.photos.iter().enumerate().skip(5) {
                        html_sync! {
                            <span class="hidden" data-gallery-src={photo.src} data-gallery-alt={photo.alt} data-gallery-index={index}></span>
                        }
                    }}
                </section>
            </div>

            <div class="sticky top-[80px] z-30 hidden border-y border-[var(--air-line)] bg-white/95 backdrop-blur lg:block">
                <div class="air-shell flex items-center justify-between gap-8 py-4">
                    <nav class="flex items-center gap-6 text-sm font-semibold text-[#222]">
                        <a href="#photos" class="air-link">"Photos"</a>
                        <a href="#amenities" class="air-link">"Amenities"</a>
                        <a href="#reviews" class="air-link">"Reviews"</a>
                        <a href="#location" class="air-link">"Location"</a>
                    </nav>
                    <div class="flex items-center gap-5">
                        <div class="text-right">
                            <div class="text-sm font-semibold text-[#222]">
                                <span>"$"{data.nightly_rate}</span>
                                <span class="font-normal text-[var(--air-muted)]">" night"</span>
                            </div>
                            <div class="flex items-center justify-end gap-1 text-xs text-[#222]">
                                {StaticRaw(STAR_SVG)}
                                <span>{data.rating}" · "{data.review_count}" reviews"</span>
                            </div>
                        </div>
                        <button class="rounded-xl bg-gradient-to-r from-[#E61E4D] via-[#E31C5F] to-[#D70466] px-5 py-3 text-sm font-semibold text-white">
                            "Reserve"
                        </button>
                    </div>
                </div>
            </div>

            <div class="air-shell py-10">
                <div class="grid grid-cols-1 gap-y-12 lg:grid-cols-12 lg:gap-x-16">
                    <div class="lg:col-span-7">
                        <section class="pb-8">
                            <div class="flex items-start justify-between gap-4">
                                <div>
                                    <h2 class="text-[26px] font-semibold tracking-[-0.03em] text-[#222]">
                                        {data.property_line}
                                    </h2>
                                    <p class="mt-2 text-[16px] text-[#222]">{data.overview_summary}</p>
                                </div>
                                {AirImage {
                                    id: None,
                                    src: data.host.photo,
                                    alt: "Host portrait",
                                    class: Some("h-14 w-14 rounded-full object-cover"),
                                    loading: AirImageLoading::Lazy,
                                    fetch_priority: AirImageFetchPriority::Low,
                                    fallback_src: None,
                                }}
                            </div>
                        </section>

                        <hr class="air-divider" />

                        <section class="py-8">
                            <div class="airbnb-favorite-strip">
                                <div class="airbnb-favorite-badge">
                                    <div class="text-[#222]">{StaticRaw(STAR_SVG)}</div>
                                    <div>
                                        <div class="text-base font-semibold text-[#222]">"Guest favorite"</div>
                                        <div class="text-sm text-[var(--air-muted)]">"Highly rated for reliability"</div>
                                    </div>
                                </div>
                                <div class="airbnb-favorite-copy">
                                    "One of the most loved homes on Airbnb, according to guests"
                                </div>
                                <div class="airbnb-favorite-score">
                                    <div class="text-[30px] font-semibold tracking-[-0.04em] text-[#222]">{data.rating}</div>
                                    <div class="text-sm font-medium text-[#222]">{data.review_count}" reviews"</div>
                                </div>
                            </div>
                        </section>

                        <hr class="air-divider" />

                        <section class="py-8">
                            <div class="flex items-start gap-5">
                                {AirImage {
                                    id: None,
                                    src: data.host.photo,
                                    alt: "Host portrait",
                                    class: Some("h-16 w-16 rounded-full object-cover"),
                                    loading: AirImageLoading::Lazy,
                                    fetch_priority: AirImageFetchPriority::Low,
                                    fallback_src: None,
                                }}
                                <div>
                                    <h3 class="text-[22px] font-semibold tracking-[-0.03em] text-[#222]">"Hosted by "{data.host.name}</h3>
                                    <p class="mt-1 text-sm text-[var(--air-muted)]">"Superhost · "{data.host.years_hosting}" years hosting"</p>
                                    <div class="mt-4 grid grid-cols-2 gap-4 sm:grid-cols-4">
                                        <div>
                                            <div class="text-lg font-semibold text-[#222]">{data.host.review_count}</div>
                                            <div class="text-sm text-[var(--air-muted)]">"Reviews"</div>
                                        </div>
                                        <div>
                                            <div class="text-lg font-semibold text-[#222]">{data.rating}</div>
                                            <div class="text-sm text-[var(--air-muted)]">"Rating"</div>
                                        </div>
                                        <div>
                                            <div class="text-lg font-semibold text-[#222]">{data.host.response_rate}</div>
                                            <div class="text-sm text-[var(--air-muted)]">"Response rate"</div>
                                        </div>
                                        <div>
                                            <div class="text-lg font-semibold text-[#222]">{data.host.response_time}</div>
                                            <div class="text-sm text-[var(--air-muted)]">"Response time"</div>
                                        </div>
                                    </div>
                                </div>
                            </div>
                        </section>

                        <hr class="air-divider" />

                        <section class="space-y-6 py-8">
                            {for highlight in data.highlights.iter() {
                                html_sync! {
                                    <div class="flex gap-4">
                                        <div class="flex h-10 w-10 shrink-0 items-center justify-center rounded-xl bg-[var(--air-soft)] text-[#222]">
                                            <svg xmlns="http://www.w3.org/2000/svg" width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.9" stroke-linecap="round" stroke-linejoin="round"><path d="M5 12h14"/><path d="M12 5v14"/></svg>
                                        </div>
                                        <div>
                                            <p class="font-medium text-[#222]">{highlight.title}</p>
                                            <p class="mt-1 text-[15px] leading-6 text-[var(--air-muted)]">{highlight.description}</p>
                                        </div>
                                    </div>
                                }
                            }}
                        </section>

                        <hr class="air-divider" />

                        <section class="py-8">
                            <h2 class="mb-5 text-[24px] font-semibold tracking-[-0.03em] text-[#222]">"About this place"</h2>
                            <div class="space-y-4 text-[16px] leading-7 text-[#222]">
                                {for paragraph in data.description.split("\n\n") {
                                    html_sync! { <p>{paragraph}</p> }
                                }}
                            </div>
                            <button class="mt-5 inline-flex items-center gap-2 rounded-lg px-1 py-1 text-sm font-semibold underline underline-offset-4">
                                "Show more"
                            </button>
                        </section>

                        <hr class="air-divider" />

                        <section id="amenities" class="py-8">
                            <div class="mb-6 flex items-end justify-between gap-4">
                                <h2 class="text-[24px] font-semibold tracking-[-0.03em] text-[#222]">"What this place offers"</h2>
                                <span class="text-sm text-[var(--air-muted)]">"Preview of "{data.total_amenity_count}" amenities"</span>
                            </div>
                            <div class="grid grid-cols-1 gap-x-10 gap-y-5 sm:grid-cols-2">
                                {for amenity in data.amenities.iter() {
                                    html_sync! {
                                        <div class="flex items-center gap-4 border-b border-[var(--air-line)] pb-4 text-[#222]">
                                            <div class="flex h-10 w-10 shrink-0 items-center justify-center rounded-xl bg-[var(--air-soft)]">
                                                <svg xmlns="http://www.w3.org/2000/svg" width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><path d="M4 12h16"/><path d="M12 4v16"/></svg>
                                            </div>
                                            <span class="text-[15px]">{*amenity}</span>
                                        </div>
                                    }
                                }}
                            </div>
                            <button class="mt-8 rounded-xl border border-[#222] px-6 py-3 text-sm font-semibold text-[#222] hover:bg-[var(--air-soft)]">
                                "Show all "{data.total_amenity_count}" amenities"
                            </button>
                        </section>

                        <hr class="air-divider" />

                        {if let Some(islands) = islands {
                            html_sync! { {UnsafeRaw(&islands.date_picker)} }
                        } else {
                            html_sync! {
                                <section class="py-8">
                                    <div class="mb-6 flex items-end justify-between gap-4">
                                        <div>
                                            <h2 class="text-[24px] font-semibold tracking-[-0.03em] text-[#222]">"Select dates"</h2>
                                            <p class="mt-2 text-[15px] text-[var(--air-muted)]">"Add your travel dates for exact pricing"</p>
                                        </div>
                                        <button data-booking-clear-all type="button" class="text-sm font-semibold underline underline-offset-4">"Clear dates"</button>
                                    </div>
                                    <div class="grid gap-5 md:grid-cols-2">
                                        <div data-airbnb-synced-calendar class="rounded-[22px] border border-[var(--air-line)] p-5">
                                            {AirbnbCalendar::new(2026, 3).with_range(18, 25)}
                                        </div>
                                        <div data-airbnb-synced-calendar class="rounded-[22px] border border-[var(--air-line)] p-5">
                                            {AirbnbCalendar::new(2026, 4)}
                                        </div>
                                    </div>
                                </section>
                            }
                        }}

                        <hr class="air-divider" />

                        <section id="location" class="py-8">
                            <h2 class="text-[24px] font-semibold tracking-[-0.03em] text-[#222]">"Where you'll be"</h2>
                            <p class="mt-2 text-[15px] text-[var(--air-muted)]">{data.location}</p>
                            <div class="air-map relative mt-6 h-[360px] overflow-hidden rounded-[26px] border border-[var(--air-line)]">
                                <div class="absolute inset-0 bg-[linear-gradient(135deg,rgba(255,255,255,0.24),transparent_42%)]"></div>
                                <div class="absolute left-[18%] top-[24%] h-28 w-28 rounded-full border border-white/60 bg-white/35 blur-[2px]"></div>
                                <div class="absolute right-[14%] top-[18%] h-20 w-20 rounded-full border border-white/60 bg-white/30 blur-[2px]"></div>
                                <div class="absolute bottom-[18%] left-[52%] h-24 w-24 rounded-full border border-white/60 bg-white/35 blur-[2px]"></div>
                                <div class="air-price-pin left-[47%] top-[44%] bg-[#222] text-white shadow-lg">
                                    {data.location_short}
                                </div>
                            </div>
                            <p class="mt-5 max-w-2xl text-[15px] leading-6 text-[var(--air-muted)]">
                                "This listing's location is verified and the exact location will be provided after booking. Expect quick access to the coast with a quieter canyon setting than many central Malibu homes."
                            </p>
                        </section>

                        <hr class="air-divider" />

                        <section class="py-8">
                            <div class="grid gap-8 lg:grid-cols-[320px_1fr]">
                                <div class="rounded-[26px] bg-[var(--air-soft)] p-6">
                                    <div class="flex items-center gap-4">
                                        {AirImage {
                                            id: None,
                                            src: data.host.photo,
                                            alt: "Host portrait",
                                            class: Some("h-20 w-20 rounded-full object-cover"),
                                            loading: AirImageLoading::Lazy,
                                            fetch_priority: AirImageFetchPriority::Low,
                                            fallback_src: None,
                                        }}
                                        <div>
                                            <h2 class="text-[24px] font-semibold tracking-[-0.03em] text-[#222]">"Meet your host"</h2>
                                            <p class="mt-1 text-sm text-[var(--air-muted)]">{data.host.name}" · Superhost"</p>
                                        </div>
                                    </div>
                                    <div class="mt-6 grid grid-cols-3 gap-4 border-t border-[var(--air-line)] pt-5 text-center">
                                        <div>
                                            <div class="text-xl font-semibold text-[#222]">{data.host.review_count}</div>
                                            <div class="text-xs text-[var(--air-muted)]">"Reviews"</div>
                                        </div>
                                        <div>
                                            <div class="text-xl font-semibold text-[#222]">{data.rating}</div>
                                            <div class="text-xs text-[var(--air-muted)]">"Rating"</div>
                                        </div>
                                        <div>
                                            <div class="text-xl font-semibold text-[#222]">{data.host.years_hosting}</div>
                                            <div class="text-xs text-[var(--air-muted)]">"Years hosting"</div>
                                        </div>
                                    </div>
                                </div>
                                <div>
                                    <h3 class="text-[22px] font-semibold tracking-[-0.03em] text-[#222]">{data.host.name}" is a Superhost"</h3>
                                    <p class="mt-3 max-w-2xl text-[15px] leading-6 text-[var(--air-muted)]">
                                        {data.host.bio}" "{data.host.name}" lives in "{data.location}
                                        {if let Some(ref cohost) = data.cohost {
                                            html_sync! {" and cohosts with "{cohost.name}" for smoother arrivals and faster support."}
                                        } else {
                                            html_sync! {"."}
                                        }}
                                    </p>
                                    <div class="mt-6 flex flex-wrap items-center gap-6">
                                        {if let Some(ref cohost) = data.cohost {
                                            html_sync! {
                                                <div class="flex items-center gap-3">
                                                    {AirImage {
                                                        id: None,
                                                        src: cohost.photo,
                                                        alt: "Cohost portrait",
                                                        class: Some("h-12 w-12 rounded-full object-cover"),
                                                        loading: AirImageLoading::Lazy,
                                                        fetch_priority: AirImageFetchPriority::Low,
                                                        fallback_src: None,
                                                    }}
                                                    <div>
                                                        <div class="text-sm font-semibold text-[#222]">{cohost.name}</div>
                                                        <div class="text-sm text-[var(--air-muted)]">"Co-host"</div>
                                                    </div>
                                                </div>
                                            }
                                        }}
                                        <div class="text-sm text-[var(--air-muted)]">"Response rate: "{data.host.response_rate}" · Responds within "{data.host.response_time}</div>
                                    </div>
                                    <div class="mt-6 rounded-[20px] border border-[var(--air-line)] p-5 text-[15px] leading-6 text-[var(--air-muted)]">
                                        "To help protect your payment, always use Airbnb to send money and communicate with hosts."
                                    </div>
                                    <button class="mt-6 rounded-xl border border-[#222] px-6 py-3 text-sm font-semibold text-[#222] hover:bg-[var(--air-soft)]">
                                        "Message host"
                                    </button>
                                </div>
                            </div>
                        </section>

                        <hr class="air-divider" />

                        <section class="py-8">
                            <h2 class="mb-6 text-[24px] font-semibold tracking-[-0.03em] text-[#222]">"Things to know"</h2>
                            <div class="grid gap-6 md:grid-cols-3">
                                <div class="rounded-[22px] border border-[var(--air-line)] p-6">
                                    <div class="mb-4 text-[#222]">{StaticRaw(HOUSE_RULES_ICON)}</div>
                                    <h3 class="text-lg font-semibold text-[#222]">"House rules"</h3>
                                    <div class="mt-4 space-y-3 text-sm text-[var(--air-muted)]">
                                        {for item in data.house_rules.iter() {
                                            html_sync! { <p>{*item}</p> }
                                        }}
                                    </div>
                                </div>
                                <div class="rounded-[22px] border border-[var(--air-line)] p-6">
                                    <div class="mb-4 text-[#222]">{StaticRaw(SAFETY_ICON)}</div>
                                    <h3 class="text-lg font-semibold text-[#222]">"Safety & property"</h3>
                                    <div class="mt-4 space-y-3 text-sm text-[var(--air-muted)]">
                                        {for item in data.safety_items.iter() {
                                            html_sync! { <p>{*item}</p> }
                                        }}
                                    </div>
                                </div>
                                <div class="rounded-[22px] border border-[var(--air-line)] p-6">
                                    <div class="mb-4 text-[#222]">{StaticRaw(CANCELLATION_ICON)}</div>
                                    <h3 class="text-lg font-semibold text-[#222]">"Cancellation policy"</h3>
                                    <div class="mt-4 space-y-3 text-sm text-[var(--air-muted)]">
                                        {for item in data.cancellation_items.iter() {
                                            html_sync! { <p>{*item}</p> }
                                        }}
                                    </div>
                                </div>
                            </div>
                        </section>
                    </div>

                    <aside class="lg:col-span-4 lg:col-start-9">
                        {if let Some(islands) = islands {
                            html_sync! { <div class="sticky top-28">{UnsafeRaw(&islands.booking_card)}</div> }
                        } else {
                            html_sync! {
                                <div
                                    class="sticky top-28"
                                    data-airbnb-booking-root
                                    data-initial-checkin={data.booking.check_in}
                                    data-initial-checkout={data.booking.checkout}
                                    data-nightly-rate={data.nightly_rate}
                                    data-cleaning-fee={data.cleaning_fee}
                                    data-service-fee={data.service_fee}
                                >
                                    <div class="relative">
                                        <div class="mb-4 flex justify-end">
                                            <div class="airbnb-booking-pill">
                                                {StaticRaw(STAR_SVG)}
                                                <span>"Prices include all fees"</span>
                                            </div>
                                        </div>

                                        <div class="air-booking-card airbnb-booking-card p-6">
                                            <div class="mb-4">
                                                <div class="flex flex-wrap items-baseline gap-x-2 gap-y-1 text-[#222]">
                                                    <span data-booking-header-total class="text-[32px] font-semibold tracking-[-0.04em] text-[#222]">"$"{total}</span>
                                                    <span data-booking-header-context class="text-[15px] text-[var(--air-muted)]">"for "{data.booking.nights}" nights"</span>
                                                </div>
                                                <div class="mt-2 flex items-center gap-1 text-sm text-[#222]">
                                                    {StaticRaw(STAR_SVG)}
                                                    <span class="font-medium">{data.rating}</span>
                                                    <span>"·"</span>
                                                    <a href="#reviews" class="underline">{data.review_count}" reviews"</a>
                                                </div>
                                            </div>

                                            <div class="mb-3 overflow-hidden rounded-2xl border border-[#B0B0B0]">
                                                <div class="grid grid-cols-2">
                                                    <button data-booking-trigger="checkIn" type="button" class="airbnb-booking-trigger border-b border-r border-[#B0B0B0] px-3 py-2.5 text-left transition-colors hover:bg-[var(--air-soft)]">
                                                        <span class="block text-[10px] font-bold tracking-wide uppercase text-[#222]">"CHECK-IN"</span>
                                                        <span data-booking-display="checkIn" class="airbnb-booking-display mt-0.5 block text-sm text-[#222]">{data.booking.check_in_display}</span>
                                                    </button>
                                                    <button data-booking-trigger="checkOut" type="button" class="airbnb-booking-trigger border-b border-[#B0B0B0] px-3 py-2.5 text-left transition-colors hover:bg-[var(--air-soft)]">
                                                        <span class="block text-[10px] font-bold tracking-wide uppercase text-[#222]">"CHECKOUT"</span>
                                                        <span data-booking-display="checkOut" class="airbnb-booking-display mt-0.5 block text-sm text-[#222]">{data.booking.checkout_display}</span>
                                                    </button>
                                                </div>
                                                <div class="px-3 py-2.5">
                                                    <label class="block text-[10px] font-bold tracking-wide uppercase text-[#222]">"GUESTS"</label>
                                                    <p class="mt-0.5 text-sm text-[#222]">{data.booking.guests}</p>
                                                </div>
                                            </div>

                                            <div class="mb-4 rounded-xl bg-[var(--air-soft)] px-4 py-2.5 text-center text-xs font-medium text-[#222]">
                                                "Free cancellation before "{data.booking.free_cancel_date}
                                            </div>

                                            <button class="mb-4 w-full rounded-xl bg-gradient-to-r from-[#E61E4D] via-[#E31C5F] to-[#D70466] py-3.5 font-semibold text-white transition-transform hover:scale-[1.01]">
                                                "Reserve"
                                            </button>

                                            <p class="mb-5 text-center text-sm text-[var(--air-muted)]">"You won't be charged yet"</p>

                                            <div class="space-y-4 text-sm">
                                                <div class="flex justify-between">
                                                    <span data-booking-night-line-label class="text-[#222] underline">"$"{data.nightly_rate}" x "{data.booking.nights}" nights"</span>
                                                    <span data-booking-night-line-value class="text-[#222]">"$"{nightly_total}</span>
                                                </div>
                                                <div class="flex justify-between">
                                                    <span class="text-[#222] underline">"Cleaning fee"</span>
                                                    <span class="text-[#222]">"$"{data.cleaning_fee}</span>
                                                </div>
                                                <div class="flex justify-between">
                                                    <span class="text-[#222] underline">"Airbnb service fee"</span>
                                                    <span class="text-[#222]">"$"{data.service_fee}</span>
                                                </div>
                                                <hr class="air-divider" />
                                                <div class="flex justify-between font-semibold">
                                                    <span class="text-[#222]">"Total before taxes"</span>
                                                    <span data-booking-total-value class="text-[#222]">"$"{total}</span>
                                                </div>
                                            </div>
                                        </div>

                                        <div data-booking-popover aria-hidden="true" class="airbnb-booking-popover hidden">
                                            <div class="airbnb-popover-shell">
                                                <div class="airbnb-popover-header">
                                                    <div class="airbnb-popover-summary">
                                                        <h3 data-booking-nights>{data.booking.nights}" nights"</h3>
                                                        <p data-booking-range-label>{data.booking.range_label}</p>
                                                    </div>
                                                    <div class="airbnb-popover-fields">
                                                        <div class="airbnb-popover-field">
                                                            <button data-booking-trigger="checkIn" type="button" class="block w-full text-left">
                                                                <span class="airbnb-popover-field-label">"Check-in"</span>
                                                                <span data-booking-popover-display="checkIn" class="airbnb-popover-field-value airbnb-popover-value">{data.booking.check_in_display}</span>
                                                            </button>
                                                            <button data-booking-clear-field="checkIn" type="button" class="airbnb-popover-clear" aria-label="Clear dates">
                                                                <svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 32 32" width="12" height="12" aria-hidden="true"><path d="m6 6 20 20M26 6 6 26" fill="none" stroke="currentColor" stroke-width="4"/></svg>
                                                            </button>
                                                        </div>
                                                        <div class="airbnb-popover-field">
                                                            <button data-booking-trigger="checkOut" type="button" class="block w-full text-left">
                                                                <span class="airbnb-popover-field-label">"Checkout"</span>
                                                                <span data-booking-popover-display="checkOut" class="airbnb-popover-field-value airbnb-popover-value">{data.booking.checkout_display}</span>
                                                            </button>
                                                            <button data-booking-clear-field="checkOut" type="button" class="airbnb-popover-clear" aria-label="Clear checkout date">
                                                                <svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 32 32" width="12" height="12" aria-hidden="true"><path d="m6 6 20 20M26 6 6 26" fill="none" stroke="currentColor" stroke-width="4"/></svg>
                                                            </button>
                                                        </div>
                                                    </div>
                                                </div>

                                                <div class="airbnb-popover-calendars">
                                                    <div data-airbnb-synced-calendar class="airbnb-popover-calendar">
                                                        {AirbnbCalendar::new(2026, 3).with_range(18, 25)}
                                                    </div>
                                                    <div data-airbnb-synced-calendar class="airbnb-popover-calendar">
                                                        {AirbnbCalendar::new(2026, 4)}
                                                    </div>
                                                </div>

                                                <div class="airbnb-popover-footer">
                                                    <button data-booking-clear-all type="button" class="airbnb-link-button">"Clear dates"</button>
                                                    <button data-booking-close type="button" class="airbnb-close-button">"Close"</button>
                                                </div>
                                            </div>
                                        </div>
                                    </div>

                                    <button class="mt-6 inline-flex items-center gap-2 text-sm font-semibold text-[#222] underline underline-offset-4">
                                        <svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.9" stroke-linecap="round" stroke-linejoin="round"><path d="M12 9v4"/><path d="M12 17h.01"/><path d="M10.29 3.86 1.82 18a2 2 0 0 0 1.71 3h16.94a2 2 0 0 0 1.71-3L13.71 3.86a2 2 0 0 0-3.42 0z"/></svg>
                                        "Report this listing"
                                    </button>
                                </div>
                            }
                        }}
                    </aside>
                </div>
            </div>

            <hr class="air-divider mx-auto" style="max-width:var(--air-shell-max)" />

            <section id="reviews" class="air-shell py-10">
                // Guest favorite badge
                <div class="airbnb-guest-fav-badge">
                    <div class="airbnb-guest-fav-laurels">
                        {AirImage {
                            id: None,
                            src: "/images/airbnb/external/laurel-left.png",
                            alt: "Laurel left",
                            class: Some("airbnb-guest-fav-laurel"),
                            loading: AirImageLoading::Lazy,
                            fetch_priority: AirImageFetchPriority::Low,
                            fallback_src: None,
                        }}
                        <div class="airbnb-guest-fav-score">
                            <span class="sr-only">"Rated "{data.rating}" out of 5 from "{data.review_count}" reviews."</span>
                            <div aria-hidden="true">{data.rating}</div>
                        </div>
                        {AirImage {
                            id: None,
                            src: "/images/airbnb/external/laurel-right.png",
                            alt: "Laurel right",
                            class: Some("airbnb-guest-fav-laurel"),
                            loading: AirImageLoading::Lazy,
                            fetch_priority: AirImageFetchPriority::Low,
                            fallback_src: None,
                        }}
                    </div>
                    <div class="airbnb-guest-fav-title">"Guest favorite"</div>
                    <div class="airbnb-guest-fav-desc">"This home is a guest favorite based on ratings, reviews, and reliability"</div>
                </div>

                <div class="mb-8 flex items-center gap-2">
                    {StaticRaw(STAR_SVG)}
                    <h2 class="text-[24px] font-semibold tracking-[-0.03em] text-[#222]">{data.rating}" · "{data.review_count}" reviews"</h2>
                </div>

                // Rating cards: overall + categories in a horizontal scroll row
                <div class="airbnb-ratings-row">
                    // Overall rating card
                    <div class="airbnb-ratings-card airbnb-ratings-card-overall">
                        <h3 class="text-base font-semibold text-[#222]">"Overall rating"</h3>
                        <div class="mt-4 airbnb-rating-bars">
                            {for (label, value) in data.review_distribution.iter() {
                                html_sync! {
                                    <div class="airbnb-rating-bar">
                                        <div class="airbnb-rating-bar-label">{*label}</div>
                                        <div class="airbnb-rating-bar-track">
                                            <div class="airbnb-rating-bar-fill" style={proa_core::style_ok(WidthPct(*value))}></div>
                                        </div>
                                    </div>
                                }
                            }}
                        </div>
                    </div>
                    // Category cards
                    {for item in review_details.iter() {
                        html_sync! {
                            <div class="airbnb-ratings-card">
                                <span class="sr-only">"Rated "{item.1}" out of 5 stars for "{item.0}</span>
                                <div class="airbnb-ratings-card-label">{item.0}</div>
                                <div class="airbnb-ratings-card-score">{item.1}</div>
                                <div class="airbnb-ratings-card-icon">{StaticRaw(item.2)}</div>
                            </div>
                        }
                    }}
                </div>

                <div class="mt-12 grid grid-cols-1 gap-x-10 gap-y-12 md:grid-cols-2">
                    {for review in data.reviews.iter() {
                        html_sync! {
                            <div data-review-card data-expanded="false" class="airbnb-review-card">
                                <div class="mb-4 flex items-start justify-between gap-4">
                                    <div>
                                        <div class="flex items-center gap-3">
                                            {AirImage {
                                                id: None,
                                                src: review.avatar,
                                                alt: review.name,
                                                class: Some("airbnb-review-avatar"),
                                                loading: AirImageLoading::Lazy,
                                                fetch_priority: AirImageFetchPriority::Low,
                                                fallback_src: None,
                                            }}
                                            <div>
                                                <p class="font-medium text-sm text-[#222]">{review.name}</p>
                                                <p class="text-xs text-[var(--air-muted)]">{review.location}</p>
                                            </div>
                                        </div>
                                        <div class="mt-4 flex flex-wrap items-center gap-2 text-xs text-[var(--air-muted)]">
                                            <div class="airbnb-review-stars">
                                                {for _ in 0..5 {
                                                    html_sync! { <span aria-hidden="true">{StaticRaw(STAR_SVG)}</span> }
                                                }}
                                            </div>
                                            <span>{review.date}</span>
                                            <span>"·"</span>
                                            <span>{review.duration}</span>
                                        </div>
                                    </div>
                                </div>
                                <p data-review-body class="text-[15px] leading-6 text-[#222]">{review.body}</p>
                                <button data-review-toggle type="button" class="mt-3 text-sm font-semibold underline underline-offset-4 text-[#222]">
                                    "Show more"
                                </button>
                            </div>
                        }
                    }}
                </div>

                <div class="mt-10 flex flex-wrap items-center gap-4">
                    <button class="rounded-xl border border-[#222] px-6 py-3 text-sm font-semibold text-[#222] hover:bg-[var(--air-soft)]">
                        "Show all "{data.review_count}" reviews"
                    </button>
                    <button type="button" class="text-sm font-semibold underline underline-offset-4 text-[#222]">
                        "How reviews work"
                    </button>
                </div>
            </section>

            <section class="mt-8 bg-[var(--air-soft)] py-12">
                <div class="air-shell">
                    <h2 class="text-[24px] font-semibold tracking-[-0.03em] text-[#222]">"Explore other options in and around "{data.location_short}</h2>
                    <div class="mt-6 grid gap-4 sm:grid-cols-2 lg:grid-cols-4">
                        {for link in data.nearby_links.iter() {
                            html_sync! {
                                <a href="#explore" class="rounded-[20px] border border-[var(--air-line)] bg-white px-5 py-4 text-sm font-medium text-[#222] transition-colors hover:bg-[#fcfcfc]">
                                    {*link}
                                </a>
                            }
                        }}
                    </div>
                </div>
            </section>
        </main>

        <div class="gallery-modal hidden" id="gallery-modal">
            <div class="gallery-dialog" role="dialog" aria-modal="true" aria-label="Photo gallery">
                // Header: [Close] [counter centered] [Share + Save]
                <div class="gallery-header">
                    <div class="gallery-header-left">
                        <button class="gallery-header-btn" data-gallery-close type="button">
                            <span class="gallery-header-btn-icon">
                                <svg viewBox="0 0 12 12" aria-hidden="true" focusable="false" style="display:block;fill:currentcolor;height:12px;width:12px"><path d="m11.5 10.5c.3.3.3.8 0 1.1s-.8.3-1.1 0l-4.4-4.5-4.5 4.5c-.3.3-.8.3-1.1 0s-.3-.8 0-1.1l4.5-4.5-4.4-4.5c-.3-.3-.3-.8 0-1.1s.8-.3 1.1 0l4.4 4.5 4.5-4.5c.3-.3.8-.3 1.1 0s.3.8 0 1.1l-4.5 4.5z" fill-rule="evenodd"/></svg>
                            </span>
                            <span>"Close"</span>
                        </button>
                    </div>
                    <div class="gallery-counter">
                        <span class="sr-only" id="gallery-counter-sr" aria-live="polite" aria-atomic="true">"Showing photo 1 of 8"</span>
                        <span id="gallery-counter" aria-hidden="true">"1 / 8"</span>
                    </div>
                    <div class="gallery-header-right">
                        <button class="gallery-icon-btn" type="button" aria-label="Share">
                            <svg viewBox="0 0 32 32" xmlns="http://www.w3.org/2000/svg" aria-hidden="true" focusable="false" style="display:block;fill:none;height:16px;width:16px;stroke:currentcolor;stroke-width:2;overflow:visible"><path d="m27 18v9c0 1.1046-.8954 2-2 2h-18c-1.10457 0-2-.8954-2-2v-9m11-15v21m-10-11 9.2929-9.29289c.3905-.39053 1.0237-.39053 1.4142 0l9.2929 9.29289"/></svg>
                        </button>
                        <button class="gallery-icon-btn" type="button" aria-label="Save">
                            <svg viewBox="0 0 32 32" xmlns="http://www.w3.org/2000/svg" aria-hidden="true" focusable="false" style="display:block;fill:none;height:16px;width:16px;stroke:currentcolor;stroke-width:2;overflow:visible"><path d="m15.9998 28.6668c7.1667-4.8847 14.3334-10.8844 14.3334-18.1088 0-1.84951-.6993-3.69794-2.0988-5.10877-1.3996-1.4098-3.2332-2.11573-5.0679-2.11573-1.8336 0-3.6683.70593-5.0668 2.11573l-2.0999 2.11677-2.0988-2.11677c-1.3995-1.4098-3.2332-2.11573-5.06783-2.11573-1.83364 0-3.66831.70593-5.06683 2.11573-1.39955 1.41083-2.09984 3.25926-2.09984 5.10877 0 7.2244 7.16667 13.2241 14.3333 18.1088z"/></svg>
                        </button>
                    </div>
                </div>
                // Carousel: [prev] [image] [next]
                <div class="gallery-stage">
                    <div class="gallery-nav-slot">
                        <button class="gallery-nav-btn" data-gallery-prev type="button" aria-label="Previous" disabled>
                            <svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 32 32" aria-hidden="true" focusable="false" style="display:block;fill:none;height:12px;width:12px;stroke:currentcolor;stroke-width:4;overflow:visible"><path d="M20 28 8.7 16.7a1 1 0 0 1 0-1.4L20 4"/></svg>
                        </button>
                    </div>
                    <div class="gallery-image-wrap">
                        {AirImage {
                            id: Some("gallery-image"),
                            src: data.photos[0].src,
                            alt: data.photos[0].alt,
                            class: Some("gallery-image"),
                            loading: AirImageLoading::Eager,
                            fetch_priority: AirImageFetchPriority::Auto,
                            fallback_src: None,
                        }}
                    </div>
                    <div class="gallery-nav-slot">
                        <button class="gallery-nav-btn" data-gallery-next type="button" aria-label="Next">
                            <svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 32 32" aria-hidden="true" focusable="false" style="display:block;fill:none;height:12px;width:12px;stroke:currentcolor;stroke-width:4;overflow:visible"><path d="m12 4 11.3 11.3a1 1 0 0 1 0 1.4L12 28"/></svg>
                        </button>
                    </div>
                </div>
            </div>
        </div>

    }
    .render(cx)?;
    shared::render_footer(cx);
    if let Some(islands) = islands {
        // proa-perf-ok(scope): bootstrap is trusted pre-rendered React module markup
        html_sync! {
            {UnsafeRaw(&islands.bootstrap)}
        }
        .render(cx)?;
    }
    Ok(())
}

pub fn render_md() -> String {
    let data = ListingData::malibu();
    render_md_with(&data)
}

pub fn render_md_with(data: &ListingData<'_>) -> String {
    // proa-perf-ok: markdown showcase helper intentionally returns an owned String
    proa_core::render_to_string(md_sync! { r###"
        # {data.title} — {data.property_line}

        {data.rating} · {data.review_count} reviews · Guest favorite
        {data.overview_summary}

        ## About this place

        {data.description}

        ## Highlights

        {for highlight in data.highlights.iter() {
            md_sync! { "- **{highlight.title}** — {highlight.description}\n" }
        }}
        ## Amenities

        {for amenity in data.amenities.iter() {
            md_sync! { "- {amenity}\n" }
        }}
        ## Booking snapshot

        - ${data.nightly_rate} per night
        - Check-in: {data.booking.check_in_display}
        - Checkout: {data.booking.checkout_display}
        - Free cancellation before {data.booking.free_cancel_date}

        ## Things to know

        House rules:
        {for rule in data.house_rules.iter() {
            md_sync! { "- {rule}\n" }
        }}
        Safety & property:
        {for safety_item in data.safety_items.iter() {
            md_sync! { "- {safety_item}\n" }
        }}
        Cancellation policy:
        {for policy in data.cancellation_items.iter() {
            md_sync! { "- {policy}\n" }
        }}
        ## Host

        Hosted by {data.host.name} — Superhost · {data.host.years_hosting} years hosting · Response rate {data.host.response_rate} · Responds within {data.host.response_time}
        "###
    })
}
