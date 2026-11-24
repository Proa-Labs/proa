// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Integration test for OG image rendering.
//!
//! Run inside the Docker container:
//!   cargo test -p proa_og

use proa_og::{OgImageError, OgRenderer, RenderOptions};

// WebKit requires its native callbacks and process teardown on the main thread.
fn main() {
    let renderer = match OgRenderer::new() {
        Ok(renderer) => renderer,
        Err(OgImageError::PlatformNotAvailable | OgImageError::GtkInit) => return,
        Err(error) => panic!("renderer initialization failed: {error}"),
    };
    assert!(matches!(
        renderer.render_html("before\0after", &RenderOptions::default()),
        Err(OgImageError::LoadFailed(_))
    ));
    render_simple_og_image(&renderer);
    render_with_flexbox_and_grid(&renderer);
    let options = RenderOptions {
        width: 640,
        height: 360,
        ..RenderOptions::default()
    };
    let image = renderer
        .render_html("<div style='height: 1500px'></div>", &options)
        .expect("render a page longer than the viewport");
    assert_eq!((image.width, image.height), (options.width, options.height));
}

fn render_simple_og_image(renderer: &OgRenderer) {
    let html = r#"
    <html>
    <head>
        <style>
            * { margin: 0; padding: 0; box-sizing: border-box; }
            body {
                width: 1200px;
                height: 630px;
                display: flex;
                align-items: center;
                justify-content: center;
                background: linear-gradient(135deg, #667eea 0%, #764ba2 100%);
                font-family: 'Inter', system-ui, sans-serif;
                color: white;
            }
            .card {
                text-align: center;
                padding: 60px;
            }
            h1 {
                font-size: 72px;
                font-weight: 800;
                margin-bottom: 24px;
                letter-spacing: -2px;
            }
            p {
                font-size: 28px;
                opacity: 0.85;
            }
        </style>
    </head>
    <body>
        <div class="card">
            <h1>Proa SSR Engine</h1>
            <p>10-20x faster than traditional template engines</p>
        </div>
    </body>
    </html>
    "#;

    let options = RenderOptions {
        width: 1200,
        height: 630,
        transparent: false,
        ..RenderOptions::default()
    };

    let image = renderer.render_html(html, &options).expect("render failed");

    assert_eq!(image.width, 1200);
    assert_eq!(image.height, 630);
    assert!(!image.png_bytes.is_empty(), "PNG bytes should not be empty");
    assert!(
        image.png_bytes.starts_with(&[0x89, 0x50, 0x4E, 0x47]),
        "should be valid PNG (magic bytes)"
    );

    // Write to tmp for visual inspection
    std::fs::write("/tmp/proa_og_test.png", &image.png_bytes).ok();
    eprintln!(
        "Rendered {}x{} image, {} bytes PNG, {} bytes raw pixels",
        image.width,
        image.height,
        image.png_bytes.len(),
        image.pixels.len()
    );
}

fn render_with_flexbox_and_grid(renderer: &OgRenderer) {
    let html = r#"
    <html>
    <head>
        <style>
            * { margin: 0; padding: 0; box-sizing: border-box; }
            body {
                width: 1200px;
                height: 630px;
                display: grid;
                grid-template-columns: 1fr 1fr;
                gap: 0;
                font-family: system-ui;
            }
            .left {
                background: #0f172a;
                color: white;
                display: flex;
                flex-direction: column;
                justify-content: center;
                padding: 60px;
            }
            .right {
                background: #1e293b;
                display: flex;
                align-items: center;
                justify-content: center;
                color: #94a3b8;
                font-size: 120px;
            }
            h1 { font-size: 48px; margin-bottom: 16px; }
            p { font-size: 20px; color: #94a3b8; line-height: 1.6; }
        </style>
    </head>
    <body>
        <div class="left">
            <h1>CSS Grid + Flexbox</h1>
            <p>Real browser layout engine.<br>No Yoga. No Taffy. No compromises.</p>
        </div>
        <div class="right">⚡</div>
    </body>
    </html>
    "#;

    let image = renderer
        .render_html(html, &RenderOptions::default())
        .expect("render failed");

    assert!(!image.png_bytes.is_empty());
    std::fs::write("/tmp/proa_og_grid_test.png", &image.png_bytes).ok();
}
