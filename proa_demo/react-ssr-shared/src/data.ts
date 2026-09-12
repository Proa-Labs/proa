// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

// All static data transcribed from airbnb_data.rs + listing.rs::ListingData::malibu()

export interface Listing {
  title: string;
  location: string;
  pricePerNight: number;
  rating: string;
  reviewsCount: number;
  imagePlaceholder: string;
  href: string;
  propertyType: string;
  beds: number;
  baths: number;
}

export const LISTINGS: Listing[] = [
  {
    title: "Malibu Beach House",
    location: "Malibu, California",
    pricePerNight: 350,
    rating: "4.92",
    reviewsCount: 128,
    imagePlaceholder: "bg-gradient-to-br from-sky-400 to-blue-600",
    href: "/blocks/airbnb_listing",
    propertyType: "Entire home",
    beds: 3,
    baths: 2,
  },
  {
    title: "Santa Monica Studio",
    location: "Santa Monica, California",
    pricePerNight: 185,
    rating: "4.87",
    reviewsCount: 94,
    imagePlaceholder: "bg-gradient-to-br from-amber-300 to-orange-500",
    href: "/blocks/airbnb_listing",
    propertyType: "Entire guest suite",
    beds: 1,
    baths: 1,
  },
  {
    title: "Pacific Palisades Villa",
    location: "Pacific Palisades, California",
    pricePerNight: 520,
    rating: "4.95",
    reviewsCount: 67,
    imagePlaceholder: "bg-gradient-to-br from-emerald-400 to-teal-600",
    href: "/blocks/airbnb_listing",
    propertyType: "Entire villa",
    beds: 5,
    baths: 4,
  },
  {
    title: "Topanga Canyon Cabin",
    location: "Topanga, California",
    pricePerNight: 210,
    rating: "4.89",
    reviewsCount: 156,
    imagePlaceholder: "bg-gradient-to-br from-green-500 to-emerald-700",
    href: "/blocks/airbnb_listing",
    propertyType: "Entire cabin",
    beds: 2,
    baths: 1,
  },
  {
    title: "Venice Beach Loft",
    location: "Venice, California",
    pricePerNight: 275,
    rating: "4.78",
    reviewsCount: 203,
    imagePlaceholder: "bg-gradient-to-br from-rose-400 to-pink-600",
    href: "/blocks/airbnb_listing",
    propertyType: "Entire loft",
    beds: 2,
    baths: 2,
  },
  {
    title: "Malibu Airstream Suite",
    location: "Malibu, California",
    pricePerNight: 295,
    rating: "4.80",
    reviewsCount: 247,
    imagePlaceholder: "bg-gradient-to-br from-violet-400 to-purple-600",
    href: "/blocks/airbnb_listing",
    propertyType: "Tiny home",
    beds: 1,
    baths: 1,
  },
];

// ---------------------------------------------------------------------------
// Listing detail data (from listing.rs::ListingData::malibu())
// ---------------------------------------------------------------------------

export interface ListingPhoto {
  src: string;
  alt: string;
}

export interface ListingHighlight {
  title: string;
  description: string;
}

export interface ListingReview {
  name: string;
  location: string;
  date: string;
  duration: string;
  body: string;
  avatar: string;
}

export interface ReviewScores {
  cleanliness: string;
  accuracy: string;
  checkIn: string;
  communication: string;
  location: string;
  value: string;
}

export interface HostInfo {
  name: string;
  photo: string;
  reviewCount: number;
  yearsHosting: number;
  responseRate: string;
  responseTime: string;
  bio: string;
}

export interface CohostInfo {
  name: string;
  photo: string;
}

export interface BookingDefaults {
  checkIn: string;
  checkout: string;
  checkInDisplay: string;
  checkoutDisplay: string;
  rangeLabel: string;
  guests: string;
  freeCancelDate: string;
  nights: number;
}

export interface ListingData {
  title: string;
  location: string;
  locationShort: string;
  propertyLine: string;
  rating: string;
  reviewCount: number;
  overviewSummary: string;
  description: string;
  nightlyRate: number;
  cleaningFee: number;
  serviceFee: number;
  totalAmenityCount: number;
  photos: ListingPhoto[];
  highlights: ListingHighlight[];
  amenities: string[];
  reviewDistribution: [string, number][];
  reviewScores: ReviewScores;
  reviews: ListingReview[];
  houseRules: string[];
  safetyItems: string[];
  cancellationItems: string[];
  nearbyLinks: string[];
  host: HostInfo;
  cohost: CohostInfo | null;
  booking: BookingDefaults;
}

export const MALIBU_LISTING: ListingData = {
  title: "Breathtaking Malibu Ocean View Sanctuary",
  location: "Malibu, California, United States",
  locationShort: "Malibu",
  propertyLine: "Entire home in Malibu, California",
  rating: "4.92",
  reviewCount: 75,
  overviewSummary: "6 guests \u00b7 3 bedrooms \u00b7 3 beds \u00b7 3.5 baths",
  description:
    "Experience panoramic ocean and canyon views at this one-of-a-kind Malibu home, overlooking one of the most pristine canyons in Malibu.\n\nThis stunning 3 BD / 3.5 BA home was recently renovated into a modern sanctuary to reset in nature while staying close to the coast. The architecture frames sunrise, marine layer, and sunset with the kind of calm that makes you slow down immediately.\n\nFurniture may differ slightly from older photos, but the overall atmosphere remains a warm modern retreat with strong indoor-outdoor flow.",
  nightlyRate: 1042,
  cleaningFee: 375,
  serviceFee: 545,
  totalAmenityCount: 46,
  photos: [
    { src: "https://a0.muscache.com/im/pictures/miso/Hosting-53344025/original/8db0ac0c-2124-4588-8331-e3d7b619bc0d.jpeg", alt: "Main living room with glass walls and ocean view" },
    { src: "https://a0.muscache.com/im/pictures/miso/Hosting-53344025/original/10e86c22-a721-4299-82fd-b1787f71ea22.jpeg", alt: "Terrace overlooking canyon and ocean" },
    { src: "https://a0.muscache.com/im/pictures/miso/Hosting-53344025/original/cd18a7b0-de22-453b-a61a-0a641a44833f.jpeg", alt: "Warm-toned sitting area with canyon light" },
    { src: "https://a0.muscache.com/im/pictures/miso/Hosting-53344025/original/52635eec-5de9-443c-acf9-04a4177061b0.jpeg", alt: "Bedroom with sliding doors to the deck" },
    { src: "https://a0.muscache.com/im/pictures/miso/Hosting-53344025/original/d09a38d0-28a7-4c4f-a142-55c93a0047ee.jpeg", alt: "Kitchen and dining area framed by large windows" },
    { src: "https://a0.muscache.com/im/pictures/miso/Hosting-53344025/original/a0b82a02-af0c-4c95-a8d9-8f7c8eea0604.jpeg", alt: "Outdoor dining setup with canyon backdrop" },
    { src: "https://a0.muscache.com/im/pictures/miso/Hosting-53344025/original/68840de4-241a-4e9e-bbeb-9b193bd6a213.jpeg", alt: "Bathroom with sculptural tub and warm light" },
    { src: "https://a0.muscache.com/im/pictures/miso/Hosting-53344025/original/c893e5ee-7f1f-40c9-8e86-04dc49bf4919.jpeg", alt: "Exterior showing the house perched above the canyon" },
  ],
  highlights: [
    { title: "Exceptional check-in experience", description: "Recent guests gave the check-in process a 5-star rating." },
    { title: "Beautiful area", description: "Guests love this home's scenic location and sense of privacy." },
    { title: "Ocean and mountain views", description: "Guests consistently call out the canyon light and coastal panorama." },
  ],
  amenities: [
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
  ],
  reviewDistribution: [["5", 96], ["4", 1], ["3", 1], ["2", 1], ["1", 0]],
  reviewScores: {
    cleanliness: "4.9",
    accuracy: "4.9",
    checkIn: "5.0",
    communication: "5.0",
    location: "4.9",
    value: "4.8",
  },
  reviews: [
    { name: "Jessy", location: "San Francisco, California", date: "DATE", duration: "Stayed a few nights", body: "Thanks so much for a lovely visit. We so enjoyed the gorgeous view, the fire pit at sunset and the great views from inside too. There were just 3 of us so there was lots of room to spread out. The kitchen was completely stocked which was a really nice bonus since we like to cook. Will definitely come again!", avatar: "https://a0.muscache.com/im/pictures/user/c51d580a-c2f8-4587-ab61-b32ef41db967.jpg?im_w=240" },
    { name: "Calvin", location: "New York, New York", date: "DATE", duration: "Stayed about a week", body: "Incredible views. Really pleasant and helpful owner. The house is something out of the ordinary, stepping down from the cliff with cozy balconies and multiple places to work or relax. We're coming back tomorrow. It is quite the undiscovered gem.", avatar: "https://a0.muscache.com/im/pictures/user/User/original/efb490bc-e855-4b32-b400-5f72c01947d7.jpeg?im_w=240" },
    { name: "Adriana", location: "Virginia Beach, Virginia", date: "DATE", duration: "Stayed a few nights", body: "The hosts could not have made us feel more at home, helpful and friendly while still making it feel like our own place. The home is beautiful, spacious, and the outdoor rooms are perfect in every way.", avatar: "https://a0.muscache.com/im/pictures/user/ff1e1bb2-c4ce-46d1-ade9-522a4d96a091.jpg?im_w=240" },
    { name: "Tony", location: "Orlando, Florida", date: "DATE", duration: "Stayed a few nights", body: "The home exceeded our expectations. The beautiful and relaxing views made it a perfect place for peace and quiet. Matt, our host, was incredibly helpful throughout our stay. Highly recommend.", avatar: "https://a0.muscache.com/im/pictures/user/User/original/090131e8-e05f-422d-a8ba-aec2e630093c.jpeg?im_w=240" },
  ],
  houseRules: [
    "Check-in after 4:00 PM",
    "Checkout before 11:00 AM",
    "6 guests maximum",
    "No parties or events",
  ],
  safetyItems: [
    "Exterior security cameras on property",
    "Smoke alarm",
    "Carbon monoxide alarm",
    "Free parking on premises",
  ],
  cancellationItems: [
    "Free cancellation before March 13.",
    "Cancel before check-in on March 18 for a partial refund.",
    "Review the full cancellation policy before you book.",
  ],
  nearbyLinks: [
    "Southern California vacation rentals",
    "Los Angeles vacation rentals",
    "Palm Springs vacation rentals",
    "San Diego vacation rentals",
    "Malibu monthly stays",
    "House vacation rentals in Malibu",
    "Beach house vacation rentals in California",
    "House vacation rentals in United States",
  ],
  host: {
    name: "Darya",
    photo: "https://a0.muscache.com/im/pictures/user/387255af-8adf-439b-87ee-4a5afaf7bd7c.jpg",
    reviewCount: 76,
    yearsHosting: 7,
    responseRate: "100%",
    responseTime: "1 hr",
    bio: "Superhosts are experienced, highly rated hosts who are committed to providing great stays for guests.",
  },
  cohost: {
    name: "Matt",
    photo: "https://a0.muscache.com/im/users/3378233/profile_pic/1346002785/original.jpg?aki_policy=profile_x_medium",
  },
  booking: {
    checkIn: "DATE",
    checkout: "DATE",
    checkInDisplay: "DATE",
    checkoutDisplay: "DATE",
    rangeLabel: "DATE - DATE",
    guests: "2 guests",
    freeCancelDate: "March 13",
    nights: 7,
  },
};

// Home page section data
export const SEARCH_IMAGES: string[] = [
  "/images/airbnb/bedroom.jpg",
  "/images/airbnb/deck-wine.jpg",
  "/images/airbnb/exterior.jpg",
  "/images/airbnb/ocean-view.jpg",
  "/images/airbnb/deck.jpg",
  "/images/airbnb/bedroom.jpg",
];

export const HOME_IMAGES: string[] = [
  "/images/airbnb/bedroom.jpg",
  "/images/airbnb/deck-wine.jpg",
  "/images/airbnb/exterior.jpg",
  "/images/airbnb/ocean-view.jpg",
  "/images/airbnb/deck.jpg",
  "/images/airbnb/bedroom.jpg",
  "/images/airbnb/exterior.jpg",
  "/images/airbnb/deck.jpg",
];

export const FILTERS: string[] = [
  "Price",
  "Type of place",
  "Rooms and beds",
  "Amenities",
  "Book instantly",
];

export const HOUSE_POSTER =
  "/images/airbnb/external/4aae4ed7-5939-4e76-b100-e69440ebeae4.png";
export const BALLOON_POSTER =
  "/images/airbnb/external/search-icon-1.png";
export const SERVICE_POSTER = "/images/airbnb/external/search-icon-2.png";

export const HOMES_SECTIONS: [string, string, number[]][] = [
  ["Popular homes in Los Angeles", "", [0, 1, 2, 3, 4, 5, 0]],
  ["Featured hotels in Los Angeles", "A collection of independent and handpicked hotels", [2, 4, 1, 5, 3, 0, 2]],
  ["Stay by the beach in Malibu", "Ocean-facing stays with relaxed design details", [5, 0, 1, 4, 2, 3, 5]],
];

export const EXPERIENCES_SECTIONS: [string, string, number[]][] = [
  ["Book experiences led by locals", "Design-minded activities around Los Angeles", [1, 3, 5, 0, 2, 4, 1]],
  ["Food, movement, and culture", "Small-group experiences with polished presentation", [4, 2, 0, 5, 1, 3, 4]],
];

export const SERVICES_SECTIONS: [string, string, number[]][] = [
  ["Services for your stay", "Private chefs, photography, and wellness add-ons", [2, 5, 3, 1, 4, 0, 2]],
  ["Top-rated service pros nearby", "Curated services that feel native to the Airbnb flow", [3, 0, 4, 1, 5, 2, 3]],
];

export const DEFAULT_FALLBACK_SRC = "data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 1200 900'%3E%3Cdefs%3E%3ClinearGradient id='g' x1='0' y1='0' x2='1' y2='1'%3E%3Cstop stop-color='%23f6f6f6'/%3E%3Cstop offset='1' stop-color='%23ececec'/%3E%3C/linearGradient%3E%3C/defs%3E%3Crect width='1200' height='900' fill='url(%23g)'/%3E%3Ccircle cx='910' cy='220' r='160' fill='%23ffffff' fill-opacity='.58'/%3E%3Ccircle cx='280' cy='640' r='220' fill='%23ffffff' fill-opacity='.44'/%3E%3Cpath d='M0 685c113-43 217-70 312-81 168-20 283 16 400 1 95-12 201-51 488-177v472H0z' fill='%23e3e3e3'/%3E%3Cpath d='M0 585c154-68 301-97 430-83 130 15 224 57 331 54 92-2 211-39 439-150v494H0z' fill='%23d8d8d8'/%3E%3Ctext x='50%25' y='50%25' text-anchor='middle' dominant-baseline='middle' fill='%23717171' font-family='system-ui,-apple-system,sans-serif' font-size='42'%3EImage unavailable%3C/text%3E%3C/svg%3E";
