# proa_showcase

Reusable, optimized showcase page fixtures live here.

This crate owns page-shaped SSR fixtures that should be shared across the
playground, benchmarks, and comparison harnesses. It should not own servers,
routes, rsjs demo wiring, framework adapters, or benchmark orchestration.

Current fixtures:

- `src/airbnb`: Airbnb home/search/listing mirrors.
- `src/nike`: Nike catalog/product mirrors and shared CSS.
- `src/ebay`: eBay SRP card/chrome renderers plus captured fixture data.
- `src/espn`: ESPN Gamecast/box-score mirror and captured game data.
- `src/sentry_otel`: Sentry trace-log mirror with scalable OpenTelemetry rows.

Keep captured assets next to the fixture that owns them. For eBay, that means
`src/ebay/data`; React, Next.js, Marko, and Proa demo code all read from that
same directory so the engines compare against identical inputs.

The root workspace release profile intentionally keeps this crate fully
optimized because these pages are rendered in production-style benchmarks.
