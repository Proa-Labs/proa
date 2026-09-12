// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Every dossier pane requests the same `ripestat:dossier:<query>` key; the
//! request-scoped resolver must collapse them into one origin fan-out.

use std::sync::Arc;

use proa_origin_routing_atlas::loader::FixtureLoader;

#[tokio::test]
async fn seven_pane_requests_collapse_to_one_origin_load() {
    let loader = Arc::new(FixtureLoader::github());
    let html = proa_origin_routing_atlas::render_atlas_page(None, Arc::clone(&loader)).await;

    assert!(html.contains("GitHub's network"));
    assert!(html.contains("140.82.112.4"));
    assert!(html.contains("AS36459"));
    assert_eq!(
        loader.origin_loads(),
        1,
        "the dossier panes must share one deduplicated RIPEstat fan-out"
    );
}
