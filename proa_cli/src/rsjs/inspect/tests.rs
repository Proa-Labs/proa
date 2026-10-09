// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use super::*;

fn minimal_sidecar(analysis_id: &str, struct_name: &str, struct_path: &str, extra: &str) -> String {
    format!(
        r#"{{
                "format":"rsjs.strategy",
                "strategy_version":1,
                "strategy_mode":"manifest",
                "analysis_id":"{analysis_id}",
                "runtime_asset_id":null,
                "struct_name":"{struct_name}",
                "struct_path":"{struct_path}",
                "render_strategy":"server_only",
                {extra}
                "roots":[],
                "emitted":{{
                    "wrapper":false,
                    "seed_script":false,
                    "manifest_registration":false,
                    "js_asset":false,
                    "component_fragment_asset":false,
                    "uses_queries":false,
                    "uses_mutations":false
                }}
            }}"#
    )
}

const WIDGET_SOURCE: &str = r#"use proa_macros::rsjs;

pub struct Widget;

#[rsjs(component)]
impl<B: WriteBuf> WebRenderSync<::proa_core::NoLoader, B> for Widget {
    fn render(self, _cx: &mut WebContext<::proa_core::NoLoader, B>) -> Result<(), WriteError> {
        Ok(())
    }
}
"#;

fn widget_fingerprint() -> String {
    let file = syn::parse_file(WIDGET_SOURCE).unwrap();
    let item_impl = file
        .items
        .iter()
        .find_map(|item| match item {
            syn::Item::Impl(item_impl) => Some(item_impl),
            _ => None,
        })
        .unwrap();
    crate::lint::rsjs_input_fingerprint_for_impl(item_impl).unwrap()
}

#[test]
fn ambiguous_selector_lists_candidates() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("target").join("rsjs").join("strategy");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("widget_a.json"),
        minimal_sidecar("widget_a", "Widget", "pages::a::Widget", ""),
    )
    .unwrap();
    std::fs::write(
        dir.join("widget_b.json"),
        minimal_sidecar("widget_b", "Widget", "pages::b::Widget", ""),
    )
    .unwrap();

    let error = explain_strategy(tmp.path(), "Widget")
        .unwrap_err()
        .to_string();
    assert!(error.contains("matches 2"), "{error}");
    assert!(error.contains("pages::a::Widget"), "{error}");
    assert!(error.contains("pages::b::Widget"), "{error}");
}

#[test]
fn explain_reports_stale_sidecar_when_source_changed() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("target").join("rsjs").join("strategy");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::create_dir_all(tmp.path().join("src")).unwrap();
    std::fs::write(tmp.path().join("src/widget.rs"), WIDGET_SOURCE).unwrap();
    let extra = r#""input_fingerprint":"sha256:0000","source":{"file":"src/widget.rs","impl_start_line":5,"impl_start_column":1},"#;
    std::fs::write(
        dir.join("widget.json"),
        minimal_sidecar("widget", "Widget", "Widget", extra),
    )
    .unwrap();

    let explanation = explain_strategy(tmp.path(), "widget").unwrap();
    let staleness = explanation.staleness.expect("expected staleness warning");
    assert!(staleness.contains("changed since"), "{staleness}");
}

#[test]
fn explain_reports_fresh_sidecar_as_current() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("target").join("rsjs").join("strategy");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::create_dir_all(tmp.path().join("src")).unwrap();
    std::fs::write(tmp.path().join("src/widget.rs"), WIDGET_SOURCE).unwrap();
    let extra = format!(
        r#""input_fingerprint":"{}","source":{{"file":"src/widget.rs","impl_start_line":5,"impl_start_column":1}},"#,
        widget_fingerprint()
    );
    std::fs::write(
        dir.join("widget.json"),
        minimal_sidecar("widget", "Widget", "Widget", &extra),
    )
    .unwrap();

    let explanation = explain_strategy(tmp.path(), "widget").unwrap();
    assert_eq!(explanation.staleness, None);
}

#[test]
fn explain_reports_missing_anchor_file_as_stale() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("target").join("rsjs").join("strategy");
    std::fs::create_dir_all(&dir).unwrap();
    let extra = r#""input_fingerprint":"sha256:0000","source":{"file":"src/gone.rs","impl_start_line":5,"impl_start_column":1},"#;
    std::fs::write(
        dir.join("widget.json"),
        minimal_sidecar("widget", "Widget", "Widget", extra),
    )
    .unwrap();

    let explanation = explain_strategy(tmp.path(), "widget").unwrap();
    let staleness = explanation.staleness.expect("expected staleness warning");
    assert!(staleness.contains("was not found"), "{staleness}");
}

#[test]
fn explains_server_only_sidecar_by_analysis_id() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("target").join("rsjs").join("strategy");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
            dir.join("panel_1234.json"),
            r#"{
                "format":"rsjs.strategy",
                "strategy_version":1,
                "strategy_mode":"manifest",
                "analysis_id":"panel_1234",
                "runtime_asset_id":null,
                "struct_name":"Panel",
                "struct_path":"Panel",
                "render_strategy":"server_only",
                "client_origin_permission":"participant_only",
                "hydration_strategy":{"kind":"visible","label":"visible"},
                "server_surfaces":[
                    {"kind":"server_attr","node":1,"attr":"aria-label","source":"explicit_boundary","reason":"explicit SSR boundary"},
                    {"kind":"server_slot","node":2,"source":"explicit_simple_ssr_boundary","reason":"explicit wrapper around simple SSR projection","source_anchor":{"start_line":8,"start_column":22}},
                    {"kind":"server_slot","node":3,"source":"inferred_simple_ssr","reason":"inferred simple SSR projection"}
                ],
                "roots":[],
                "emitted":{
                    "wrapper":false,
                    "seed_script":false,
                    "manifest_registration":false,
                    "js_asset":false,
                    "component_fragment_asset":false,
                    "uses_queries":false,
                    "uses_mutations":false
                }
            }"#,
        )
        .unwrap();

    let explanation = explain_strategy(tmp.path(), "panel_1234").unwrap();
    assert_eq!(explanation.render_strategy, "ServerOnly");
    assert_eq!(
        explanation.client_origin_permission.as_deref(),
        Some("participant_only")
    );
    assert!(explanation
        .next_actions
        .iter()
        .any(|action| { action.contains("change to #[rsjs(component, client)]") }));
    assert!(explanation.roots.is_empty());
    assert!(explanation.declared_client_modules.is_empty());
    assert!(explanation.client_modules.is_empty());
    assert!(explanation.unused_client_modules.is_empty());
    assert!(explanation.runtime_features.is_empty());
    assert!(explanation.crossings.is_empty());
    assert_eq!(explanation.server_surfaces.len(), 3);
    assert_eq!(explanation.server_surfaces[0].kind, "server_attr");
    assert_eq!(
        explanation.server_surfaces[0].source.as_deref(),
        Some("explicit_boundary")
    );
    assert!(
        explanation.server_surfaces[0].detail.contains("aria-label"),
        "{explanation:#?}"
    );
    assert_eq!(explanation.server_surfaces[1].kind, "server_slot");
    assert_eq!(
        explanation.server_surfaces[1].source.as_deref(),
        Some("explicit_simple_ssr_boundary")
    );
    assert_eq!(
        explanation.server_surfaces[1].source_anchor,
        Some(StrategyServerSurfaceAnchor {
            start_line: 8,
            start_column: 22
        })
    );
    assert!(
        explanation.server_surfaces[1]
            .detail
            .contains("explicit wrapper around simple SSR projection"),
        "{explanation:#?}"
    );
    assert_eq!(
        explanation.server_surfaces[2].source.as_deref(),
        Some("inferred_simple_ssr")
    );
    assert!(!explanation.emitted.js_asset);
    assert_eq!(explanation.source.runtime_asset_id, None);
    let hydration = explanation
        .hydration_strategy
        .expect("hydration strategy should be explained");
    assert_eq!(hydration.label, "visible");
    assert!(hydration.ignored);
}

#[test]
fn explains_dead_client_primitives() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("target").join("rsjs").join("strategy");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
            dir.join("panel_1234.json"),
            r#"{
                "format":"rsjs.strategy",
                "strategy_version":1,
                "strategy_mode":"manifest",
                "analysis_id":"panel_1234",
                "runtime_asset_id":null,
                "struct_name":"Panel",
                "struct_path":"Panel",
                "render_strategy":"server_only",
                "dead_primitives":[
                    {"kind":"signal","signal":0,"label":"count","source_anchor":{"start_line":7,"start_column":21}},
                    {"kind":"memo","signal":1,"label":"total","source_anchor":{"start_line":8,"start_column":25}},
                    {"kind":"handler","handler":2,"label":"on_save","source_anchor":{"start_line":9,"start_column":25}},
                    {"kind":"node_ref","node_ref":3,"label":"input_ref","source_anchor":{"start_line":10,"start_column":25}},
                    {"kind":"query","query":4,"label":"items","source_anchor":{"start_line":11,"start_column":25}},
                    {"kind":"mutation","mutation":5,"label":"save"}
                ],
                "roots":[],
                "emitted":{
                    "wrapper":false,
                    "seed_script":false,
                    "manifest_registration":false,
                    "js_asset":false,
                    "component_fragment_asset":false,
                    "uses_queries":false,
                    "uses_mutations":false
                }
            }"#,
        )
        .unwrap();

    let explanation = explain_strategy(tmp.path(), "panel_1234").unwrap();
    assert_eq!(explanation.dead_primitives.len(), 6);
    assert_eq!(explanation.dead_primitives[0].kind, "signal");
    assert_eq!(
        explanation.dead_primitives[0].label.as_deref(),
        Some("count")
    );
    assert_eq!(explanation.dead_primitives[0].signal, Some(0));
    assert_eq!(
        explanation.dead_primitives[0].source_anchor,
        Some(StrategyDeadPrimitiveAnchor {
            start_line: 7,
            start_column: 21,
        })
    );
    assert!(explanation.dead_primitives[0]
        .detail
        .contains("was declared but no reachable client root"));
    assert_eq!(explanation.dead_primitives[1].kind, "memo");
    assert_eq!(explanation.dead_primitives[1].signal, Some(1));
    assert!(explanation.dead_primitives[1]
        .detail
        .contains("memo signal 1"));
    assert_eq!(explanation.dead_primitives[2].kind, "handler");
    assert_eq!(explanation.dead_primitives[2].handler, Some(2));
    assert_eq!(explanation.dead_primitives[3].kind, "node_ref");
    assert_eq!(explanation.dead_primitives[3].node_ref, Some(3));
    assert_eq!(explanation.dead_primitives[4].kind, "query");
    assert_eq!(explanation.dead_primitives[5].kind, "mutation");
    assert!(explanation.crossings.is_empty());
}

#[test]
fn explains_server_only_declared_client_modules_as_unused_capabilities() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("target").join("rsjs").join("strategy");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("price_1234.json"),
        r#"{
                "format":"rsjs.strategy",
                "strategy_version":1,
                "strategy_mode":"manifest",
                "analysis_id":"price_1234",
                "runtime_asset_id":null,
                "struct_name":"PriceLabel",
                "struct_path":"crate :: PriceLabel",
                "render_strategy":"server_only",
                "declared_client_modules":["crate::money"],
                "client_modules":[],
                "dead_primitives":[
                    {"kind":"signal","signal":0,"label":"price"}
                ],
                "roots":[],
                "emitted":{
                    "wrapper":false,
                    "seed_script":false,
                    "manifest_registration":false,
                    "js_asset":false,
                    "component_fragment_asset":false,
                    "uses_queries":false,
                    "uses_mutations":false
                }
            }"#,
    )
    .unwrap();

    let explanation = explain_strategy(tmp.path(), "PriceLabel").unwrap();

    assert_eq!(explanation.render_strategy, "ServerOnly");
    assert_eq!(
        explanation.declared_client_modules,
        vec!["crate::money".to_string()]
    );
    assert!(explanation.client_modules.is_empty());
    assert_eq!(
        explanation.unused_client_modules,
        vec!["crate::money".to_string()]
    );
    assert!(explanation.runtime_features.is_empty());
    assert!(!explanation.emitted.js_asset);
    assert!(
        explanation
            .summary
            .contains("grouping metadata is unreachable here; grouping never forces hydration"),
        "{}",
        explanation.summary
    );
    assert!(
        explanation
            .summary
            .contains("declarations alone do not force hydration"),
        "{}",
        explanation.summary
    );
    assert!(explanation.next_actions.iter().any(|action| {
        action.contains("annotate each portable origin-free helper once with #[rsjs::client]")
            && !action.contains("#[rsjs(clients(...))]")
    }));
    assert!(explanation
        .next_actions
        .iter()
        .any(|action| { action.contains("remove dead client primitive declarations") }));
}

#[test]
fn sidecar_summaries_include_redacted_server_surfaces() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("target").join("rsjs").join("strategy");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
            dir.join("panel_1234.json"),
            r#"{
                "format":"rsjs.strategy",
                "strategy_version":1,
                "strategy_mode":"manifest",
                "analysis_id":"panel_1234",
                "runtime_asset_id":null,
                "struct_name":"Panel",
                "struct_path":"Panel",
                "render_strategy":"server_only",
                "server_surfaces":[
                    {"kind":"server_attr","node":1,"attr":"aria-label","source":"explicit_boundary","reason":"explicit SSR boundary"},
                    {"kind":"server_slot","node":2,"source":"explicit_simple_ssr_boundary","reason":"explicit wrapper around simple SSR projection","source_anchor":{"start_line":8,"start_column":22}},
                    {"kind":"server_slot","node":3,"source":"inferred_simple_ssr","reason":"inferred simple SSR projection"}
                ],
                "roots":[],
                "emitted":{
                    "wrapper":false,
                    "seed_script":false,
                    "manifest_registration":false,
                    "js_asset":false,
                    "component_fragment_asset":false,
                    "uses_queries":false,
                    "uses_mutations":false
                }
            }"#,
        )
        .unwrap();

    let summaries = strategy_sidecar_summaries(tmp.path());
    let summary = summaries
        .iter()
        .find(|summary| summary.analysis_id == "panel_1234")
        .expect("summary should include the test sidecar");

    assert_eq!(summary.server_surfaces.len(), 3);
    assert_eq!(summary.server_surfaces[0].kind, "server_attr");
    assert_eq!(
        summary.server_surfaces[0].attr.as_deref(),
        Some("aria-label")
    );
    assert_eq!(
        summary.server_surfaces[1].source.as_deref(),
        Some("explicit_simple_ssr_boundary")
    );
    assert_eq!(summary.server_surfaces[1].node, Some(2));
    assert_eq!(
        summary.server_surfaces[1].source_anchor,
        Some(StrategyServerSurfaceAnchor {
            start_line: 8,
            start_column: 22
        })
    );
    assert_eq!(
        summary.server_surfaces[2].source.as_deref(),
        Some("inferred_simple_ssr")
    );
}

#[test]
fn crossing_report_lists_redacted_sidecar_crossings() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("target").join("rsjs").join("strategy");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
            dir.join("price_1234.json"),
            r#"{
                "format":"rsjs.strategy",
                "strategy_version":1,
                "strategy_mode":"manifest",
                "analysis_id":"price_1234",
                "runtime_asset_id":"price_1234",
                "struct_name":"Price",
                "struct_path":"crate :: Price",
                "render_strategy":"ssr_hydrate",
                "crossings":[
                    {"kind":"query_key","query":2,"label":"price","surface":"query_metadata_blob","requirement":"query_codec","source_anchor":{"start_line":9,"start_column":25}},
                    {"kind":"query_fetcher","query":2,"label":"price.fetcher","surface":"query_metadata_blob","requirement":"request_config","source_anchor":{"start_line":9,"start_column":25}}
                ],
                "roots":[{"kind":"query","id":2}],
                "emitted":{
                    "wrapper":true,
                    "seed_script":true,
                    "manifest_registration":true,
                    "js_asset":true,
                    "component_fragment_asset":true,
                    "uses_queries":true,
                    "uses_mutations":false
                }
            }"#,
        )
        .unwrap();
    std::fs::write(
        dir.join("panel_1234.json"),
        r#"{
                "format":"rsjs.strategy",
                "strategy_version":1,
                "strategy_mode":"manifest",
                "analysis_id":"panel_1234",
                "runtime_asset_id":null,
                "struct_name":"Panel",
                "struct_path":"Panel",
                "render_strategy":"server_only",
                "crossings":[],
                "roots":[],
                "emitted":{
                    "wrapper":false,
                    "seed_script":false,
                    "manifest_registration":false,
                    "js_asset":false,
                    "component_fragment_asset":false,
                    "uses_queries":false,
                    "uses_mutations":false
                }
            }"#,
    )
    .unwrap();

    let report = crossing_report(tmp.path(), None).unwrap();
    assert_eq!(report.sidecar_count, 2);
    assert_eq!(report.crossing_count, 2);
    assert_eq!(report.crossings[0].analysis_id, "price_1234");
    assert_eq!(report.crossings[0].struct_path, "crate :: Price");
    assert_eq!(report.crossings[0].crossing.kind, "query_key");
    assert_eq!(report.crossings[1].crossing.kind, "query_fetcher");
    assert_eq!(
        report.crossings[1].crossing.requirement.as_deref(),
        Some("request_config")
    );

    let selected = crossing_report(tmp.path(), Some("Price")).unwrap();
    assert_eq!(selected.sidecar_count, 1);
    assert_eq!(selected.crossing_count, 2);
    assert_eq!(selected.selector.as_deref(), Some("Price"));
    let json = serde_json::to_string(&selected).unwrap();
    assert!(json.contains("\"kind\":\"query_fetcher\""));
    assert!(!json.contains("/api/price"));
}

#[test]
fn explains_hydrated_sidecar_by_struct_name() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("target").join("rsjs").join("strategy");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
            dir.join("counter_1234.json"),
            r#"{
                "format":"rsjs.strategy",
                "strategy_version":1,
                "strategy_mode":"manifest",
                "analysis_id":"counter_1234",
                "runtime_asset_id":"counter_1234",
                "struct_name":"Counter",
                "struct_path":"crate :: Counter",
                "render_strategy":"ssr_hydrate",
                "hydration_strategy":{"kind":"interaction","event":"click"},
                "declared_client_modules":["crate::money","crate::dates"],
                "client_modules":["crate::money"],
                "runtime_features":["query","mutation","shared_state"],
                "shared_state_dependencies":[
                    {"store":0,"rust_type":"crate::WorkbenchState","fields":["button_bg","mode"],"source":{"start_line":6,"start_column":17}}
                ],
                "crossings":[
                    {"kind":"signal_initial_value","signal":0,"label":"count","surface":"signal_seed","requirement":"client_encode","source_anchor":{"start_line":7,"start_column":21}},
                    {"kind":"handler_payload","node":3,"attr":"data-rsjs-payload-p0","label":"day","surface":"handler_payload_attr","requirement":"client_encode","source_anchor":{"start_line":8,"start_column":35}},
                    {"kind":"node_payload","node":4,"attr":"data-rsjs-payload-p1","label":"item.label","surface":"node_payload_attr","requirement":"client_encode","source_anchor":{"start_line":8,"start_column":45}},
                    {"kind":"query_key","query":2,"label":"price","surface":"query_metadata_blob","requirement":"query_codec","source_anchor":{"start_line":9,"start_column":25}},
                    {"kind":"query_fetcher","query":2,"label":"price.fetcher","surface":"query_metadata_blob","requirement":"request_config","source_anchor":{"start_line":9,"start_column":25}},
                    {"kind":"query_initial_value","query":2,"label":"price","surface":"query_seed_blob","requirement":"query_codec","source_anchor":{"start_line":9,"start_column":25}},
                    {"kind":"mutation_key","mutation":4,"label":"save","surface":"mutation_metadata_blob","requirement":"mutation_codec","source_anchor":{"start_line":10,"start_column":25}},
                    {"kind":"mutation_runner","mutation":4,"label":"save.runner","surface":"mutation_metadata_blob","requirement":"request_config","source_anchor":{"start_line":10,"start_column":25}},
                    {"kind":"mutation_invalidation_key","mutation":4,"invalidation_index":0,"label":"save.invalidates[0]","surface":"mutation_metadata_blob","requirement":"mutation_codec","source_anchor":{"start_line":10,"start_column":25}},
                    {"kind":"descriptor_reactive_prop","asset_id":"child_fragment","prop":"name","root":6,"child_signal":8,"parent_signal":0,"label":"child_fragment::name","surface":"descriptor_reactive_prop","requirement":"client_encode","source_anchor":{"start_line":11,"start_column":33}}
                ],
                "roots":[
                    {"kind":"event_binding","node":1,"event":"on_click","dom_event":"click"},
                    {"kind":"text_binding","node":2},
                    {"kind":"signal","id":0},
                    {"kind":"query","id":2},
                    {"kind":"mutation","id":4},
                    {"kind":"node_ref","id":1},
                    {"kind":"browser_read","read":"location.pathname"},
                    {"kind":"browser_effect","effect":"clipboard.write_text"},
                    {"kind":"client_module_call","module":"crate::money","name":"format_usd"},
                    {"kind":"component_fragment_mount","root":6,"asset_id":"child_fragment"}
                ],
                "emitted":{
                    "wrapper":true,
                    "seed_script":true,
                    "manifest_registration":true,
                    "js_asset":true,
                    "component_fragment_asset":true,
                    "uses_queries":true,
                    "uses_mutations":true,
                    "uses_shared_state":true
                }
            }"#,
        )
        .unwrap();

    let explanation = explain_strategy(tmp.path(), "Counter").unwrap();
    assert_eq!(explanation.render_strategy, "SSR + Hydrate");
    assert_eq!(
        explanation.declared_client_modules,
        vec!["crate::money".to_string(), "crate::dates".to_string()]
    );
    assert_eq!(explanation.client_modules, vec!["crate::money".to_string()]);
    assert_eq!(
        explanation.unused_client_modules,
        vec!["crate::dates".to_string()]
    );
    assert_eq!(
        explanation.component_fragment_imports,
        vec!["child_fragment".to_string()]
    );
    assert_eq!(
        explanation.runtime_features,
        vec![
            "query".to_string(),
            "mutation".to_string(),
            "shared_state".to_string()
        ]
    );
    assert_eq!(
        explanation.shared_state_dependencies,
        vec![StrategySharedStateDependency {
            store: 0,
            rust_type: "crate::WorkbenchState".to_string(),
            type_key: None,
            fields: vec!["button_bg".to_string(), "mode".to_string()],
            serialized_fields: Vec::new(),
            provider_serializes_full_snapshot: false,
            source_anchor: Some(StrategyClientCrossingAnchor {
                start_line: 6,
                start_column: 17,
            }),
        }]
    );
    assert!(explanation.emitted.uses_shared_state);
    assert_eq!(explanation.crossings.len(), 10);
    assert!(
            explanation
                .crossings
                .iter()
                .all(|crossing| crossing.kind != "client_helper_argument"),
            "client helper calls are roots/import edges; their data is reported by the underlying crossing surfaces: {explanation:#?}"
        );
    assert_eq!(explanation.crossings[0].kind, "signal_initial_value");
    assert_eq!(explanation.crossings[0].label.as_deref(), Some("count"));
    assert_eq!(explanation.crossings[0].signal, Some(0));
    assert_eq!(
        explanation.crossings[0].source_anchor,
        Some(StrategyClientCrossingAnchor {
            start_line: 7,
            start_column: 21
        })
    );
    assert!(explanation.crossings[0]
        .detail
        .contains("signal initial value crosses"));
    assert_eq!(explanation.crossings[1].kind, "handler_payload");
    assert_eq!(explanation.crossings[1].node, Some(3));
    assert_eq!(
        explanation.crossings[1].attr.as_deref(),
        Some("data-rsjs-payload-p0")
    );
    assert_eq!(explanation.crossings[1].label.as_deref(), Some("day"));
    assert!(explanation.crossings[1]
        .detail
        .contains("handler payload crosses"));
    assert_eq!(explanation.crossings[2].kind, "node_payload");
    assert_eq!(explanation.crossings[2].node, Some(4));
    assert_eq!(
        explanation.crossings[2].attr.as_deref(),
        Some("data-rsjs-payload-p1")
    );
    assert_eq!(
        explanation.crossings[2].label.as_deref(),
        Some("item.label")
    );
    assert_eq!(
        explanation.crossings[2].surface.as_deref(),
        Some("node_payload_attr")
    );
    assert!(explanation.crossings[2]
        .detail
        .contains("node payload crosses"));
    assert_eq!(explanation.crossings[3].kind, "query_key");
    assert_eq!(explanation.crossings[3].query, Some(2));
    assert_eq!(explanation.crossings[3].label.as_deref(), Some("price"));
    assert_eq!(
        explanation.crossings[3].surface.as_deref(),
        Some("query_metadata_blob")
    );
    assert_eq!(
        explanation.crossings[3].requirement.as_deref(),
        Some("query_codec")
    );
    assert!(explanation.crossings[3]
        .detail
        .contains("query key crosses"));
    assert_eq!(explanation.crossings[4].kind, "query_fetcher");
    assert_eq!(explanation.crossings[4].query, Some(2));
    assert_eq!(
        explanation.crossings[4].label.as_deref(),
        Some("price.fetcher")
    );
    assert_eq!(
        explanation.crossings[4].surface.as_deref(),
        Some("query_metadata_blob")
    );
    assert_eq!(
        explanation.crossings[4].requirement.as_deref(),
        Some("request_config")
    );
    assert!(explanation.crossings[4]
        .detail
        .contains("query fetcher config crosses"));
    assert_eq!(explanation.crossings[5].kind, "query_initial_value");
    assert_eq!(explanation.crossings[5].query, Some(2));
    assert_eq!(explanation.crossings[5].label.as_deref(), Some("price"));
    assert_eq!(
        explanation.crossings[5].surface.as_deref(),
        Some("query_seed_blob")
    );
    assert_eq!(
        explanation.crossings[5].requirement.as_deref(),
        Some("query_codec")
    );
    assert!(explanation.crossings[5]
        .detail
        .contains("query initial value crosses"));
    assert_eq!(explanation.crossings[6].kind, "mutation_key");
    assert_eq!(explanation.crossings[6].mutation, Some(4));
    assert_eq!(explanation.crossings[6].label.as_deref(), Some("save"));
    assert_eq!(
        explanation.crossings[6].surface.as_deref(),
        Some("mutation_metadata_blob")
    );
    assert_eq!(
        explanation.crossings[6].requirement.as_deref(),
        Some("mutation_codec")
    );
    assert!(explanation.crossings[6]
        .detail
        .contains("mutation key crosses"));
    assert_eq!(explanation.crossings[7].kind, "mutation_runner");
    assert_eq!(explanation.crossings[7].mutation, Some(4));
    assert_eq!(
        explanation.crossings[7].label.as_deref(),
        Some("save.runner")
    );
    assert_eq!(
        explanation.crossings[7].surface.as_deref(),
        Some("mutation_metadata_blob")
    );
    assert_eq!(
        explanation.crossings[7].requirement.as_deref(),
        Some("request_config")
    );
    assert!(explanation.crossings[7]
        .detail
        .contains("mutation runner config crosses"));
    assert_eq!(explanation.crossings[8].kind, "mutation_invalidation_key");
    assert_eq!(explanation.crossings[8].mutation, Some(4));
    assert_eq!(explanation.crossings[8].invalidation_index, Some(0));
    assert_eq!(
        explanation.crossings[8].label.as_deref(),
        Some("save.invalidates[0]")
    );
    assert_eq!(
        explanation.crossings[8].surface.as_deref(),
        Some("mutation_metadata_blob")
    );
    assert_eq!(
        explanation.crossings[8].requirement.as_deref(),
        Some("mutation_codec")
    );
    assert_eq!(
        explanation.crossings[8].source_anchor,
        Some(StrategyClientCrossingAnchor {
            start_line: 10,
            start_column: 25
        })
    );
    assert!(explanation.crossings[8]
        .detail
        .contains("mutation invalidation key 0 crosses"));
    assert_eq!(explanation.crossings[9].kind, "descriptor_reactive_prop");
    assert_eq!(
        explanation.crossings[9].asset_id.as_deref(),
        Some("child_fragment")
    );
    assert_eq!(explanation.crossings[9].prop.as_deref(), Some("name"));
    assert_eq!(explanation.crossings[9].root, Some(6));
    assert_eq!(explanation.crossings[9].child_signal, Some(8));
    assert_eq!(explanation.crossings[9].parent_signal, Some(0));
    assert_eq!(
        explanation.crossings[9].label.as_deref(),
        Some("child_fragment::name")
    );
    assert_eq!(
        explanation.crossings[9].surface.as_deref(),
        Some("descriptor_reactive_prop")
    );
    assert_eq!(
        explanation.crossings[9].source_anchor,
        Some(StrategyClientCrossingAnchor {
            start_line: 11,
            start_column: 33
        })
    );
    assert!(explanation.crossings[9]
        .detail
        .contains("descriptor reactive prop `name` crosses"));
    assert_eq!(explanation.roots.len(), 10);
    assert!(explanation.emitted.manifest_registration);
    assert!(
        explanation.roots[0].detail.contains("on_click"),
        "{explanation:#?}"
    );
    assert!(
        explanation
            .roots
            .iter()
            .any(|root| root.detail == "signal 0"),
        "{explanation:#?}"
    );
    assert!(
        explanation
            .roots
            .iter()
            .any(|root| root.detail == "query 2"),
        "{explanation:#?}"
    );
    assert!(
        explanation
            .roots
            .iter()
            .any(|root| root.detail == "mutation 4"),
        "{explanation:#?}"
    );
    assert!(
        explanation
            .roots
            .iter()
            .any(|root| root.detail == "node_ref 1"),
        "{explanation:#?}"
    );
    assert!(
        explanation
            .roots
            .iter()
            .any(|root| root.detail.contains("crate::money")),
        "{explanation:#?}"
    );
    assert!(
        explanation
            .roots
            .iter()
            .any(|root| root.detail == "browser read `location.pathname`"),
        "{explanation:#?}"
    );
    assert!(
        explanation
            .roots
            .iter()
            .any(|root| root.detail == "browser effect `clipboard.write_text`"),
        "{explanation:#?}"
    );
    assert!(
            explanation.roots.iter().any(|root| {
                root.detail
                    == "component fragment `child_fragment` mounted at root 6 through the schema-v4 parent-mounted fragment path"
            }),
            "{explanation:#?}"
        );
    let hydration = explanation
        .hydration_strategy
        .expect("hydration strategy should be explained");
    assert_eq!(hydration.label, "interaction:click");
    assert!(!hydration.ignored);

    let summaries = strategy_sidecar_summaries(tmp.path());
    let summary = summaries
        .iter()
        .find(|summary| summary.analysis_id == "counter_1234")
        .expect("summary should include hydrated sidecar");
    assert_eq!(
        summary.component_fragment_mounts,
        vec![StrategyFragmentMountSummary {
            root: 6,
            asset_id: "child_fragment".to_string(),
        }]
    );
    assert_eq!(
        summary.component_fragment_imports,
        vec!["child_fragment".to_string()]
    );
    assert_eq!(
        summary.shared_state_dependencies,
        vec![StrategySharedStateDependency {
            store: 0,
            rust_type: "crate::WorkbenchState".to_string(),
            type_key: None,
            fields: vec!["button_bg".to_string(), "mode".to_string()],
            serialized_fields: Vec::new(),
            provider_serializes_full_snapshot: false,
            source_anchor: Some(StrategyClientCrossingAnchor {
                start_line: 6,
                start_column: 17,
            }),
        }]
    );
    assert_eq!(summary.crossings.len(), 10);
    assert_eq!(summary.crossings[0].kind, "signal_initial_value");
    assert_eq!(summary.crossings[0].label.as_deref(), Some("count"));
    assert_eq!(summary.crossings[0].surface.as_deref(), Some("signal_seed"));
    assert_eq!(
        summary.crossings[0].source_anchor,
        Some(StrategyClientCrossingAnchor {
            start_line: 7,
            start_column: 21
        })
    );
    assert_eq!(summary.crossings[1].kind, "handler_payload");
    assert_eq!(summary.crossings[1].node, Some(3));
    assert_eq!(
        summary.crossings[1].attr.as_deref(),
        Some("data-rsjs-payload-p0")
    );
    assert_eq!(summary.crossings[2].kind, "node_payload");
    assert_eq!(summary.crossings[2].node, Some(4));
    assert_eq!(
        summary.crossings[2].attr.as_deref(),
        Some("data-rsjs-payload-p1")
    );
    assert_eq!(summary.crossings[2].label.as_deref(), Some("item.label"));
    assert_eq!(
        summary.crossings[2].surface.as_deref(),
        Some("node_payload_attr")
    );
    assert_eq!(summary.crossings[3].kind, "query_key");
    assert_eq!(summary.crossings[3].query, Some(2));
    assert_eq!(summary.crossings[3].label.as_deref(), Some("price"));
    assert_eq!(
        summary.crossings[3].surface.as_deref(),
        Some("query_metadata_blob")
    );
    assert_eq!(summary.crossings[4].kind, "query_fetcher");
    assert_eq!(summary.crossings[4].query, Some(2));
    assert_eq!(summary.crossings[4].label.as_deref(), Some("price.fetcher"));
    assert_eq!(
        summary.crossings[4].surface.as_deref(),
        Some("query_metadata_blob")
    );
    assert_eq!(summary.crossings[5].kind, "query_initial_value");
    assert_eq!(summary.crossings[5].query, Some(2));
    assert_eq!(summary.crossings[5].label.as_deref(), Some("price"));
    assert_eq!(summary.crossings[6].kind, "mutation_key");
    assert_eq!(summary.crossings[6].mutation, Some(4));
    assert_eq!(summary.crossings[6].label.as_deref(), Some("save"));
    assert_eq!(
        summary.crossings[6].surface.as_deref(),
        Some("mutation_metadata_blob")
    );
    assert_eq!(summary.crossings[7].kind, "mutation_runner");
    assert_eq!(summary.crossings[7].mutation, Some(4));
    assert_eq!(summary.crossings[7].label.as_deref(), Some("save.runner"));
    assert_eq!(
        summary.crossings[7].surface.as_deref(),
        Some("mutation_metadata_blob")
    );
    assert_eq!(summary.crossings[8].kind, "mutation_invalidation_key");
    assert_eq!(summary.crossings[8].mutation, Some(4));
    assert_eq!(summary.crossings[8].invalidation_index, Some(0));
    assert_eq!(
        summary.crossings[8].label.as_deref(),
        Some("save.invalidates[0]")
    );
    assert_eq!(
        summary.crossings[8].surface.as_deref(),
        Some("mutation_metadata_blob")
    );
    assert_eq!(
        summary.crossings[8].source_anchor,
        Some(StrategyClientCrossingAnchor {
            start_line: 10,
            start_column: 25
        })
    );
    assert_eq!(summary.crossings[9].kind, "descriptor_reactive_prop");
    assert_eq!(
        summary.crossings[9].asset_id.as_deref(),
        Some("child_fragment")
    );
    assert_eq!(summary.crossings[9].prop.as_deref(), Some("name"));
    assert_eq!(summary.crossings[9].root, Some(6));
    assert_eq!(summary.crossings[9].child_signal, Some(8));
    assert_eq!(summary.crossings[9].parent_signal, Some(0));
    assert_eq!(
        summary.crossings[9].surface.as_deref(),
        Some("descriptor_reactive_prop")
    );
    assert_eq!(
        summary.crossings[9].source_anchor,
        Some(StrategyClientCrossingAnchor {
            start_line: 11,
            start_column: 33
        })
    );
}

#[test]
fn explains_runtime_features_from_legacy_emitted_flags() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("target").join("rsjs").join("strategy");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("legacy_1234.json"),
        r#"{
                "format":"rsjs.strategy",
                "strategy_version":1,
                "strategy_mode":"manifest",
                "analysis_id":"legacy_1234",
                "runtime_asset_id":"legacy_1234",
                "struct_name":"Legacy",
                "struct_path":"Legacy",
                "render_strategy":"ssr_hydrate",
                "roots":[],
                "emitted":{
                    "wrapper":true,
                    "seed_script":true,
                    "manifest_registration":true,
                    "js_asset":true,
                    "component_fragment_asset":true,
                    "uses_queries":true,
                    "uses_mutations":true,
                    "uses_shared_state":true
                }
            }"#,
    )
    .unwrap();

    let explanation = explain_strategy(tmp.path(), "Legacy").unwrap();
    assert_eq!(
        explanation.runtime_features,
        vec![
            "query".to_string(),
            "mutation".to_string(),
            "shared_state".to_string()
        ]
    );
}

#[test]
fn explains_nested_module_shared_store_types_without_token_spacing() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("target").join("rsjs").join("strategy");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
            dir.join("theme_panel_1234.json"),
            r#"{
                "format":"rsjs.strategy",
                "strategy_version":1,
                "strategy_mode":"manifest",
                "analysis_id":"theme_panel_1234",
                "runtime_asset_id":"theme_panel_1234",
                "struct_name":"ThemePanel",
                "struct_path":"crate :: panels :: ThemePanel",
                "render_strategy":"ssr_hydrate",
                "runtime_features":["shared_state"],
                "shared_state_dependencies":[
                    {"store":0,"rust_type":"state :: WorkbenchState","fields":["button_bg","mode"],"source":{"start_line":12,"start_column":17}},
                    {"store":1,"rust_type":"state::WorkbenchState","fields":["mode"]},
                    {"store":2,"rust_type":"crate :: state :: Cart < Item >","fields":["items"]}
                ],
                "roots":[{"kind":"text_binding","node":1}],
                "emitted":{
                    "wrapper":true,
                    "seed_script":true,
                    "manifest_registration":true,
                    "js_asset":true,
                    "uses_shared_state":true
                }
            }"#,
        )
        .unwrap();

    let explanation = explain_strategy(tmp.path(), "ThemePanel").unwrap();
    assert_eq!(
        explanation.shared_state_dependencies[0].rust_type, "state::WorkbenchState",
        "token-stream spacing from the sidecar must be collapsed back to the \
             source spelling: {explanation:#?}"
    );
    assert_eq!(
        explanation.shared_state_dependencies[0].rust_type,
        explanation.shared_state_dependencies[1].rust_type,
        "the same store type spelled through the same path must compare equal \
             regardless of sidecar token spacing: {explanation:#?}"
    );
    assert_eq!(
        explanation.shared_state_dependencies[2].rust_type, "crate::state::Cart<Item>",
        "generic arguments must also lose token-stream spacing: {explanation:#?}"
    );

    let json = serde_json::to_string(&explanation).unwrap();
    assert!(
        json.contains("\"rust_type\":\"state::WorkbenchState\""),
        "--json output must carry the normalized type: {json}"
    );
    assert!(
        !json.contains("state :: WorkbenchState"),
        "--json output must not leak token-stream spacing: {json}"
    );

    let summaries = strategy_sidecar_summaries(tmp.path());
    let summary = summaries
        .iter()
        .find(|summary| summary.analysis_id == "theme_panel_1234")
        .expect("summary should include the shared-state sidecar");
    assert_eq!(
        summary.shared_state_dependencies[0].rust_type,
        "state::WorkbenchState"
    );
}
