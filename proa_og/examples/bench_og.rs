// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Simple benchmark comparing WebKitGTK and WPE WebKit rendering backends.
//!
//! Run with WebKitGTK (default):
//!   cargo run --example bench_og
//!
//! Run with WPE WebKit:
//!   cargo run --example bench_og --features wpe
//!
//! Run both (two separate invocations):
//!   cargo run --example bench_og
//!   cargo run --example bench_og --features wpe

use std::time::Instant;

use proa_og::{OgImageError, RenderOptions};

const HTML: &str = r#"
<!DOCTYPE html>
<html>
<head>
<style>
  body {
    margin: 0;
    width: 1200px;
    height: 630px;
    display: flex;
    align-items: center;
    justify-content: center;
    background: linear-gradient(135deg, #667eea 0%, #764ba2 100%);
    font-family: system-ui, -apple-system, sans-serif;
    color: white;
  }
  .card {
    text-align: center;
    padding: 40px;
  }
  h1 { font-size: 64px; margin: 0 0 16px; }
  p { font-size: 28px; opacity: 0.9; margin: 0; }
</style>
</head>
<body>
  <div class="card">
    <h1>Benchmark Test</h1>
    <p>Comparing WebKitGTK vs WPE WebKit rendering</p>
  </div>
</body>
</html>
"#;

const ITERATIONS: usize = 5;

fn main() -> Result<(), OgImageError> {
    let options = RenderOptions::default();

    // Benchmark the default OgRenderer (WebKitGTK)
    #[cfg(not(feature = "wpe"))]
    {
        println!("=== WebKitGTK Backend ===");
        let start = Instant::now();
        let renderer = proa_og::OgRenderer::new()?;
        let init_time = start.elapsed();
        println!("  Init:     {:?}", init_time);

        // Warmup render
        let _ = renderer.render_html(HTML, &options)?;

        let mut times = Vec::with_capacity(ITERATIONS);
        for i in 0..ITERATIONS {
            let start = Instant::now();
            let result = renderer.render_html(HTML, &options)?;
            let elapsed = start.elapsed();
            times.push(elapsed);
            println!(
                "  Render {}: {:?} ({}x{}, {} bytes PNG)",
                i + 1,
                elapsed,
                result.width,
                result.height,
                result.png_bytes.len(),
            );
        }

        let total: std::time::Duration = times.iter().sum();
        let avg = total / ITERATIONS as u32;
        println!("  Average:  {:?}", avg);
        println!();
    }

    // Benchmark the WPE renderer
    #[cfg(feature = "wpe")]
    {
        println!("=== WPE WebKit Backend ===");
        let start = Instant::now();
        let renderer = proa_og::WpeRenderer::new()?;
        let init_time = start.elapsed();
        println!("  Init:     {:?}", init_time);

        // Warmup render
        let _ = renderer.render_html(HTML, &options)?;

        let mut times = Vec::with_capacity(ITERATIONS);
        for i in 0..ITERATIONS {
            let start = Instant::now();
            let result = renderer.render_html(HTML, &options)?;
            let elapsed = start.elapsed();
            times.push(elapsed);
            println!(
                "  Render {}: {:?} ({}x{}, {} bytes PNG)",
                i + 1,
                elapsed,
                result.width,
                result.height,
                result.png_bytes.len(),
            );
        }

        let total: std::time::Duration = times.iter().sum();
        let avg = total / ITERATIONS as u32;
        println!("  Average:  {:?}", avg);
        println!();
    }

    Ok(())
}
