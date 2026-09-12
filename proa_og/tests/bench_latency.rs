// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use proa_og::{OgRenderer, RenderOptions};
use std::time::Instant;

#[test]
fn bench_latency_breakdown() {
    let renderer = match OgRenderer::new() {
        Ok(r) => r,
        Err(e) => {
            eprintln!("Skipping: {e}");
            return;
        }
    };

    let html = r#"
    <html><head><style>
        * { margin: 0; padding: 0; box-sizing: border-box; }
        body { width: 1200px; height: 630px; display: flex; align-items: center;
               justify-content: center; background: #1a1a2e; color: white;
               font-family: system-ui; }
        h1 { font-size: 64px; }
    </style></head>
    <body><h1>Benchmark</h1></body></html>
    "#;

    let options = RenderOptions::default();

    // Cold render (first render, includes all initialization)
    let t0 = Instant::now();
    let img = renderer.render_html(html, &options).expect("render failed");
    let cold = t0.elapsed();
    eprintln!(
        "Cold render:  {:>6.1}ms  ({}x{}, {} bytes PNG)",
        cold.as_secs_f64() * 1000.0,
        img.width,
        img.height,
        img.png_bytes.len()
    );

    // Warm renders (reuse everything)
    let mut times = Vec::new();
    for i in 0..10 {
        let varied_html = format!(
            r#"
        <html><head><style>
            * {{ margin: 0; padding: 0; box-sizing: border-box; }}
            body {{ width: 1200px; height: 630px; display: flex; align-items: center;
                   justify-content: center; background: #1a1a2e; color: white;
                   font-family: system-ui; }}
            h1 {{ font-size: 64px; }}
        </style></head>
        <body><h1>Render #{}</h1></body></html>
        "#,
            i
        );

        let t = Instant::now();
        let _ = renderer
            .render_html(&varied_html, &options)
            .expect("render failed");
        let elapsed = t.elapsed();
        times.push(elapsed.as_secs_f64() * 1000.0);
    }

    let avg = times.iter().sum::<f64>() / times.len() as f64;
    let min = times.iter().cloned().fold(f64::INFINITY, f64::min);
    let max = times.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    eprintln!("Warm renders: avg={avg:.1}ms  min={min:.1}ms  max={max:.1}ms  (n=10)");

    // PNG encoding only (from raw pixels)
    let raw = &img.pixels;
    let t_png = Instant::now();
    for _ in 0..10 {
        let _ = encode_png(raw, img.width, img.height);
    }
    let png_avg = t_png.elapsed().as_secs_f64() * 100.0; // /10 * 1000
    eprintln!("PNG encode:   {png_avg:.1}ms avg  (10 iters)");
}

fn encode_png(bgra: &[u8], width: u32, height: u32) -> Vec<u8> {
    let mut rgba = Vec::with_capacity(bgra.len());
    for chunk in bgra.chunks_exact(4) {
        rgba.push(chunk[2]);
        rgba.push(chunk[1]);
        rgba.push(chunk[0]);
        rgba.push(chunk[3]);
    }
    let mut buf = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut buf, width, height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().unwrap();
        writer.write_image_data(&rgba).unwrap();
    }
    buf
}
