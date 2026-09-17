// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! cargo run -p proa_third_party --example preview --all-features > /tmp/preview.html

#[cfg(all(feature = "youtube", feature = "google-analytics"))]
fn main() {
    use proa_core::{WebContext, WebRenderSync};
    use proa_macros::html_sync;
    use proa_third_party::{google::GoogleAnalytics, youtube::YouTubeEmbed};
    // Fixed nonce for this local test fixture only. Production responses must
    // use a fresh nonce from their CSP middleware.
    let mut cx =
        WebContext::new().with_csp_nonce(proa_core::CspNonce::new("cHJvYS1wcmV2aWV3").unwrap());
    html_sync! {
        <html lang="en">
            <head>
                <meta charset="utf-8" />
                <meta name="viewport" content="width=device-width, initial-scale=1" />
                <title>"Third-party integration preview"</title>
                {GoogleAnalytics::new("G-TEST123456").unwrap()}
            </head>
            <body>
                <h1>"YouTube preview"</h1>
                {YouTubeEmbed::new("M7lc1UVf-VE", "YouTube player demonstration").unwrap()}
                <p>
                    "Analytics is consent-gated in this example and makes no Google Analytics requests."
                </p>
            </body>
        </html>
    }.render(&mut cx).unwrap();
    println!("<!doctype html>{}", cx.into_output().into_string());
}

#[cfg(not(all(feature = "youtube", feature = "google-analytics")))]
fn main() {
    eprintln!("Enable google-analytics and youtube to run this example.");
}
