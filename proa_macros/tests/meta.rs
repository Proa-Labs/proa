// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use proa_core::render_to_string;
use proa_macros::html_sync;

#[test]
fn property_meta_accepts_static_and_dynamic_content() {
    let title = "Szkolenia & <examples> \"quoted\" 'single'";
    let output = render_to_string(html_sync! {
        <meta property="og:type" content="website" />
        <meta property="og:title" content={title} />
    });

    assert_eq!(
        output,
        concat!(
            r#"<meta property="og:type" content="website">"#,
            r#"<meta property="og:title" content="Szkolenia &amp; &lt;examples&gt; &quot;quoted&quot; &#39;single&#39;">"#,
        )
    );
}
