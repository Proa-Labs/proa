// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

// Nike Catalog page component, React equivalent of the Proa Nike catalog showcase.
//
// Produces HTML matching the Proa server-rendered Nike product grid.
// 120 product cards with color swatches, filter sidebar, header/footer.

import React from "react";

// ---------------------------------------------------------------------------
// SVGs
// ---------------------------------------------------------------------------

export const NIKE_SWOOSH_SVG = `<svg aria-hidden="true" focusable="false" viewBox="0 0 24 24" role="img" width="78" height="78" fill="none"><path fill="currentColor" d="M21 8.719L7.836 14.303C6.74 14.768 5.818 15 5.075 15c-.836 0-1.445-.295-1.819-.884-.485-.76-.273-1.982.559-3.272.494-.754 1.122-1.446 1.734-2.108-.144.234-1.415 2.349-.025 3.345.275.2.666.298 1.147.298.386 0 .829-.063 1.316-.19L21 8.719z"></path></svg>`;

export const JORDAN_SVG = `<svg aria-hidden="true" focusable="false" viewBox="0 0 24 24" role="img" width="24px" height="24px" fill="none"><path fill="currentColor" fill-rule="evenodd" d="M13.182 3.733c-.012-.039-.012-.039-.012-.072a.966.966 0 01.922-1.007.97.97 0 011.007.922.964.964 0 01-.917 1.007c-.027.004-.062 0-.101 0-.016.004-.04.004-.056.022-.056.084.14.073-.005.44 0 0 0 .038-.012.077-.012.14-.08.562-.096.793.029.04.04.05.029.13a6.003 6.003 0 01-.09.534c-.04.14-.096.174-.147.202-.073.298-.095.545-.281.905-.022.276-.045.35-.106.484-.017.4.01.46-.281 1.101-.08.3-.017.507.05.821.068.321.276.461.298.793.05.771.017 1.305-.163 1.907l.135.348c.18.084.618.326.36.675.343.19.865.394 1.28.781.176.147.35.315.513.5.316.057.276.08.506.231.675.438 1.749 1.304 2.373 1.906.112.067.147.112.236.163.01.023.017.034.01.04-.026.072-.026.072-.06.14.039.04.095.073.134.107.04.005.04-.006.096-.017.079.073.18.135.214.135.106-.022.084-.005.185-.1.029-.035.084 0 .084 0 .04-.04.113-.119.214-.176.079-.045.23-.045.23-.045.052.006.04.051.029.073-.057.023-.18.057-.247.108-.152.14-.276.353-.276.353.299-.033.484.045.719.023.136-.005.237.006.377-.09 0 0 .14-.096.265-.14.118-.05.23-.017.33.062.069.084.119.084 0 .196-.044.045-.1.096-.18.17-.133.123-.313.291-.5.432a3.11 3.11 0 01-.527.315c-.338.23-.26.174-.523.394-.03.022-.124.078-.163.106-.107.062-.135.006-.197-.118 0 0-.028-.045-.08-.14-.05-.107-.09-.23-.072-.23-.062-.007-.331-.344-.331-.41-.063-.013-.304-.26-.31-.31l-.214-.18c-.253.044-.31-.113-.961-.608-.08-.006-.197-.05-.36-.174-.298-.253-1.007-.815-1.124-.883-.13-.067-.281-.134-.383-.214-.146-.022-.218-.05-.298-.067-.08-.022-.14-.057-.326-.079-.303-.045-.618-.18-.911-.337-.14-.073-.264-.107-.382-.169-.27-.124-.506-.236-.686-.28a2.148 2.148 0 01-.568-.226c-.061-.034-.095-.06-.134-.073-.09-.022-.153.006-.192.022-.23.108-.438.203-.636.31-.18.09-.348.186-.528.286a7.971 7.971 0 01-.534.254s-.534.253-.832.348c-.26.197-.787.546-1.107.715-.158.073-.467.252-.608.292-.08.061-.371.258-.596.421-.18.124-.31.231-.31.231-.106.084-.101.13-.28.045a1.491 1.491 0 00-.13.096c-.14.095-.146.073-.202.067-.101.08-.113.04-.197.13-.061.084 0 .061-.118.106-.028.006-.04.04-.057.056-.094.073-.1.293-.325.304-.135.09-.107.203-.197.191 0 .102-.18.23-.214.23-.292.096-.304-.118-.646.035-.045.016-.113.072-.197.084-.152.022-.332-.006-.444-.102a1.93 1.93 0 01-.326-.398c-.051-.13-.017-.208.163-.332.073-.045.084-.079.208-.084.06-.024.045.01.15-.024.064-.016.064-.005.193-.005.028-.017.067-.022.124-.045.1-.034.196-.062.196-.062s.028-.023.124-.01c.078-.035.169-.08.214-.097-.012-.124.005-.124.06-.174.08-.062.09-.05.148-.01.022-.007.039-.013.027-.035-.01-.073-.061-.107-.045-.247-.022-.057-.061-.129-.05-.174.01-.045.028-.069.056-.079.029-.012.045.006.057.022.028.034.05.135.05.135.006.118.04.26.152.18.067-.062.084-.242.214-.203l.096.085c.084-.073.084-.073.14-.107 0 0-.08-.073-.012-.135.045-.039.108-.067.208-.175.276-.292.422-.42.714-.657a6.811 6.811 0 011.699-.939c.146-.174.28-.286.585-.304.377-.606 1.085-1.136 1.248-1.22.134-.23.19-.208.365-.247.135-.107.175-.107.23-.214.063-.23-.112-.86.383-.877.112-.146.078-.112.196-.248a2.19 2.19 0 01-.118-.5c-.005-.016-.196-.157-.13-.332a2.33 2.33 0 01-.268-.432.202.202 0 01-.063-.012c-.022-.005-.055-.005-.09-.005-.078.196-.163.208-.303.253-.26.512-.35.731-1.046 1.142-.28.298-.382.64-.382.634a.46.46 0 00-.012.321c-.045.107-.027.124-.027.124.01.045.056.106.106.112.079.023.169.023.158.118-.011.113-.163.09-.237.073-.275-.05-.185-.23-.365-.174-.141.085-.196.348-.416.31-.028-.023-.017-.074.006-.119.028-.06.084-.118.056-.14-.146.04-.433.123-.433.123-.135.04-.281-.039-.147-.124.063-.022.153-.05.265-.118 0 0 .062-.072-.057-.039a1.144 1.144 0 01-.416.045s-.257-.039-.292-.056c-.028-.022-.061-.107.017-.1a2.71 2.71 0 00.563-.068c.095-.035.28-.14.382-.186 0 0 .113-.157.18-.19.107-.114.19-.18.28-.299.09-.18.192-.46.5-.906a4.16 4.16 0 01.535-.646s.062-.338.343-.573c.063-.14.157-.31.259-.462a1.7 1.7 0 00.106-.168c.09-.13.186-.377.518-.41 0 0 .147-.102.197-.175.084-.073.074-.186.14-.259-.106-.106-.365-.309-.382-.573a.85.85 0 01.265-.692c.196-.185.398-.275.646-.258.309.055.366.157.455.258.09.101.13.04.163.146.259.073.248.045.237.236.045.057.106.108.1.214.085-.175.108-.208.344-.399.062-.157.1-.315.15-.478.052-.146.114-.298.154-.41-.04-.326.06-.377.196-.664-.022-.039-.016-.05-.006-.112.057-.192.136-.433.186-.596 0 0 .017-.063.085-.063.06-.202.157-.579.179-.663.062-.208.029-.287-.01-.41-.012-.04-.006-.09-.03-.136a5.483 5.483 0 01-.19-.41c-.028-.073-.08-.354-.08-.354-.004-.062-.004-.09-.004-.09z" clip-rule="evenodd"></path></svg>`;

export const CONVERSE_SVG = `<svg aria-hidden="true" focusable="false" viewBox="0 0 24 24" role="img" width="24px" height="24px" fill="none"><path fill="currentColor" fill-rule="evenodd" d="M8.045 10.99V7.73L6.19 10.37 3.115 9.35l1.94 2.64-1.905 2.64 3.045-1.005 1.855 2.64v-3.26l3.11-1.02-3.115-.995zm12.84 1.02l-6.82 7.49H8.21l6.825-7.49L8.21 4.5h5.856l6.819 7.51z" clip-rule="evenodd"></path></svg>`;

export const HEART_ICON = `<svg aria-hidden="true" focusable="false" viewBox="0 0 24 24" role="img" width="24" height="24" fill="none"><path stroke="currentColor" stroke-width="1.5" d="M16.794 3.75c1.324 0 2.568.516 3.504 1.451a4.96 4.96 0 010 7.008L12 20.508l-8.299-8.299a4.96 4.96 0 010-7.008A4.923 4.923 0 017.205 3.75c1.324 0 2.568.516 3.504 1.451l.53.53.53-.53c.937-.935 2.18-1.451 3.504-1.451h.521z"></path></svg>`;

export const BAG_ICON = `<svg aria-hidden="true" focusable="false" viewBox="0 0 24 24" role="img" width="24" height="24" fill="none"><path stroke="currentColor" stroke-width="1.5" d="M8.25 8.25V6a2.25 2.25 0 012.25-2.25h3A2.25 2.25 0 0115.75 6v2.25m-11.5 0h15.5a1 1 0 011 1V19a2 2 0 01-2 2H5.25a2 2 0 01-2-2V9.25a1 1 0 011-1z"></path></svg>`;

function SearchIcon() {
  return (
    <svg aria-hidden="true" focusable="false" viewBox="0 0 24 24" role="img" width="24" height="24" fill="none">
      <path stroke="currentColor" strokeWidth="1.5" d="M13.962 16.296a6.716 6.716 0 01-3.462.954 6.728 6.728 0 01-4.773-1.977A6.728 6.728 0 013.75 10.5c0-1.864.755-3.551 1.977-4.773A6.728 6.728 0 0110.5 3.75c1.864 0 3.551.755 4.773 1.977A6.728 6.728 0 0117.25 10.5a6.726 6.726 0 01-.921 3.407c-.517.882-.434 1.988.289 2.711l3.853 3.853" />
    </svg>
  );
}

function ChevronIcon() {
  return (
    <svg aria-hidden="true" focusable="false" viewBox="0 0 24 24" role="img" width="12" height="12" fill="none">
      <path stroke="currentColor" strokeWidth="1.5" d="M19.5 8.25l-7.5 7.5-7.5-7.5" />
    </svg>
  );
}

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

interface Product {
  title: string;
  subtitle: string;
  price: string;
  messaging: string;
  image: string;
  colors: string[];
  colorCount: string;
}

interface CatalogData {
  title: string;
  products: Product[];
}

// ---------------------------------------------------------------------------
// Filter data
// ---------------------------------------------------------------------------

const FILTER_GROUPS: [string, [string, boolean][]][] = [
  ["Gender", [["Men", true], ["Women", false], ["Unisex", false]]],
  ["Shop By Price", [["$25 - $50", false], ["$50 - $100", false], ["$100 - $150", false], ["Over $150", false]]],
  ["Sale & Offers", [["Sale", false], ["Member Exclusives", false]]],
  ["Brand", [["Nike Sportswear", false], ["Nike By You", false], ["NikeLab", false], ["ACG", false], ["Jordan", false]]],
  ["Sport", [["Lifestyle", false], ["Running", false], ["Basketball", false], ["Football", false], ["Soccer", false], ["Training & Gym", false], ["Skateboarding", false], ["Golf", false], ["Tennis", false], ["Baseball", false], ["Trail Running", false]]],
  ["Athletes", [["LeBron James", false], ["Kylian Mbappé", false], ["Giannis Antetokounmpo", false], ["Sabrina Ionescu", false], ["Kevin Durant", false], ["Ja Morant", false]]],
];

const FILTER_COLORS: [string, string][] = [
  ["Black", "background:#111"], ["Blue", "background:#326BDA"], ["Brown", "background:#8D6346"],
  ["Green", "background:#2E7D4F"], ["Grey", "background:#808080"],
  ["Multi-Color", "background:conic-gradient(red, yellow, lime, aqua, blue, magenta, red)"],
  ["Orange", "background:#D95B28"], ["Pink", "background:#E87EA1"], ["Purple", "background:#744C9C"],
  ["Red", "background:#C53333"], ["White", "background:#FFF"], ["Yellow", "background:#E3B42A"],
];

// ---------------------------------------------------------------------------
// Components
// ---------------------------------------------------------------------------

export function TopBar() {
  return (
    <div className="nike-topbar">
      <div className="nike-topbar__brands">
        <a href="#" aria-label="Jordan" dangerouslySetInnerHTML={{ __html: JORDAN_SVG }} />
        <a href="#" aria-label="Converse" dangerouslySetInnerHTML={{ __html: CONVERSE_SVG }} />
      </div>
      <div className="nike-topbar__links">
        <a href="#">Find a Store</a>
        <span className="nike-topbar__sep">|</span>
        <a href="#">Help</a>
        <span className="nike-topbar__sep">|</span>
        <a href="#">Join Us</a>
        <span className="nike-topbar__sep">|</span>
        <a href="#">Sign In</a>
      </div>
    </div>
  );
}

export function Header() {
  return (
    <header className="nike-header">
      <a href="/showcase/nike/proa" className="nike-logo" aria-label="Nike home" dangerouslySetInnerHTML={{ __html: NIKE_SWOOSH_SVG }} />
      <nav className="nike-nav">
        <a href="/showcase/nike/proa">Men</a>
        <a href="/showcase/nike/proa">Women</a>
        <a href="/showcase/nike/proa">Kids</a>
        <a href="/showcase/nike/proa">Jordan</a>
        <a href="/showcase/nike/proa">NikeSKIMS</a>
        <a href="/showcase/nike/proa">Sport</a>
      </nav>
      <div className="nike-header__actions">
        <div className="nike-search">
          <SearchIcon />
          <span>Search</span>
        </div>
        <a href="#" aria-label="Favorites" dangerouslySetInnerHTML={{ __html: HEART_ICON }} />
        <a href="#" aria-label="Bag" dangerouslySetInnerHTML={{ __html: BAG_ICON }} />
      </div>
    </header>
  );
}

function FilterSidebar() {
  return (
    <aside className="nike-filters">
      <div className="nike-filter-group">
        <div className="nike-filter-group__title">Shoes</div>
      </div>
      {FILTER_GROUPS.map(([title, items], gi) => (
        <div key={gi} className="nike-filter-group">
          <div className="nike-filter-group__title">
            {title}
            <ChevronIcon />
          </div>
          {items.map(([label, checked], ii) => (
            <label key={ii} className="nike-filter-item">
              <div className={checked ? "nike-checkbox is-checked" : "nike-checkbox"} />
              <span>{label}</span>
            </label>
          ))}
        </div>
      ))}
      <div className="nike-filter-group">
        <div className="nike-filter-group__title">
          Color
          <ChevronIcon />
        </div>
        <div className="nike-filter-colors">
          {FILTER_COLORS.map(([name, style], i) => (
            <div key={i} className="nike-filter-color" style={{ cssText: style } as any} title={name} />
          ))}
        </div>
      </div>
    </aside>
  );
}

function ProductCard({ product }: { product: Product }) {
  return (
    <div className="nike-card">
      <a className="nike-card__link" href="/showcase/nike/proa/product">
        <div className="nike-card__img-wrap">
          <img
            className="nike-card__img"
            src={product.image}
            alt={product.title}
            loading="lazy"
            decoding="async"
            width={592}
            height={592}
          />
        </div>
        <div className="nike-card__info">
          {product.messaging && (
            <div className="nike-card__messaging">{product.messaging}</div>
          )}
          <div className="nike-card__title">{product.title}</div>
          <div className="nike-card__subtitle">{product.subtitle}</div>
          {product.colors.length > 0 && (
            <>
              <div className="nike-card__swatches">
                {product.colors.map((style, i) => (
                  <div key={i} className="nike-swatch" style={{ backgroundColor: style.replace("background-color:", "") }} />
                ))}
              </div>
              <div className="nike-card__color-count">{product.colorCount}</div>
            </>
          )}
          {product.price && (
            <div className="nike-card__price">{product.price}</div>
          )}
        </div>
      </a>
    </div>
  );
}

export function Footer() {
  return (
    <>
      <p className="nike-disclaimer">
        <strong>Disclaimer: </strong>
        This is an unofficial technology demo built with{" "}
        <a href="/">Proa UI</a> to showcase server-rendered UI components. It is{" "}
        <strong>not affiliated with, endorsed by, or connected to Nike, Inc.</strong>{" "}
        in any way. All Nike trademarks, logos, and design elements belong to their
        respective owners and are used here solely for illustrative purposes.
      </p>
      <footer className="nike-footer">
        <div className="nike-footer__cols">
          <div className="nike-footer__col">
            <div className="nike-footer__heading">Resources</div>
            <a href="#">Gift Cards</a><a href="#">Find a Store</a>
            <a href="#">Become a Member</a><a href="#">Nike Journal</a>
            <a href="#">Send Us Feedback</a>
          </div>
          <div className="nike-footer__col">
            <div className="nike-footer__heading">Help</div>
            <a href="#">Get Help</a><a href="#">Order Status</a>
            <a href="#">Shipping and Delivery</a><a href="#">Returns</a>
            <a href="#">Order Cancellation</a><a href="#">Payment Options</a>
          </div>
          <div className="nike-footer__col">
            <div className="nike-footer__heading">Company</div>
            <a href="#">About Nike</a><a href="#">News</a>
            <a href="#">Careers</a><a href="#">Investors</a>
            <a href="#">Purpose</a><a href="#">Sustainability</a>
          </div>
        </div>
        <div className="nike-footer__bottom">
          <span>&copy; 2026 Proa Labs, Inc. &middot; This is a demo, not a real Nike site &middot; Nike&reg; is a registered trademark of Nike, Inc.</span>
          <div>
            <a href="#">Guides</a><a href="#">Terms of Use</a><a href="#">Privacy Policy</a>
          </div>
        </div>
      </footer>
    </>
  );
}

// ---------------------------------------------------------------------------
// Main page component
// ---------------------------------------------------------------------------

export function NikeCatalogPage({ data }: { data?: Partial<CatalogData> }) {
  const title = data?.title ?? "Men's Shoes & Sneakers";
  const products = data?.products ?? [];

  return (
    <>
      <TopBar />
      <Header />
      <div className="nike-page-container">
        <div className="nike-page-header">
          <h1 className="nike-page-title">{title}</h1>
        </div>
        <div className="nike-results-bar">
          <span className="nike-results-count">{products.length} Results</span>
          <div className="nike-sort">
            <span className="nike-sort-label">Sort By</span>
            <span className="nike-sort-value">Featured</span>
          </div>
        </div>
        <div className="nike-layout">
          <FilterSidebar />
          <div className="nike-grid">
            <div className="nike-grid__items">
              {products.map((product, i) => (
                <ProductCard key={i} product={product} />
              ))}
            </div>
          </div>
        </div>
      </div>
      <Footer />
    </>
  );
}
