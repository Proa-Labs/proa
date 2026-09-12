// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Owned, serde-deserializable versions of the Airbnb listing data types.
//!
//! These mirror `proa_showcase::airbnb::listing::*` but use
//! `String` instead of `&str` so they can be deserialized from JSON.
#![allow(dead_code)]

use proa_showcase::airbnb::home::{HomeData, HomeListing, HomeSection};
use proa_showcase::airbnb::listing::{
    BookingDefaults, CohostInfo, HostInfo, ListingData, ListingHighlight, ListingPhoto,
    ListingReview, ReviewScores,
};
use proa_showcase::airbnb::search::{SearchData, SearchListing};
use proa_showcase::nike::catalog::Product;
use proa_showcase::nike::product::{
    Benefit, Colorway, ProductData, RecommendedProduct, Review, Size,
};
use serde::Deserialize;

#[derive(Deserialize)]
pub struct ListingDataOwned {
    pub title: String,
    pub location: String,
    pub location_short: String,
    pub property_line: String,
    pub rating: String,
    pub review_count: u32,
    pub overview_summary: String,
    pub description: String,
    pub nightly_rate: u32,
    pub cleaning_fee: u32,
    pub service_fee: u32,
    pub total_amenity_count: u32,
    pub photos: Vec<ListingPhotoOwned>,
    pub highlights: Vec<ListingHighlightOwned>,
    pub amenities: Vec<String>,
    pub review_distribution: Vec<(String, u8)>,
    pub review_scores: ReviewScoresOwned,
    pub reviews: Vec<ListingReviewOwned>,
    pub house_rules: Vec<String>,
    pub safety_items: Vec<String>,
    pub cancellation_items: Vec<String>,
    pub nearby_links: Vec<String>,
    pub host: HostInfoOwned,
    pub cohost: Option<CohostInfoOwned>,
    pub booking: BookingDefaultsOwned,
}

#[derive(Deserialize)]
pub struct ListingPhotoOwned {
    pub src: String,
    pub alt: String,
}

#[derive(Deserialize)]
pub struct ListingHighlightOwned {
    pub title: String,
    pub description: String,
}

#[derive(Deserialize)]
pub struct ListingReviewOwned {
    pub name: String,
    pub location: String,
    pub date: String,
    pub duration: String,
    pub body: String,
    pub avatar: String,
}

#[derive(Deserialize)]
pub struct ReviewScoresOwned {
    pub cleanliness: String,
    pub accuracy: String,
    pub check_in: String,
    pub communication: String,
    pub location: String,
    pub value: String,
}

#[derive(Deserialize)]
pub struct HostInfoOwned {
    pub name: String,
    pub photo: String,
    pub review_count: u32,
    pub years_hosting: u32,
    pub response_rate: String,
    pub response_time: String,
    pub bio: String,
}

#[derive(Deserialize)]
pub struct CohostInfoOwned {
    pub name: String,
    pub photo: String,
}

#[derive(Deserialize)]
pub struct BookingDefaultsOwned {
    pub check_in: String,
    pub checkout: String,
    pub check_in_display: String,
    pub checkout_display: String,
    pub range_label: String,
    pub guests: String,
    pub free_cancel_date: String,
    pub nights: u32,
}

/// Intermediate struct that holds borrowed slices for the lifetime of an owned ListingDataOwned.
/// Created by `borrow_from` — the caller must keep the owned data alive.
pub struct BorrowedListing<'a> {
    pub photos: Vec<ListingPhoto<'a>>,
    pub highlights: Vec<ListingHighlight<'a>>,
    pub amenities: Vec<&'a str>,
    pub review_distribution: Vec<(&'a str, u8)>,
    pub reviews: Vec<ListingReview<'a>>,
    pub house_rules: Vec<&'a str>,
    pub safety_items: Vec<&'a str>,
    pub cancellation_items: Vec<&'a str>,
    pub nearby_links: Vec<&'a str>,
}

impl ListingDataOwned {
    /// Build the intermediate borrowed collections from the owned data.
    pub fn borrow_collections(&self) -> BorrowedListing<'_> {
        BorrowedListing {
            photos: self
                .photos
                .iter()
                .map(|p| ListingPhoto {
                    src: &p.src,
                    alt: &p.alt,
                })
                .collect(),
            highlights: self
                .highlights
                .iter()
                .map(|h| ListingHighlight {
                    title: &h.title,
                    description: &h.description,
                })
                .collect(),
            amenities: self.amenities.iter().map(|s| s.as_str()).collect(),
            review_distribution: self
                .review_distribution
                .iter()
                .map(|(s, v)| (s.as_str(), *v))
                .collect(),
            reviews: self
                .reviews
                .iter()
                .map(|r| ListingReview {
                    name: &r.name,
                    location: &r.location,
                    date: &r.date,
                    duration: &r.duration,
                    body: &r.body,
                    avatar: &r.avatar,
                })
                .collect(),
            house_rules: self.house_rules.iter().map(|s| s.as_str()).collect(),
            safety_items: self.safety_items.iter().map(|s| s.as_str()).collect(),
            cancellation_items: self.cancellation_items.iter().map(|s| s.as_str()).collect(),
            nearby_links: self.nearby_links.iter().map(|s| s.as_str()).collect(),
        }
    }

    /// Build a `ListingData` referencing both `self` and the borrowed collections.
    #[allow(clippy::wrong_self_convention)]
    pub fn to_listing_data<'a>(&'a self, b: &'a BorrowedListing<'a>) -> ListingData<'a> {
        ListingData {
            title: &self.title,
            location: &self.location,
            location_short: &self.location_short,
            property_line: &self.property_line,
            rating: &self.rating,
            review_count: self.review_count,
            overview_summary: &self.overview_summary,
            description: &self.description,
            nightly_rate: self.nightly_rate,
            cleaning_fee: self.cleaning_fee,
            service_fee: self.service_fee,
            total_amenity_count: self.total_amenity_count,
            photos: &b.photos,
            highlights: &b.highlights,
            amenities: &b.amenities,
            review_distribution: &b.review_distribution,
            review_scores: ReviewScores {
                cleanliness: &self.review_scores.cleanliness,
                accuracy: &self.review_scores.accuracy,
                check_in: &self.review_scores.check_in,
                communication: &self.review_scores.communication,
                location: &self.review_scores.location,
                value: &self.review_scores.value,
            },
            reviews: &b.reviews,
            house_rules: &b.house_rules,
            safety_items: &b.safety_items,
            cancellation_items: &b.cancellation_items,
            nearby_links: &b.nearby_links,
            host: HostInfo {
                name: &self.host.name,
                photo: &self.host.photo,
                review_count: self.host.review_count,
                years_hosting: self.host.years_hosting,
                response_rate: &self.host.response_rate,
                response_time: &self.host.response_time,
                bio: &self.host.bio,
            },
            cohost: self.cohost.as_ref().map(|c| CohostInfo {
                name: &c.name,
                photo: &c.photo,
            }),
            booking: BookingDefaults {
                check_in: &self.booking.check_in,
                checkout: &self.booking.checkout,
                check_in_display: &self.booking.check_in_display,
                checkout_display: &self.booking.checkout_display,
                range_label: &self.booking.range_label,
                guests: &self.booking.guests,
                free_cancel_date: &self.booking.free_cancel_date,
                nights: self.booking.nights,
            },
        }
    }
}

// ---------------------------------------------------------------------------
// Search page owned types
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct SearchListingOwned {
    pub title: String,
    pub location: String,
    pub price_per_night: u32,
    pub rating: String,
    pub reviews_count: u32,
    pub property_type: String,
    pub beds: u8,
    pub baths: u8,
    pub image: String,
}

#[derive(Deserialize)]
pub struct SearchDataOwned {
    pub title: String,
    pub subtitle: String,
    pub stay_info: String,
    pub filters: Vec<String>,
    pub listings: Vec<SearchListingOwned>,
}

pub struct BorrowedSearch<'a> {
    pub listings: Vec<SearchListing<'a>>,
    pub filters: Vec<&'a str>,
}

impl SearchDataOwned {
    pub fn borrow_collections(&self) -> BorrowedSearch<'_> {
        BorrowedSearch {
            listings: self
                .listings
                .iter()
                .map(|l| SearchListing {
                    title: &l.title,
                    location: &l.location,
                    price_per_night: l.price_per_night,
                    rating: &l.rating,
                    reviews_count: l.reviews_count,
                    property_type: &l.property_type,
                    beds: l.beds,
                    baths: l.baths,
                    image: &l.image,
                })
                .collect(),
            filters: self.filters.iter().map(|s| s.as_str()).collect(),
        }
    }

    pub fn to_search_data<'a>(&'a self, b: &'a BorrowedSearch<'a>) -> SearchData<'a> {
        SearchData {
            title: &self.title,
            subtitle: &self.subtitle,
            stay_info: &self.stay_info,
            filters: &b.filters,
            listings: &b.listings,
        }
    }
}

// ---------------------------------------------------------------------------
// Home page owned types
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct HomeListingOwned {
    pub title: String,
    pub location: String,
    pub price_per_night: u32,
    pub rating: String,
    pub property_type: String,
    pub image: String,
}

#[derive(Deserialize)]
pub struct HomeSectionOwned {
    pub title: String,
    pub subtitle: String,
    pub listing_indexes: Vec<usize>,
}

#[derive(Deserialize)]
pub struct HomeDataOwned {
    pub listings: Vec<HomeListingOwned>,
    pub homes_sections: Vec<HomeSectionOwned>,
    pub experiences_sections: Vec<HomeSectionOwned>,
    pub services_sections: Vec<HomeSectionOwned>,
}

pub struct BorrowedHome<'a> {
    pub listings: Vec<HomeListing<'a>>,
    pub homes_sections: Vec<HomeSection<'a>>,
    pub experiences_sections: Vec<HomeSection<'a>>,
    pub services_sections: Vec<HomeSection<'a>>,
}

impl HomeDataOwned {
    pub fn borrow_collections(&self) -> BorrowedHome<'_> {
        BorrowedHome {
            listings: self
                .listings
                .iter()
                .map(|l| HomeListing {
                    title: &l.title,
                    location: &l.location,
                    price_per_night: l.price_per_night,
                    rating: &l.rating,
                    property_type: &l.property_type,
                    image: &l.image,
                })
                .collect(),
            homes_sections: self
                .homes_sections
                .iter()
                .map(|s| HomeSection {
                    title: &s.title,
                    subtitle: &s.subtitle,
                    listing_indexes: &s.listing_indexes,
                })
                .collect(),
            experiences_sections: self
                .experiences_sections
                .iter()
                .map(|s| HomeSection {
                    title: &s.title,
                    subtitle: &s.subtitle,
                    listing_indexes: &s.listing_indexes,
                })
                .collect(),
            services_sections: self
                .services_sections
                .iter()
                .map(|s| HomeSection {
                    title: &s.title,
                    subtitle: &s.subtitle,
                    listing_indexes: &s.listing_indexes,
                })
                .collect(),
        }
    }

    pub fn to_home_data<'a>(&'a self, b: &'a BorrowedHome<'a>) -> HomeData<'a> {
        HomeData {
            listings: &b.listings,
            homes_sections: &b.homes_sections,
            experiences_sections: &b.experiences_sections,
            services_sections: &b.services_sections,
        }
    }
}

// ---------------------------------------------------------------------------
// Nike catalog owned types
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct NikeProductOwned {
    pub title: String,
    pub subtitle: String,
    pub price: String,
    #[serde(default)]
    pub messaging: String,
    pub image: String,
    #[serde(default)]
    pub colors: Vec<String>,
    #[serde(default)]
    pub color_count: String,
}

#[derive(Deserialize)]
pub struct NikeCatalogDataOwned {
    #[serde(default = "default_nike_title")]
    pub title: String,
    pub products: Vec<NikeProductOwned>,
}

fn default_nike_title() -> String {
    "Men's Shoes & Sneakers".to_string()
}

pub struct BorrowedNikeCatalog<'a> {
    pub colors: Vec<Vec<&'a str>>,
}

impl NikeCatalogDataOwned {
    /// Borrow color strings from owned data. Caller keeps this alive.
    pub fn borrow_collections(&self) -> BorrowedNikeCatalog<'_> {
        BorrowedNikeCatalog {
            colors: self
                .products
                .iter()
                .map(|p| p.colors.iter().map(|s| s.as_str()).collect())
                .collect(),
        }
    }

    /// Build the full product slice. Caller must keep the returned Vec alive.
    pub fn build_products<'a>(&'a self, b: &'a BorrowedNikeCatalog<'a>) -> Vec<Product<'a>> {
        self.products
            .iter()
            .zip(b.colors.iter())
            .map(|(p, colors)| Product {
                title: &p.title,
                subtitle: &p.subtitle,
                price: &p.price,
                messaging: &p.messaging,
                image: &p.image,
                colors: colors.as_slice(),
                color_count: &p.color_count,
            })
            .collect()
    }
}

// ---------------------------------------------------------------------------
// Nike PDP owned types
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct NikePdpDataOwned {
    pub title: String,
    pub subtitle: String,
    pub current_price: String,
    pub original_price: String,
    pub discount_pct: String,
    pub color_name: String,
    pub style_code: String,
    pub description: String,
    pub reason_to_buy: String,
    pub weight: String,
    pub drop: String,
    pub avg_rating: String,
    pub review_count: String,
    pub benefits: Vec<BenefitOwned>,
    pub whats_new: Vec<String>,
    pub sizes: Vec<SizeOwned>,
    pub colorways: Vec<ColorwayOwned>,
    pub reviews: Vec<ReviewOwned>,
    pub recommended: Vec<RecommendedOwned>,
}

#[derive(Deserialize)]
pub struct BenefitOwned {
    pub title: String,
    pub description: String,
}

#[derive(Deserialize)]
pub struct SizeOwned {
    pub label: String,
    pub available: bool,
}

#[derive(Deserialize)]
pub struct ColorwayOwned {
    pub style_code: String,
    pub selected: bool,
}

#[derive(Deserialize)]
pub struct ReviewOwned {
    pub author: String,
    pub date: String,
    pub title: String,
    pub body: String,
    pub rating: u8,
}

#[derive(Deserialize)]
pub struct RecommendedOwned {
    pub title: String,
    pub subtitle: String,
    pub price: String,
    pub image: String,
}

pub struct BorrowedNikePdp<'a> {
    pub benefits: Vec<Benefit<'a>>,
    pub whats_new: Vec<&'a str>,
    pub sizes: Vec<Size<'a>>,
    pub colorways: Vec<Colorway<'a>>,
    pub reviews: Vec<Review<'a>>,
    pub recommended: Vec<RecommendedProduct<'a>>,
}

impl NikePdpDataOwned {
    pub fn borrow_collections(&self) -> BorrowedNikePdp<'_> {
        BorrowedNikePdp {
            benefits: self
                .benefits
                .iter()
                .map(|b| Benefit {
                    title: &b.title,
                    description: &b.description,
                })
                .collect(),
            whats_new: self.whats_new.iter().map(|s| s.as_str()).collect(),
            sizes: self
                .sizes
                .iter()
                .map(|s| Size {
                    label: &s.label,
                    available: s.available,
                })
                .collect(),
            colorways: self
                .colorways
                .iter()
                .map(|c| Colorway {
                    style_code: &c.style_code,
                    selected: c.selected,
                })
                .collect(),
            reviews: self
                .reviews
                .iter()
                .map(|r| Review {
                    author: &r.author,
                    date: &r.date,
                    title: &r.title,
                    body: &r.body,
                    rating: r.rating,
                })
                .collect(),
            recommended: self
                .recommended
                .iter()
                .map(|r| RecommendedProduct {
                    title: &r.title,
                    subtitle: &r.subtitle,
                    price: &r.price,
                    image: &r.image,
                })
                .collect(),
        }
    }

    pub fn to_product_data<'a>(&'a self, b: &'a BorrowedNikePdp<'a>) -> ProductData<'a> {
        ProductData {
            title: &self.title,
            subtitle: &self.subtitle,
            current_price: &self.current_price,
            original_price: &self.original_price,
            discount_pct: &self.discount_pct,
            color_name: &self.color_name,
            style_code: &self.style_code,
            description: &self.description,
            reason_to_buy: &self.reason_to_buy,
            weight: &self.weight,
            drop: &self.drop,
            avg_rating: &self.avg_rating,
            review_count: &self.review_count,
            benefits: &b.benefits,
            whats_new: &b.whats_new,
            sizes: &b.sizes,
            colorways: &b.colorways,
            reviews: &b.reviews,
            recommended: &b.recommended,
        }
    }
}
