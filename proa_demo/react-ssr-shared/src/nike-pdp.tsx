// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

// Nike Product Detail Page (PDP), React equivalent of the Proa Nike product showcase.
//
// Produces HTML matching the Proa server-rendered Nike Alphafly 3 product detail page.

import React from "react";
import { TopBar, Header, Footer } from "./nike-catalog";

// ---------------------------------------------------------------------------
// SVGs
// ---------------------------------------------------------------------------

function StarIcon({ filled }: { filled: boolean }) {
  return (
    <svg
      viewBox="0 0 24 24"
      width="16"
      height="16"
      fill={filled ? "#111" : "none"}
      {...(!filled ? { stroke: "#111", strokeWidth: "1.5" } : {})}
    >
      <path d="M12 1.5l3.09 6.26L22 8.77l-5 4.87 1.18 6.86L12 17.27 5.82 20.5 7 13.64l-5-4.87 6.91-1.01L12 1.5z" />
    </svg>
  );
}

function HeartOutlineIcon() {
  return (
    <svg aria-hidden="true" focusable="false" viewBox="0 0 24 24" width="24" height="24" fill="none">
      <path stroke="currentColor" strokeWidth="1.5" d="M16.794 3.75c1.324 0 2.568.516 3.504 1.451a4.96 4.96 0 010 7.008L12 20.508l-8.299-8.299a4.96 4.96 0 010-7.008A4.923 4.923 0 017.205 3.75c1.324 0 2.568.516 3.504 1.451l.53.53.53-.53c.937-.935 2.18-1.451 3.504-1.451h.521z" />
    </svg>
  );
}

function StoreIcon() {
  return (
    <svg viewBox="0 0 24 24" width="24" height="24" fill="none" stroke="#111" strokeWidth="1.5">
      <path d="M3 9l1.5-5h15L21 9M3 9h18M3 9v11a1 1 0 001 1h16a1 1 0 001-1V9" />
      <path d="M9 21V13h6v8" />
    </svg>
  );
}

function DeliveryIcon() {
  return (
    <svg viewBox="0 0 24 24" width="24" height="24" fill="none" stroke="#111" strokeWidth="1.5">
      <path d="M1.5 12h5l3-9 5 18 3-9h5" />
    </svg>
  );
}

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

interface Benefit {
  title: string;
  description: string;
}

interface Size {
  label: string;
  available: boolean;
}

interface Colorway {
  styleCode: string;
  selected: boolean;
}

interface Review {
  author: string;
  date: string;
  title: string;
  body: string;
  rating: number;
}

interface RecommendedProduct {
  title: string;
  subtitle: string;
  price: string;
  image: string;
}

interface ProductData {
  title: string;
  subtitle: string;
  currentPrice: string;
  originalPrice: string;
  discountPct: string;
  colorName: string;
  styleCode: string;
  description: string;
  reasonToBuy: string;
  weight: string;
  drop: string;
  benefits: Benefit[];
  whatsNew: string[];
  sizes: Size[];
  colorways: Colorway[];
  reviews: Review[];
  avgRating: string;
  reviewCount: string;
  recommended: RecommendedProduct[];
}

// ---------------------------------------------------------------------------
// Default data
// ---------------------------------------------------------------------------

const DEFAULT_BENEFITS: Benefit[] = [
  {
    title: "Propulsive Air Zoom Units",
    description: "Two forefoot Air Zoom units deliver a burst of energy with every stride, converting pressure into a powerful forward spring that propels you toward the finish line.",
  },
  {
    title: "ZoomX Foam Cushioning",
    description: "Nike's lightest and most energy-returning foam delivers a soft, responsive ride that helps keep your legs feeling fresh through all 26.2 miles of the marathon.",
  },
  {
    title: "Full-Length Carbon Fiber Plate",
    description: "A full-length carbon fiber plate adds stiffness and snap to each toe-off, working with the Air Zoom units to create a smooth, propulsive transition from landing to takeoff.",
  },
];

const DEFAULT_WHATS_NEW: string[] = [
  "Updated Atomknit 3.0 upper is lighter and more breathable",
  "Redesigned heel counter improves lockdown and reduces slippage",
  "Wider forefoot platform increases stability at speed",
  "Refined ZoomX foam geometry for smoother transitions",
  "New outsole rubber compound improves wet-surface traction",
  "Streamlined tongue construction reduces weight and eliminates bunching",
];

const DEFAULT_SIZES: Size[] = [
  { label: "M 4", available: false },
  { label: "M 4.5", available: false },
  { label: "M 5", available: false },
  { label: "M 5.5", available: false },
  { label: "M 6", available: false },
  { label: "M 6.5", available: false },
  { label: "M 7", available: true },
  { label: "M 7.5", available: true },
  { label: "M 8", available: true },
  { label: "M 8.5", available: true },
  { label: "M 9", available: true },
  { label: "M 9.5", available: true },
  { label: "M 10", available: true },
  { label: "M 10.5", available: true },
  { label: "M 11", available: true },
  { label: "M 11.5", available: true },
  { label: "M 12", available: true },
  { label: "M 12.5", available: true },
  { label: "M 13", available: true },
  { label: "M 13.5", available: true },
  { label: "M 14", available: true },
  { label: "M 15", available: true },
];

const DEFAULT_COLORWAYS: Colorway[] = [
  { styleCode: "/images/nike/AIR+ZOOM+ALPHAFLY+NEXT%+3(1).png", selected: false },
  { styleCode: "/images/nike/AIR+ZOOM+ALPHAFLY+NEXT%+3(2).png", selected: false },
  { styleCode: "/images/nike/AIR+ZOOM+ALPHAFLY+NEXT%+3(3).png", selected: false },
  { styleCode: "/images/nike/AIR+ZOOM+ALPHAFLY+NEXT%+3.png", selected: true },
  { styleCode: "/images/nike/AIR+ZOOM+ALPHAFLY+NEXT%+3(4).png", selected: false },
  { styleCode: "/images/nike/AIR+ZOOM+ALPHAFLY+NEXT%+3(5).png", selected: false },
  { styleCode: "/images/nike/AIR+ZOOM+ALPHAFLY+NEXT%+3+EK.png", selected: false },
];

const DEFAULT_REVIEWS: Review[] = [
  {
    author: "MarathonMike",
    date: "DATE",
    title: "Best marathon shoe I've ever worn",
    body: "PR'd by 4 minutes in these at the LA Marathon. The energy return is insane and they felt great from start to finish. Worth every penny.",
    rating: 5,
  },
  {
    author: "SpeedyRunner",
    date: "DATE",
    title: "Incredible speed but narrow fit",
    body: "The propulsion is unmatched. You genuinely feel faster in these shoes. However, they run narrow so I'd recommend going up half a size if you have wider feet.",
    rating: 4,
  },
  {
    author: "RunnerJane",
    date: "DATE",
    title: "Game changer for race day",
    body: "I've raced in Vaporflys for years but the Alphafly 3 is on another level. The double Air Zoom units make a real difference on long runs. Finished my first sub-3 marathon!",
    rating: 5,
  },
  {
    author: "CasualJogger42",
    date: "DATE",
    title: "Too much shoe for my needs",
    body: "These are clearly built for serious racers. For my 10-minute mile pace, I found them unstable. Great shoe but not for everyone.",
    rating: 3,
  },
  {
    author: "TrackCoachTom",
    date: "DATE",
    title: "Perfect for my athletes",
    body: "I've had three of my marathon runners switch to the Alphafly 3 and all three set PRs. The combination of ZoomX, Air Zoom, and the carbon plate is elite.",
    rating: 5,
  },
  {
    author: "UltraDistanceAce",
    date: "DATE",
    title: "Holds up great over distance",
    body: "Wore these for a 50K training run. They held up better than I expected, though they're really optimized for marathon distance. The cushioning never bottomed out.",
    rating: 4,
  },
  {
    author: "BostonQualifier",
    date: "DATE",
    title: "Qualified for Boston in these",
    body: "Finally broke the BQ barrier after two years of trying. These shoes gave me the edge I needed. The ride is smooth and the energy return is real.",
    rating: 5,
  },
  {
    author: "DataDrivenRunner",
    date: "DATE",
    title: "Measurably faster",
    body: "I tracked my splits across identical workouts in Vaporfly 4 vs Alphafly 3. Consistently 3-5 seconds per mile faster in the Alphafly. The data doesn't lie.",
    rating: 5,
  },
  {
    author: "WeekendWarrior",
    date: "DATE",
    title: "Expensive but delivers",
    body: "At nearly $300 these aren't cheap, but if you're serious about racing they deliver. I knocked 8 minutes off my half marathon time. The price reflects the technology.",
    rating: 4,
  },
  {
    author: "RecoveryFocused",
    date: "DATE",
    title: "Less leg fatigue post-race",
    body: "The biggest benefit for me was how my legs felt after the race. Way less soreness compared to my old racing flats. The ZoomX foam really cushions the impact.",
    rating: 4,
  },
];

const DEFAULT_RECOMMENDED: RecommendedProduct[] = [
  { title: "Nike Zoom Fly 6", subtitle: "Men's Road Running Shoes", price: "$190", image: "/images/nike/ZOOM+FLY+6+GLAM.png" },
  { title: "Nike Zoom Vomero 5", subtitle: "Men's Shoes", price: "$170", image: "/images/nike/NIKE+ZOOM+VOMERO+5.png" },
  { title: "Nike Pegasus Plus", subtitle: "Men's Road Running Shoes", price: "$180", image: "/images/nike/NIKE+PEGASUS+PREMIUM+CM.png" },
  { title: "Nike Alphafly 3 Glam", subtitle: "Men's Road Racing Shoes", price: "$295", image: "/images/nike/alphafly-3-glam-mens-road-racing-shoes-5gZBgL.png" },
  { title: "Nike Pegasus Premium", subtitle: "Men's Road Running Shoes", price: "$220", image: "/images/nike/NIKE+PEGASUS+PREMIUM+CM.png" },
  { title: "Nike Vaporfly 4", subtitle: "Men's Road Racing Shoes", price: "$270", image: "/images/nike/ZOOMX+VAPORFLY+NEXT%+4.png" },
  { title: "Nike Vomero 18", subtitle: "Men's Road Running Shoes", price: "$165", image: "/images/nike/NIKE+VOMERO+18+CM.png" },
  { title: "Nike Pegasus 41", subtitle: "Men's Road Running Shoes", price: "$155", image: "/images/nike/AIR+ZOOM+PEGASUS+41+CM.png" },
  { title: "Nike Structure Plus", subtitle: "Men's Road Running Shoes", price: "$170", image: "/images/nike/NIKE+STRUCTURE+PLUS.png" },
  { title: "Nike Kiger 10", subtitle: "Men's Trail Running Shoes", price: "$170", image: "/images/nike/NIKE+TERRA+KIGER+10.png" },
];

const DEFAULT_DATA: ProductData = {
  title: "Nike Alphafly 3",
  subtitle: "Men's Road Racing Shoes",
  currentPrice: "$250.97",
  originalPrice: "$295",
  discountPct: "15% off",
  colorName: "White/Soft Pearl/Hyper Crimson/Black",
  styleCode: "IM6673-100",
  description: "Fine-tuned for marathon speed, the Alphafly 3 helps you push beyond what you thought possible with three innovative technologies. A double dose of Air Zoom units works with a full-length carbon fiber plate to help launch you into your next step. The ZoomX foam midsole helps keep you fresh from mile 1 to 26.2. Time to leave your old personal records in the dust.",
  reasonToBuy: "Pinnacle tech creates a propulsive ride fine-tuned for marathon speed.",
  weight: "Approx. 218g/7.7oz (Men's US 10)",
  drop: "8mm",
  benefits: DEFAULT_BENEFITS,
  whatsNew: DEFAULT_WHATS_NEW,
  sizes: DEFAULT_SIZES,
  colorways: DEFAULT_COLORWAYS,
  reviews: DEFAULT_REVIEWS,
  avgRating: "4.5",
  reviewCount: "50",
  recommended: DEFAULT_RECOMMENDED,
};

// ---------------------------------------------------------------------------
// Star rendering helper
// ---------------------------------------------------------------------------

function Stars({ rating }: { rating: number }) {
  return (
    <div className="nike-pdp__stars">
      {[1, 2, 3, 4, 5].map((i) => (
        <StarIcon key={i} filled={rating >= i} />
      ))}
    </div>
  );
}

// ---------------------------------------------------------------------------
// Sub-components
// ---------------------------------------------------------------------------

function ReviewCard({ review }: { review: Review }) {
  return (
    <div className="nike-pdp__review-card">
      <div className="nike-pdp__review-header">
        <Stars rating={review.rating} />
        <span className="nike-pdp__review-date">{review.date}</span>
      </div>
      <h3 className="nike-pdp__review-title">{review.title}</h3>
      <p className="nike-pdp__review-body">{review.body}</p>
      <span className="nike-pdp__review-author">{review.author}</span>
    </div>
  );
}

function RecCard({ rec }: { rec: RecommendedProduct }) {
  return (
    <div className="nike-pdp__rec-card">
      <a className="nike-pdp__rec-link" href="/showcase/nike/proa/product">
        <div className="nike-pdp__rec-img-wrap">
          <img
            className="nike-pdp__rec-img"
            src={rec.image}
            alt={rec.title}
            loading="lazy"
            decoding="async"
            width={400}
            height={400}
          />
        </div>
        <div className="nike-pdp__rec-info">
          <div className="nike-pdp__rec-title">{rec.title}</div>
          <div className="nike-pdp__rec-subtitle">{rec.subtitle}</div>
          <div className="nike-pdp__rec-price">{rec.price}</div>
        </div>
      </a>
    </div>
  );
}

function ProductInfo({ data }: { data: ProductData }) {
  return (
    <>
      <h1 className="nike-pdp__title">{data.title}</h1>
      <p className="nike-pdp__subtitle">{data.subtitle}</p>

      {/* Price */}
      <div className="nike-pdp__price">
        <span className="nike-pdp__current-price">{data.currentPrice}</span>
        {data.originalPrice && (
          <span className="nike-pdp__original-price">{data.originalPrice}</span>
        )}
        {data.discountPct && (
          <span className="nike-pdp__discount">{data.discountPct}</span>
        )}
      </div>

      {/* Colorways */}
      <div className="nike-pdp__color-section">
        <p className="nike-pdp__color-name">{data.colorName}</p>
        <div className="nike-pdp__colors">
          {data.colorways.map((colorway, i) => (
            <div
              key={i}
              className={colorway.selected ? "nike-pdp__color-chip selected" : "nike-pdp__color-chip"}
            >
              <img
                src={colorway.styleCode}
                alt="Colorway"
                width={80}
                height={80}
                loading="lazy"
              />
            </div>
          ))}
        </div>
      </div>

      {/* Size selector */}
      <div className="nike-pdp__size-section">
        <div className="nike-pdp__size-header">
          <span className="nike-pdp__size-label">Select Size</span>
          <a href="#" className="nike-pdp__size-guide">Size Guide</a>
        </div>
        <div className="nike-pdp__sizes">
          {data.sizes.map((size, i) => (
            <button
              key={i}
              className={size.available ? "nike-pdp__size" : "nike-pdp__size disabled"}
              type="button"
            >
              {size.label}
            </button>
          ))}
        </div>
      </div>

      {/* Add to Bag */}
      <button className="nike-pdp__btn-bag" type="button">Add to Bag</button>

      {/* Favorite */}
      <button className="nike-pdp__btn-fav" type="button">
        Favorite
        <HeartOutlineIcon />
      </button>

      {/* Shipping */}
      <div className="nike-pdp__shipping">
        <div className="nike-pdp__shipping-row">
          <StoreIcon />
          <div>
            <strong>Free Pickup</strong>
            <span> — </span>
            <a href="#">Find a Store</a>
          </div>
        </div>
        <div className="nike-pdp__shipping-row">
          <DeliveryIcon />
          <p>You'll see our shipping options at checkout.</p>
        </div>
      </div>
    </>
  );
}

// ---------------------------------------------------------------------------
// Main page component
// ---------------------------------------------------------------------------

export function NikePdpPage({ data }: { data?: Partial<ProductData> }) {
  const d: ProductData = { ...DEFAULT_DATA, ...data };

  return (
    <>
      <TopBar />
      <Header />
      <main>
        <div className="nike-pdp">
        {/* Breadcrumbs */}
        <nav className="nike-pdp__breadcrumbs">
          <a href="/showcase/nike/proa">Home</a>
          <span className="nike-pdp__breadcrumb-sep">/</span>
          <a href="/showcase/nike/proa">Men</a>
          <span className="nike-pdp__breadcrumb-sep">/</span>
          <a href="/showcase/nike/proa">Running</a>
          <span className="nike-pdp__breadcrumb-sep">/</span>
          <span>Running Shoes</span>
        </nav>

        {/* Hero: image + info */}
        <div className="nike-pdp__hero">
          <div className="nike-pdp__gallery">
            <img
              className="nike-pdp__main-img"
              src="/images/nike/AIR+ZOOM+ALPHAFLY+NEXT%+3(10).png"
              alt={d.title}
              width={800}
              height={800}
              fetchPriority="high"
            />
          </div>
          <div className="nike-pdp__info">
            <ProductInfo data={d} />
          </div>
        </div>

        {/* Description */}
        <section className="nike-pdp__section">
          <h2 className="nike-pdp__section-title">{d.title}</h2>
          <p className="nike-pdp__description">{d.description}</p>
          <p className="nike-pdp__reason">{d.reasonToBuy}</p>
        </section>

        {/* Benefits */}
        <section className="nike-pdp__section">
          <h2 className="nike-pdp__section-title">Benefits</h2>
          <div className="nike-pdp__benefits">
            {d.benefits.map((benefit, i) => (
              <div key={i} className="nike-pdp__benefit-card">
                <h3 className="nike-pdp__benefit-title">{benefit.title}</h3>
                <p className="nike-pdp__benefit-desc">{benefit.description}</p>
              </div>
            ))}
          </div>
        </section>

        {/* What's New */}
        <section className="nike-pdp__section">
          <h2 className="nike-pdp__section-title">What's New</h2>
          <ul className="nike-pdp__whats-new">
            {d.whatsNew.map((item, i) => (
              <li key={i}>{item}</li>
            ))}
          </ul>
        </section>

        {/* Product Details */}
        <section className="nike-pdp__section">
          <h2 className="nike-pdp__section-title">Product Details</h2>
          <dl className="nike-pdp__details">
            <div className="nike-pdp__detail-row">
              <dt>Weight</dt>
              <dd>{d.weight}</dd>
            </div>
            <div className="nike-pdp__detail-row">
              <dt>Drop</dt>
              <dd>{d.drop}</dd>
            </div>
            <div className="nike-pdp__detail-row">
              <dt>Shown</dt>
              <dd>{d.colorName}</dd>
            </div>
            <div className="nike-pdp__detail-row">
              <dt>Style</dt>
              <dd>{d.styleCode}</dd>
            </div>
          </dl>
        </section>

        {/* Reviews */}
        <section className="nike-pdp__section">
          <h2 className="nike-pdp__section-title">Reviews ({d.reviewCount})</h2>
          <div className="nike-pdp__rating-summary">
            <Stars rating={4} />
            <span className="nike-pdp__avg-rating">{d.avgRating} out of 5</span>
          </div>
          <div className="nike-pdp__reviews">
            {d.reviews.map((review, i) => (
              <ReviewCard key={i} review={review} />
            ))}
          </div>
        </section>

        {/* You Might Also Like */}
        <section className="nike-pdp__section">
          <h2 className="nike-pdp__section-title">You Might Also Like</h2>
          <div className="nike-pdp__recommended">
            {d.recommended.map((rec, i) => (
              <RecCard key={i} rec={rec} />
            ))}
          </div>
        </section>
        </div>
      </main>
      <Footer />
    </>
  );
}
