// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use proa_core::*;
use std::sync::{Arc, Mutex};

fn resolve(
    mut config: HeadConfig,
    patches: &[fn(&mut proa_core::web::HeadWriter<'_>) -> Result<(), WriteError>],
) -> String {
    // Shared layers are configuration; only the final patch is the Page hook.
    let (page, defaults) = patches.split_last().unwrap();
    for defaults in defaults {
        config = config.with_defaults(defaults).unwrap();
    }
    let wire = Arc::new(Mutex::new(Vec::new()));
    let sink = wire.clone();
    let mut cx = WebContext::new();
    cx.configure_head(
        config,
        Arc::new(move |_, pages| {
            for page in pages {
                sink.lock().unwrap().extend_from_slice(&page);
            }
            Ok(())
        }),
    )
    .unwrap();
    let doc = cx.head_state().document().unwrap();
    let guard = doc.begin_hook().unwrap();
    let scope = guard.finish(page(&mut cx.head_mut())).unwrap();
    cx.managed_seal().unwrap();
    WebRenderSync::render(cx.head(), &mut cx).unwrap();
    scope.finish(Ok(())).unwrap();
    doc.final_check().unwrap();
    let result = String::from_utf8(wire.lock().unwrap().clone()).unwrap();
    result
}
fn config() -> HeadConfig {
    HeadConfig::default().with_urls(HeadUrlPolicy::new("https://example.test", "/shop").unwrap())
}
#[test]
fn nearest_configured_template_and_final_social_fallbacks() {
    let defaults = config()
        .with_defaults(|h| {
            h.title_template("%s | Site")?;
            h.description("default description")?;
            h.canonical("/product")?;
            h.open_graph(OpenGraph::new().kind("product"))
        })
        .unwrap();
    let html = resolve(
        defaults.clone(),
        &[
            |h| {
                h.title("Shop")?;
                h.title_template("[%s]")
            },
            |h| h.title("A < B & C"),
        ],
    );
    assert!(html.contains("<title>[A &lt; B &amp; C]</title>"));
    assert!(html.contains("property=\"og:title\" content=\"[A &lt; B &amp; C]\""));
    assert!(html.contains("property=\"og:url\" content=\"https://example.test/shop/product\""));
    assert!(!html.contains("| Site"));
    let second = resolve(defaults, &[|h| h.title("Other")]);
    assert!(second.contains("<title>Other | Site</title>"));
    assert!(!second.contains("A &lt;"));
}
#[test]
fn explicit_social_clear_blocks_fallback_and_absolute_title_bypasses_template() {
    let config = config()
        .with_defaults(|h| {
            h.title_template("%s | Site")?;
            h.open_graph(OpenGraph::new().title("Inherited"))
        })
        .unwrap();
    let html = resolve(
        config,
        &[|h| {
            h.absolute_title("Absolute")?;
            h.open_graph(OpenGraph::default().clear_title())
        }],
    );
    assert!(html.contains("<title>Absolute</title>"));
    assert!(!html.contains("og:title"));
}
#[test]
fn malformed_templates_and_invalid_graph_patches_leave_previous_fields_intact() {
    let html = resolve(
        config(),
        &[
            |h| {
                h.title_template("%s - valid")?;
                assert!(h.title_template("no placeholder").is_err());
                assert!(h.title_template("%s %s").is_err());
                h.open_graph(OpenGraph::new().title("Original"))?;
                assert!(h
                    .open_graph(OpenGraph::new().title("Changed").url("javascript:evil"))
                    .is_err());
                Ok(())
            },
            |h| h.title("Child"),
        ],
    );
    assert!(html.contains("<title>Child - valid</title>"));
    assert!(html.contains("content=\"Original\""));
    assert!(!html.contains("Changed"));
}
#[test]
fn media_replacement_does_not_retain_dimensions_and_group_disable_is_explicit() {
    let defaults = config()
        .with_defaults(|h| {
            h.open_graph(
                OpenGraph::new()
                    .images([HeadMedia::new("/old.png").dimensions(1200, 630).alt("old")]),
            )
        })
        .unwrap();
    let html = resolve(
        defaults.clone(),
        &[|h| h.open_graph(OpenGraph::default().images([HeadMedia::new("/new.png").alt("new")]))],
    );
    assert!(html.contains("https://example.test/shop/new.png"));
    assert!(!html.contains("old"));
    assert!(!html.contains("og:image:width"));
    let disabled = resolve(
        defaults,
        &[|h| h.open_graph(OpenGraph::default().enabled(false))],
    );
    assert!(!disabled.contains("og:"));
}
#[test]
fn explicit_clears_and_resource_tombstones_survive_defaults() {
    #[derive(PartialEq, Eq)]
    enum Script {
        Site,
        Analytics,
    }
    let defaults = config()
        .with_defaults(|h| {
            h.title("Default")?;
            h.description("Default")?;
            h.canonical("/")?;
            h.remove_module(Script::Site)?;
            h.module(Script::Analytics, "/a.js")
        })
        .unwrap();
    let html = resolve(
        defaults,
        &[|h| {
            h.clear_title()?;
            h.clear_description()?;
            h.clear_canonical()?;
            h.module(Script::Site, "/site.js")
        }],
    );
    assert!(!html.contains("<title>"));
    assert!(!html.contains("description"));
    assert!(!html.contains("canonical"));
    assert!(html.find("site.js").unwrap() < html.find("a.js").unwrap());
}
