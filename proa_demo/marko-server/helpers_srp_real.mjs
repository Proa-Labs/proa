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

export const DEFAULT_QUERY = "iphone 14 pro max";
