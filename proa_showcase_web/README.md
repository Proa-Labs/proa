# proa_showcase_web

Shared Axum route and asset serving layer for `proa_showcase`.

This crate deliberately sits next to `proa_showcase` instead of inside it:

- `proa_showcase` owns page-shaped render fixtures.
- `proa_showcase_web` owns HTTP route handlers and public asset lookup.

Servers can mount `proa_showcase_web::router()` to expose the canonical
showcase routes plus the root asset paths expected by the fixtures:

- `/showcase/airbnb/proa`
- `/showcase/nike/proa`
- `/showcase/ebay/proa`
- `/showcase/espn/proa`
- `/showcase/sentry-otel/proa`

- `/proa.css`
- `/proa.js`
- `/assets/*`
- `/fonts/*`
- `/images/*`

By default the asset handler checks these public roots in order:

1. `SHOWCASE_PUBLIC_DIR`
2. `STATIC_DIR`
3. this crate's `public/`
4. this repository's `proa_marketing/public/`

The checked-in `public/assets/airbnb-showcase.js` file is the source-owned
progressive-enhancement script for the Airbnb-shaped fixtures.
The listing's calendars and booking card are Rust-authored RSJS components
served from the island manifest. Place image and font assets under the configured public root at the
paths referenced by the fixture HTML.
