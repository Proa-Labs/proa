---
description: Write tests for a Proa render component or rsjs island. Use when the user asks to add tests, validate rendering, set up snapshot testing, or wire up accessibility or visual regression checks.
---

# Test a Proa component

Proa components are plain Rust structs implementing `WebRender` /
`WebRenderSync`. Testing is straightforward: render through a
`RenderBuffer`, convert to a string, assert on the output.
Full guide:
[`testing.mdx`](../../../proa_marketing/src/docs/guides/testing.mdx).

## Unit test pattern

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use proa_core::text::text;
    use proa_core::WebContext;

    #[test]
    fn component_renders() {
        let mut cx = WebContext::new();

        Component {
            variant: ComponentVariant::Default,
            class: None,
            children: Some(text("Hello")),
        }
        .render(&mut cx)
        .unwrap();

        let html = cx.into_output().into_string();
        assert!(html.contains("data-component"));
        assert!(html.contains("Hello"));
    }
}
```

`WebContext::new()` owns the standard reusable production `RenderBuffer`
(4 KiB chained pages, never reallocates what it has written), so tests exercise
the same default write path as production. Recover it with `into_output()`;
`RenderBuffer::into_string()` reuses the first page's allocation when the
output fits in one page. `WebContext::with_buffer(custom_writer)` is the
advanced owned-writer constructor.

## Loop over variants

```rust
#[test]
fn badge_renders_all_variants() {
    for variant in [
        BadgeVariant::Default,
        BadgeVariant::Secondary,
        BadgeVariant::Destructive,
        BadgeVariant::Outline,
    ] {
        let mut cx = WebContext::new();
        Badge {
            variant,
            class: None,
            children: Some(text("Test")),
        }
        .render(&mut cx)
        .unwrap();
        let html = cx.into_output().into_string();
        assert!(html.contains("data-badge"));
        assert!(
            html.contains(&format!("data-variant=\"{}\"", variant.as_str())),
            "variant {:?} did not render data-variant attribute",
            variant,
        );
    }
}
```

One loop per variant enum is enough; the bug class this catches is
"forgot to wire one variant through `classes()` or `as_str()`."

## Test XSS escape behavior

Verify dynamic values get escaped. This is the **single most
important** test to write for any component that takes user input.

```rust
#[test]
fn card_escapes_xss_in_title() {
    let mut cx = WebContext::new();

    Card {
        title: "<script>alert('xss')</script>",
        description: "safe",
        ..Default::default()
    }
    .render(&mut cx)
    .unwrap();

    let html = cx.into_output().into_string();
    assert!(html.contains("&lt;script&gt;"));
    assert!(!html.contains("<script>alert"));
}
```

If your component intentionally uses `UnsafeRaw` for user content (you
have a sanitizer somewhere upstream), at minimum write a test that
proves the sanitizer ran.

## Test optional fields both ways

```rust
#[test]
fn card_without_optional_class() {
    let mut cx = WebContext::new();
    Card { title: "Test", class: None, ..Default::default() }
        .render(&mut cx).unwrap();
    let html = cx.into_output().into_string();
    assert!(html.contains("data-card"));
}

#[test]
fn card_with_custom_class() {
    let mut cx = WebContext::new();
    Card { title: "Test", class: Some("my-class"), ..Default::default() }
        .render(&mut cx).unwrap();
    let html = cx.into_output().into_string();
    assert!(html.contains("my-class"));
}
```

`Option<&str>` attrs should disappear when `None`; assert presence
only in the "with" case, never their absence (the macro guarantees
omission and tests for absence are brittle).

## Snapshot tests with `insta`

For larger components or full pages, snapshot the rendered HTML.

```toml
# Cargo.toml
[dev-dependencies]
insta = "1"
```

```rust
#[test]
fn listing_card_snapshot() {
    let mut cx = WebContext::new();
    ListingCard {
        title: "Cozy Mountain Cabin",
        location: "Lake Tahoe, CA",
        price: 185,
        image_url: "/images/cabin.jpg",
    }
    .render(&mut cx)
    .unwrap();
    let html = cx.into_output().into_string();
    insta::assert_snapshot!(html);
}
```

Workflow:

```bash
cargo test                # first run creates snapshots/<name>.snap
cargo install cargo-insta # one-time
cargo insta review        # interactive review of changed snapshots
```

When to snapshot vs assert:

- **Assert** for invariants (`data-foo` is present, variant attr is
  set, XSS escapes happened).
- **Snapshot** for layout / structure / class composition where you
  want to catch any unexpected diff.

Don't snapshot HTML that contains random IDs or timestamps; the
snapshot diffs will be noisy.

## Integration tests: Playwright + axe-core

Compile-time a11y checks (see `/add-component` § Accessibility) cover
the static surface. Color contrast, focus order, keyboard nav, and
screen reader announcements need runtime testing. Use axe-core via
Playwright:

```bash
npm install -D @playwright/test @axe-core/playwright
npx playwright install
```

```typescript
// tests/a11y.spec.ts
import { test, expect } from "@playwright/test";
import AxeBuilder from "@axe-core/playwright";

test("home page has no accessibility violations", async ({ page }) => {
    await page.goto("http://localhost:4000/");
    const results = await new AxeBuilder({ page })
        .withTags(["wcag2a", "wcag2aa"])
        .analyze();
    expect(results.violations).toEqual([]);
});
```

Visual regression in the same harness:

```typescript
test("listing card matches screenshot", async ({ page }) => {
    await page.goto("http://localhost:4000/rooms/12345");
    const card = page.locator("[data-listing-card]").first();
    await expect(card).toHaveScreenshot("listing-card.png");
});
```

The `data-*` attribute selectors are stable across class refactors;
prefer them over CSS class selectors in tests.

## Performance regression tests

For perf-critical components, benchmark with `criterion` and pin a
budget:

```rust
// benches/listing_card.rs
use criterion::{black_box, criterion_group, criterion_main, Criterion};

fn bench_render(c: &mut Criterion) {
    c.bench_function("listing_card", |b| {
        let mut buf = RenderBuffer::new();
        b.iter(|| {
            // render_into clears and renders, recycling the buffer's
            // pages across iterations exactly like a server would.
            buf.render_into(black_box(ListingCard { /* ... */ }));
        });
    });
}

criterion_group!(benches, bench_render);
criterion_main!(benches);
```

Run with `cargo bench`. Use against a known-good baseline; commit
the baseline numbers as a comment near the component so regressions
show up in code review.

## What to test, what to skip

| Test | Worth writing? |
|---|---|
| Renders without panic | ✅ always |
| `data-<component>` is present | ✅ always |
| Variants all set `data-variant` | ✅ if the component has variants |
| XSS escape on user-controlled fields | ✅ if any field can be untrusted |
| Optional class merges correctly | ✅ |
| Each branch of conditional rendering | ✅ |
| Class string exact match | ❌ brittle, snapshot it instead |
| Specific Tailwind classes present | ❌ couples test to design tokens |
| Output byte-exact equality | ❌ use insta snapshot |

## Run the tests

```bash
cargo test -p <owning-package> --lib
cargo test -p <owning-package> --lib card
cargo insta review                   # if snapshots changed
npx playwright test                  # integration
```
