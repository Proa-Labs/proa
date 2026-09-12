# proa_og

OG image generation using WebKit's headless rendering engine. Renders HTML/CSS to pixel-perfect PNG images using the same layout engine that powers Safari and GNOME Web.

## How it works

```
HTML + CSS string
  → WebKitGTK offscreen window (full WebCore layout engine)
  → webkit_web_view_get_snapshot → cairo surface
  → PNG encoding
  → Vec<u8>
```

No Yoga, no Taffy, no CSS subset — the real WebKit CSS/layout engine. Any HTML/CSS that works in a browser works here.

## Usage

```rust
use proa_og::{OgRenderer, RenderOptions};

let renderer = OgRenderer::new()?;

let html = r#"
<div style="width:1200px;height:630px;background:linear-gradient(135deg,#667eea,#764ba2);
            display:flex;align-items:center;justify-content:center;color:white;font-size:48px">
  Hello, OG Image!
</div>
"#;

let image = renderer.render_html(html, &RenderOptions::default())?;
std::fs::write("og.png", &image.png_bytes)?;
```

## Performance

The `OgRenderer` keeps a persistent WebView alive between renders. Subsequent renders skip WebView/window creation and just swap HTML content, making them significantly faster than cold starts.

| Scenario | Time |
|----------|------|
| Cold start (first render) | ~200-500ms |
| Warm render (reuse WebView) | ~50-100ms |

## Platform

**Linux only.** Requires WebKitGTK development libraries:

```bash
# Ubuntu/Debian
sudo apt-get install libwebkit2gtk-4.1-dev libgtk-3-dev

# Or for older WebKitGTK
sudo apt-get install libwebkit2gtk-4.0-dev libgtk-3-dev
```

On macOS, the crate compiles with stub bindings for IDE support but panics at runtime. Use the provided Dockerfile for local development:

```bash
docker build -t proa-og .
docker run --rm proa-og cargo test -p proa_og
```

## Architecture

- **`renderer.rs`** — `OgRenderer` with persistent WebView, GLib main loop integration, cairo surface snapshot
- **`ffi.rs`** — bindgen-generated FFI bindings to WebKitGTK + GTK + GLib + Cairo
- **`build.rs`** — probes for `webkit2gtk-4.1` via pkg-config, falls back to stubs on macOS

## Future: WPE WebKit

The current implementation uses WebKitGTK with a `GtkOffscreenWindow`, which requires GTK and adds overhead from the widget layer. Migrating to [WPE WebKit](https://wpewebkit.org/) would:

- Remove the GTK dependency (WPE is headless-first)
- Remove the `GtkOffscreenWindow` + cairo surface copy overhead
- Render directly to a buffer via WPE's rendering backend
- Eliminate the need for `xvfb-run` in Docker (no display server needed)

WPE WebKit is the WebKit port designed specifically for embedded and server-side rendering. The API is similar (`webkit_web_view_*`) but the backend is purpose-built for headless operation.

## Integration with Proa

Proa renders HTML in ~2-12µs. This crate takes that HTML and converts it to an OG image:

```rust
use proa_showcase::nike::product;
use proa_og::{OgRenderer, RenderOptions};

// Render HTML with Proa (microseconds)
let html = product::render();

// Convert to OG image with WebKit (milliseconds)
let renderer = OgRenderer::new()?;
let og = renderer.render_html(&html, &RenderOptions {
    width: 1200,
    height: 630,
    transparent: false,
    ..RenderOptions::default()
})?;

std::fs::write("nike-pdp-og.png", &og.png_bytes)?;
```
