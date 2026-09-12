// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

// Mirror dataset + helpers for page_srp_real.marko.

export const TITLES = [
  "Unlocked Apple iPhone 12 Pro Max - 128GB Gold - 78% Batt - Crcks on Front & Back",
  "Apple iPhone 14 Pro Max 256GB - Deep Purple - Unlocked - <Excellent Condition>",
  "Apple iPhone 14 Pro 128GB - Space Black - Verizon - Open Box & Tested",
  "Apple iPhone 14 Pro Max 1TB - Silver - AT&T Locked - Battery 92%",
  "Apple iPhone 14 Pro 512GB - Gold - T-Mobile Unlocked - Pristine",
  "Apple iPhone 14 Pro Max 256GB - Deep Purple - Sprint Locked - Used",
  "Apple iPhone 14 Pro 128GB - Space Black - Factory Unlocked Dual SIM",
  "Apple iPhone 14 Pro Max 512GB - Silver - Cricket Locked - Pre-Owned",
  "Apple iPhone 14 Pro 256GB - Deep Purple - Verizon - <Brand New & Sealed>",
  "Apple iPhone 14 Pro Max 128GB - Gold - Visible Locked - 89% Battery",
  'Apple iPhone 14 Pro 1TB - Space Black - Unlocked - "Like New" Condition',
  "Apple iPhone 14 Pro Max 256GB - Silver - Boost Mobile - Excellent",
  "Apple iPhone 14 Pro 128GB - Deep Purple - Metro by T-Mobile - 7/10",
  "Apple iPhone 14 Pro Max 512GB - Gold - Mint Mobile - Pre-Owned",
  "Apple iPhone 14 Pro 256GB - Space Black & Cracked Back - For Parts",
  "Apple iPhone 14 Pro Max 128GB - Silver - US Cellular Locked",
];

const SUBTITLE_VARIANTS = [
  ["Pre-Owned", "Apple iPhone 14 Pro Max", "128 GB", "Unlocked"],
  ["Brand New", "Apple iPhone 14 Pro", "256 GB", "Verizon"],
  ["Pre-Owned", "Apple iPhone 14 Pro", "512 GB", "AT&T"],
  ["Open Box", "Apple iPhone 14 Pro Max", "1 TB", "T-Mobile"],
  ["For parts or not working", "Apple iPhone 14 Pro", "Unlocked"],
  ["Refurbished", "Apple iPhone 14 Pro Max", "256 GB", "Sprint"],
  ["Pre-Owned", "Apple iPhone 14 Pro", "128 GB", "Cricket"],
  ["Brand New", "Apple iPhone 14 Pro Max", "512 GB", "Boost Mobile"],
];

const SELLERS = [
  "cocosprinkles",
  "tech_outlet_usa",
  "phonefactory_uk",
  "smart-deals & co",
  "best>buy_outlet",
  "the.gadget_shop",
  'global-tech"vendor"',
  "premium_resale",
  "digital_drop&co",
  "pacific_electronics",
];

const LOCATIONS = [
  "United States",
  "Hong Kong",
  "China",
  "United Kingdom",
  "Germany",
  "Japan",
  "Australia",
  "Canada",
];

const IMAGE_HASHES = [
  "C1EAAeSw3bpp1CD9",
  "DkAAAOSw9oZl5kZ4",
  "K2EAAOSwUVZmJ8x9",
  "M5EAAOSwx9plQ7Wq",
  "B3EAAOSw7P5kJ2x1",
  "G4EAAOSwY8plPwBk",
  "H6EAAOSw0XJlNVwR",
  "P7EAAOSwm5RkV2nT",
];

const FORMATS = ["Buy It Now", "Buy It Now", "Buy It Now", "or Best Offer", "Best Offer"];

export const CAPTION_LABELS = {
  NewLowPrice: "NEW LOW PRICE",
  Trending: "TRENDING",
  EndsSoon: "ENDS SOON",
  EbayChoice: "eBay's CHOICE",
};
export const CAPTION_CLASSES = {
  NewLowPrice: "signal signal--trustworthy",
  Trending: "signal signal--informational",
  EndsSoon: "signal signal--negative",
  EbayChoice: "signal signal--brand",
};

export function buildListings(n) {
  const arr = new Array(n);
  for (let i = 0; i < n; i++) {
    const base = 14999 + ((i * 4337) % 195000);
    const onSale = i % 3 === 0;
    const free = i % 2 === 0;
    let caption = null;
    switch (i % 9) {
      case 0: caption = "NewLowPrice"; break;
      case 3: caption = "Trending"; break;
      case 5: caption = "EndsSoon"; break;
      case 7: caption = "EbayChoice"; break;
    }
    arr[i] = {
      itemId: 336000000000 + i * 71,
      imageHash: IMAGE_HASHES[i % IMAGE_HASHES.length],
      title: TITLES[i % TITLES.length],
      subtitleParts: SUBTITLE_VARIANTS[i % SUBTITLE_VARIANTS.length],
      caption,
      starsFull: 3 + (i % 3),
      hasHalfStar: i % 2 === 0,
      reviewCount: 5 + ((i * 17) % 1240),
      priceCents: base,
      originalPriceCents: onSale ? base + Math.floor(base / 5) : null,
      format: FORMATS[i % 4 === 0 ? 3 : (i % 4 === 1 ? 4 : 0)],
      deliveryCents: free ? 0 : 599 + ((i * 13) % 1400),
      location: LOCATIONS[i % LOCATIONS.length],
      watcherCount: 5 + ((i * 7) % 200),
      couponPct: i % 4 === 1 ? 5 + (i % 20) : null,
      sellerName: SELLERS[i % SELLERS.length],
      sellerFeedbackPctX10: 950 + ((i * 7) % 50),
      sellerFeedbackCount: 1000 + ((i * 137) % 99000),
    };
  }
  return arr;
}

export function fmtCurrency(cents) {
  const neg = cents < 0;
  const abs = Math.abs(cents);
  const dollars = Math.floor(abs / 100);
  const pennies = abs % 100;
  const dStr = dollars.toString().replace(/\B(?=(\d{3})+(?!\d))/g, ",");
  const pStr = pennies < 10 ? "0" + pennies : "" + pennies;
  return (neg ? "-$" : "$") + dStr + "." + pStr;
}

export function fmtCount(n) {
  return n.toString().replace(/\B(?=(\d{3})+(?!\d))/g, ",");
}

export function fmtFeedbackPct(x10) {
  const whole = Math.floor(x10 / 10);
  const tenth = x10 % 10;
  return whole + "." + tenth + "%";
}

export function fmtAbbrev(n) {
  if (n < 1000) return String(n);
  if (n < 1_000_000) {
    const whole = Math.floor(n / 1000);
    const tenth = Math.floor((n % 1000) / 100);
    return tenth > 0 ? whole + "." + tenth + "K" : whole + "K";
  }
  const whole = Math.floor(n / 1_000_000);
  const tenth = Math.floor((n % 1_000_000) / 100_000);
  return tenth > 0 ? whole + "." + tenth + "M" : whole + "M";
}

export function itemUrl(id) {
  return "https://www.ebay.com/itm/" + id;
}

export function emptyStarCount(starsFull, hasHalfStar) {
  return Math.max(0, 5 - starsFull - (hasHalfStar ? 1 : 0));
}

export function repeatArray(n) {
  // Marko 6 doesn't have a repeat-N tag; emit an array we can iterate.
  return new Array(Math.max(0, n));
}

// Mirrors `ListingFormat::slug()` in ebay_srp.rs: the per-card `data-format`
// attribute the URL-driven filter CSS matches against.
export function formatSlug(label) {
  if (label === "or Best Offer") return "auction";
  if (label === "Best Offer") return "bestoffer";
  return "buyitnow";
}

// ---------------------------------------------------------------------------
// Fully dynamic SRP chrome: shared constant tables + per-request refinement
// metadata. Single JS source of truth, consumed by react-ssr/chrome.tsx (and
// through it next-ssr) and page_srp_real.marko. Mirrored by
// proa_showcase/src/ebay/srp_chrome.rs; keep the tables and the derivation
// arithmetic in sync with the Rust side.
// ---------------------------------------------------------------------------

export const DEFAULT_QUERY = "iphone 14 pro max";
export const PER_PAGE_OPTIONS = [60, 120, 240];

export const TOPBAR_LEFT = [
  ["Hi! Sign in", "#signin"],
  ["Daily Deals", "#deals"],
  ["Brand Outlet", "#outlet"],
  ["Gift Cards", "#gift"],
  ["Help & Contact", "#help"],
];
export const TOPBAR_RIGHT = [
  ["Ship to", "#ship"],
  ["Sell", "#sell"],
  ["Watchlist", "#watchlist"],
  ["My eBay", "#myebay"],
];
export const SEARCH_CATS = [
  "All Categories",
  "Cell Phones & Accessories",
  "Consumer Electronics",
  "Computers, Tablets & Network",
  "Cameras & Photo",
  "Video Games & Consoles",
  "Clothing, Shoes & Accessories",
  "Home & Garden",
  "Sporting Goods",
];
export const NAV_CATS = [
  ["eBay Live", "#live"],
  ["Saved", "#saved"],
  ["Electronics", "#electronics"],
  ["Motors", "#motors"],
  ["Fashion", "#fashion"],
  ["Collectibles and Art", "#collectibles"],
  ["Sports", "#sports"],
  ["Health & Beauty", "#health"],
  ["Industrial equipment", "#industrial"],
  ["Home & Garden", "#home"],
  ["Deals", "#deals-nav"],
  ["Sell", "#sell-nav"],
];
export const MEGANAV = [
  ["Top Categories", ["Cell Phones & Smartphones", "Laptops & Netbooks", "Watches", "Sneakers", "Trading Cards", "Video Games", "Handbags", "Power Tools"]],
  ["Electronics", ["Smartphones", "Smart Watches", "Tablets & eReaders", "Headphones", "Drones", "Cameras", "TV & Home Audio", "Computer Components"]],
  ["Fashion", ["Men's Sneakers", "Women's Handbags", "Luxury Watches", "Sunglasses", "Fine Jewelry", "Vintage Clothing", "Designer Shoes", "Streetwear"]],
  ["Collectibles", ["Pokémon Cards", "Sports Trading Cards", "Coins & Money", "Comic Books", "Action Figures", "Stamps", "Militaria", "Antiques"]],
  ["Home & Garden", ["Major Appliances", "Power Tools", "Outdoor Living", "Kitchen & Dining", "Furniture", "Smart Home", "Lighting", "Yard & Garden"]],
  ["Motors", ["Cars & Trucks", "Motorcycles", "Parts & Accessories", "Wheels & Tires", "Engines", "Exterior Parts", "Performance Parts", "Automotive Tools"]],
];
export const RAIL_CATS = [
  ["Cell Phones & Smartphones", 97],
  ["Cell Phone Cases, Covers & Skins", 41],
  ["Cell Phone Accessories", 29],
  ["Smart Watches", 17],
  ["Cell Phone & Smartphone Parts", 13],
  ["Tablets & eBook Readers", 7],
  ["Cell Phone Screen Protectors", 5],
  ["Other Cell Phones & Accessories", 3],
];
export const RELATED_SEARCHES = [
  "iphone 13 pro max",
  "iphone 15 pro max",
  "iphone 14 pro max unlocked",
  "iphone 14 pro max 256gb",
  "samsung galaxy s24 ultra",
  "iphone 14 plus",
  "iphone 14 pro",
  "iphone 14 pro max for parts",
];
export const FOOTER_COLS = [
  ["Buy", ["Registration", "Bidding & buying help", "Stores", "eBay for Charity", "Charity Shop", "Seasonal sales and events", "eBay Gift Cards"]],
  ["Sell", ["Start selling", "How to sell", "Business sellers", "Affiliates", "Site map", "eBay Live", "Security center"]],
  ["Tools & apps", ["Developers", "eBay official app", "Free shipping zone", "Money back guarantee", "Shipping labels", "Site preferences", "Accessibility"]],
  ["eBay companies", ["eBay Classifieds", "eBay Kleinanzeigen", "StubHub", "TCGplayer", "Careers", "Company news", "Investors"]],
  ["Stay connected", ["eBay's Blogs", "Facebook", "X (Twitter)", "Instagram", "YouTube", "Press room", "Policies"]],
];
export const FOOTER_LEGAL = [
  "Accessibility",
  "User Agreement",
  "Privacy",
  "Consumer Health Data",
  "Payments Terms of Use",
  "Cookies",
  "CA Privacy Notice",
  "Your Privacy Choices",
  "AdChoice",
];
export const SORT_OPTIONS = [
  "Best Match",
  "Time: ending soonest",
  "Time: newly listed",
  "Price + Shipping: lowest first",
  "Price + Shipping: highest first",
  "Distance: nearest first",
];
export const CONDITION_BUCKETS = [
  ["Brand New", "New"],
  ["Open Box", "Open box"],
  ["Pre-Owned", "Pre-owned"],
  ["Refurbished", "Certified - Refurbished"],
  ["For parts or not working", "For parts or not working"],
];
export const LOCATION_ORDER = [
  "United States",
  "Hong Kong",
  "China",
  "United Kingdom",
  "Germany",
  "Japan",
  "Australia",
  "Canada",
];

export function deriveSrpMeta(listings) {
  const n = listings.length;
  let buyItNow = 0;
  let auction = 0;
  let bestOffer = 0;
  let freeDelivery = 0;
  let coupons = 0;
  const priceBuckets = [0, 0, 0, 0];
  const condByFirst = new Map();
  const locCounts = new Map();
  const ratingThresholds = [0, 0, 0];

  for (const l of listings) {
    if (l.format === "or Best Offer") auction += 1;
    else if (l.format === "Best Offer") bestOffer += 1;
    else buyItNow += 1;
    if (l.deliveryCents === 0) freeDelivery += 1;
    if (l.couponPct != null) coupons += 1;
    if (l.priceCents < 50_000) priceBuckets[0] += 1;
    else if (l.priceCents < 100_000) priceBuckets[1] += 1;
    else if (l.priceCents < 150_000) priceBuckets[2] += 1;
    else priceBuckets[3] += 1;
    const first = l.subtitleParts?.[0] ?? "";
    condByFirst.set(first, (condByFirst.get(first) ?? 0) + 1);
    locCounts.set(l.location, (locCounts.get(l.location) ?? 0) + 1);
    const pct = l.sellerFeedbackPctX10 ?? 0;
    if (pct >= 990) ratingThresholds[0] += 1;
    if (pct >= 980) ratingThresholds[1] += 1;
    if (pct >= 950) ratingThresholds[2] += 1;
  }

  return {
    total: 117_406 + n * 7,
    cartCount: coupons,
    conditionCounts: CONDITION_BUCKETS.map(([key, label]) => [label, condByFirst.get(key) ?? 0]),
    formatCounts: [
      ["All Listings", n],
      ["Buy It Now", buyItNow],
      ["Auction", auction],
      ["Best Offer", bestOffer],
    ],
    priceCounts: [
      ["Under $500.00", priceBuckets[0]],
      ["$500.00 to $1,000.00", priceBuckets[1]],
      ["$1,000.00 to $1,500.00", priceBuckets[2]],
      ["Over $1,500.00", priceBuckets[3]],
    ],
    deliveryCounts: [
      ["Free delivery", freeDelivery],
      ["Paid delivery", n - freeDelivery],
    ],
    locationCounts: LOCATION_ORDER.map((loc) => [loc, locCounts.get(loc) ?? 0]),
    ratingCounts: [
      ["99% positive or more", ratingThresholds[0]],
      ["98% positive or more", ratingThresholds[1]],
      ["95% positive or more", ratingThresholds[2]],
    ],
  };
}
