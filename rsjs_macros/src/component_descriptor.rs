// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! JSON sidecar emission for reusable rsjs component metadata.
//!
//! The descriptor is intentionally a mechanical dump of the rsjs IR,
//! with numeric ids preserved. Parent-island inlining can then read
//! this file, allocate fresh ids in the parent, and remap every child
//! reference without trying to reconstruct structure from JS. Signal ids can
//! be sparse after dead-signal elimination, and handler ids can be sparse after
//! handler deduplication.

use quote::ToTokens;
use rsjs_ir::abi::CLIENT_IR_SCHEMA_VERSION;
use rsjs_ir::client::{
    parse_client_block_json, parse_client_expr_json, parse_client_type_json,
    write_client_block_json, write_client_expr_json, ClientBlock, ClientType,
};
use rsjs_ir::reactive::CallbackSlot;
use rsjs_ir::{
    AttrBinding, AttrBindingKind, Bindings, ConditionalSection, EventHandler, EventHandlerId,
    EventName, ExprArena, ExprId, InputBinding, InputKind, InvalidationStrategy, Island, ListDecl,
    ListItemExpr, ListSource, ListTemplateNode, MutationDecl, MutationId, MutationOptions, NodeId,
    NodeRefBinding, NodeRefDecl, NodeRefId, PayloadAttr, QueryDecl, QueryId, QueryOptions,
    ServerAttr, ServerOnlyNode, ServerSurfaceSource, SharedFieldRef, SharedStoreDecl,
    SharedStoreId, SignalDecl, SignalId, Value, ViewNode, ViewTree,
};
use sha2::{Digest, Sha256};
use std::fmt::Write as _;
use syn::visit::{self, Visit};

/// Version of the component descriptor JSON layout.
///
/// Version history:
/// - 1: pre-Client-IR-v2 transition format (no `client_ir_version` field).
/// - 2: handler bodies serialized as Client IR v2 blocks; the expression
///   arena still carried the legacy v1 expression shapes.
/// - 3: the expression arena entries are Client IR v2 expressions
///   (`rsjs_ir::client::ClientExpr` JSON); the legacy v1 expression IR is
///   gone.
/// - 4: query declarations carry `data_ty` so typed JSON projection metadata
///   survives component inlining.
/// - 5: source anchors and component-fragment metadata use the current
///   schema-v5 representation.
/// - 6: descriptors carry the component's explicit client-origin permission.
/// - 7: fields may declare a dedicated immutable, static-only server slot.
/// - 8: descriptors carry dense latent inbound field/value/prop-site/crossing
///   metadata with normative temporal modes and source anchors.
/// - 9: callback fields also carry a dense symbolic value ID and preserve
///   required (`Handler<T>`) versus optional (`Option<Handler<T>>`) shape.
pub(crate) const DESCRIPTOR_SCHEMA_VERSION: u64 = 9;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ComponentOriginPermission {
    ParticipantOnly,
    OriginCapable,
}

impl ComponentOriginPermission {
    const fn as_str(self) -> &'static str {
        match self {
            Self::ParticipantOnly => "participant_only",
            Self::OriginCapable => "origin_capable",
        }
    }

    fn parse(value: &serde_json::Value) -> Result<Self, String> {
        match value
            .get("client_origin_permission")
            .and_then(serde_json::Value::as_str)
        {
            Some("participant_only") => Ok(Self::ParticipantOnly),
            Some("origin_capable") => Ok(Self::OriginCapable),
            Some(other) => Err(format!(
                "unsupported client_origin_permission `{other}`; expected \
                 `participant_only` or `origin_capable`"
            )),
            None => Err("missing string client_origin_permission".to_string()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ComponentDescriptorSummary {
    pub path: std::path::PathBuf,
    pub asset_id: String,
    pub struct_name: String,
    pub self_ty: String,
    pub is_async: bool,
    pub client_origin_permission: ComponentOriginPermission,
    pub handler_fields: Vec<String>,
    pub prop_fields: Vec<String>,
    pub server_prop_fields: Vec<String>,
    pub static_slot_fields: Vec<String>,
    pub js_hash: String,
    pub descriptor_hash: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ComponentPropField {
    pub name: String,
    pub signal: SignalId,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ComponentDescriptorLookup {
    Found(ComponentDescriptorSummary),
    Missing,
    Ambiguous(Vec<ComponentDescriptorSummary>),
    Invalid {
        path: std::path::PathBuf,
        reason: String,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum ComponentDescriptorFullLookup {
    Found(Box<ComponentDescriptor>),
    Missing,
    Ambiguous(Vec<ComponentDescriptorSummary>),
    Invalid {
        path: std::path::PathBuf,
        reason: String,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ComponentDescriptor {
    pub summary: ComponentDescriptorSummary,
    pub island: Island,
    pub prop_fields: Vec<ComponentPropField>,
    pub inbound_inputs: ParsedInboundInputs,
    pub query_initializers: Vec<ComponentQueryInitializer>,
    pub mutation_initializers: Vec<ComponentMutationInitializer>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct ParsedInboundInputs {
    pub fields: Vec<ParsedInboundField>,
    pub values: Vec<ParsedInboundValue>,
    pub prop_sites: Vec<ParsedInboundPropSite>,
    pub crossings: Vec<ParsedInboundCrossing>,
    pub node_ref_fields: Vec<ParsedInboundNodeRefField>,
    pub projection_fields: Vec<ParsedInboundProjectionField>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ParsedInboundField {
    pub id: u32,
    pub name: String,
    pub role: crate::analyze::InboundFieldRole,
    pub value: Option<u32>,
    pub callback_shape: Option<crate::analyze::InboundCallbackShape>,
    pub crossing: u32,
    pub source_anchor: rsjs_ir::SourceAnchor,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ParsedInboundValue {
    pub id: u32,
    pub field: u32,
    pub source_anchor: rsjs_ir::SourceAnchor,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ParsedInboundPropSite {
    pub id: u32,
    pub field: u32,
    pub value: u32,
    pub crossing: u32,
    pub temporal: crate::analyze::InboundTemporalMode,
    pub target: crate::analyze::InboundPropTarget,
    pub source_anchor: rsjs_ir::SourceAnchor,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ParsedInboundCrossing {
    pub id: u32,
    pub field: u32,
    pub kind: crate::analyze::InboundCrossingKind,
    pub source_anchor: rsjs_ir::SourceAnchor,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ParsedInboundNodeRefField {
    pub field: u32,
    pub value: u32,
    pub crossing: u32,
    pub node_ref: NodeRefId,
    pub bind_target: Option<crate::analyze::NodeRefBindTarget>,
    pub source_anchor: rsjs_ir::SourceAnchor,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ParsedInboundProjectionField {
    pub field: u32,
    pub value: u32,
    pub crossing: u32,
    pub signal: SignalId,
    pub node_ref: NodeRefId,
    pub source_anchor: rsjs_ir::SourceAnchor,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ComponentQueryInitializer {
    pub id: QueryId,
    pub expr: syn::Expr,
    pub key_expr: Option<syn::Expr>,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ComponentMutationInitializer {
    pub id: MutationId,
    pub key_expr: syn::Expr,
    pub invalidates_expr: Option<syn::Expr>,
}

pub(crate) fn find_by_component_path(path: &syn::Path) -> ComponentDescriptorLookup {
    match find_full_by_component_path(path) {
        ComponentDescriptorFullLookup::Found(descriptor) => {
            ComponentDescriptorLookup::Found(descriptor.summary)
        }
        ComponentDescriptorFullLookup::Missing => ComponentDescriptorLookup::Missing,
        ComponentDescriptorFullLookup::Ambiguous(matches) => {
            ComponentDescriptorLookup::Ambiguous(matches)
        }
        ComponentDescriptorFullLookup::Invalid { path, reason } => {
            ComponentDescriptorLookup::Invalid { path, reason }
        }
    }
}

pub(crate) fn find_full_by_component_path(path: &syn::Path) -> ComponentDescriptorFullLookup {
    let Some(last) = path
        .segments
        .last()
        .map(|segment| segment.ident.to_string())
    else {
        return ComponentDescriptorFullLookup::Missing;
    };
    let canonical_path = canonical_path(path);
    let mut matches = Vec::new();
    let mut stale_version: Option<(std::path::PathBuf, String)> = None;

    for dir in descriptor_dirs() {
        let entries = match std::fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(_) => continue,
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
                continue;
            }
            let text = match std::fs::read_to_string(&path) {
                Ok(text) => text,
                Err(err) => {
                    return ComponentDescriptorFullLookup::Invalid {
                        path,
                        reason: err.to_string(),
                    };
                }
            };
            // Descriptors from other compiler versions linger in shared
            // target dirs (their file names embed a content hash, so a
            // rebuild writes a sibling file instead of overwriting). Skip
            // them here so one stale sidecar cannot poison every component
            // lookup, but remember stale descriptors for the requested
            // component: if it resolves to nothing else, the version error
            // is the answer.
            match stale_descriptor_schema_version(&text, &last, &canonical_path) {
                Some(Some(reason)) => {
                    if stale_version.is_none() {
                        stale_version = Some((path, reason));
                    }
                    continue;
                }
                Some(None) => continue,
                None => {}
            }
            let descriptor = match parse_descriptor_json(path.clone(), &text) {
                Ok(Some(descriptor)) => descriptor,
                Ok(None) => continue,
                Err(reason) => return ComponentDescriptorFullLookup::Invalid { path, reason },
            };
            if descriptor_matches(&descriptor.summary, &last, &canonical_path) {
                matches.push(descriptor);
            }
        }
    }

    matches.sort_by(|a, b| {
        (
            a.summary.struct_name.as_str(),
            a.summary.self_ty.as_str(),
            a.summary.asset_id.as_str(),
            a.summary.path.as_os_str(),
        )
            .cmp(&(
                b.summary.struct_name.as_str(),
                b.summary.self_ty.as_str(),
                b.summary.asset_id.as_str(),
                b.summary.path.as_os_str(),
            ))
    });
    matches.dedup_by(|a, b| {
        a.summary.asset_id == b.summary.asset_id
            && a.summary.descriptor_hash == b.summary.descriptor_hash
    });
    collapse_stale_descriptor_versions(&mut matches);

    match matches.len() {
        0 => match stale_version {
            Some((path, reason)) => ComponentDescriptorFullLookup::Invalid { path, reason },
            None => ComponentDescriptorFullLookup::Missing,
        },
        1 => ComponentDescriptorFullLookup::Found(Box::new(matches.remove(0))),
        _ => ComponentDescriptorFullLookup::Ambiguous(
            matches
                .into_iter()
                .map(|descriptor| descriptor.summary)
                .collect(),
        ),
    }
}

/// Classify `text` against the descriptor schema version.
///
/// - `None`: not a stale descriptor (parse it normally).
/// - `Some(None)`: a descriptor with an unsupported schema version for some
///   *other* component; skip it so it cannot poison unrelated lookups.
/// - `Some(Some(reason))`: an unsupported-version descriptor for the
///   requested component; if the lookup resolves to nothing newer, `reason`
///   is the user-facing rejection.
fn stale_descriptor_schema_version(
    text: &str,
    last_segment: &str,
    canonical_path: &str,
) -> Option<Option<String>> {
    let value = serde_json::from_str::<serde_json::Value>(text).ok()?;
    if value.get("format").and_then(|v| v.as_str()) != Some("rsjs.component_descriptor") {
        return None;
    }
    let schema_version = value.get("schema_version").and_then(|v| v.as_u64());
    let stale = match schema_version {
        Some(version) => version != DESCRIPTOR_SCHEMA_VERSION,
        // Pre-versioning descriptors are stale by definition.
        None => true,
    };
    if !stale {
        return None;
    }
    // Lenient match against the requested component path: every descriptor
    // version so far carries `struct_name` and `self_ty` at the top level.
    let matches_request = match (
        value.get("struct_name").and_then(|v| v.as_str()),
        value.get("self_ty").and_then(|v| v.as_str()),
    ) {
        (Some(struct_name), Some(self_ty)) => {
            struct_name == last_segment
                || last_type_segment(self_ty) == last_segment
                || strip_ws(self_ty) == canonical_path
        }
        _ => false,
    };
    if !matches_request {
        return Some(None);
    }
    Some(Some(schema_version_rejection(schema_version)))
}

/// Build the user-facing error for a descriptor written with an unsupported
/// schema version. The message must say what is wrong, why the descriptor
/// cannot be read, and how to get a fresh one.
fn schema_version_rejection(schema_version: Option<u64>) -> String {
    let found = match schema_version {
        Some(version) => format!("schema_version {version}"),
        None => "no schema_version field".to_string(),
    };
    let age_hint = match schema_version {
        None => "it was written by an older rsjs compiler before descriptor schema versioning",
        Some(1 | 2) => {
            "it was written by an older rsjs compiler whose expression sidecar used the removed \
             legacy v1 expression IR"
        }
        Some(version) if version < DESCRIPTOR_SCHEMA_VERSION => {
            "it was written by an older rsjs compiler with incompatible component field, \
             temporal-input, or linker metadata"
        }
        Some(_) => "it was written by a newer rsjs compiler than the one performing this build",
    };
    format!(
        "stale component descriptor: {found} is unsupported (this compiler reads schema_version \
         {DESCRIPTOR_SCHEMA_VERSION}); {age_hint}. Rebuild the crate that defines this component \
         with the current toolchain so the descriptor is regenerated, or delete the stale \
         `target/rsjs/components` sidecar files (for example via `cargo clean`) and build again"
    )
}

fn collapse_stale_descriptor_versions(matches: &mut Vec<ComponentDescriptor>) {
    let mut collapsed = Vec::new();
    for descriptor in matches.drain(..) {
        let same_type = |existing: &ComponentDescriptor| {
            existing.summary.struct_name == descriptor.summary.struct_name
                && existing.summary.self_ty == descriptor.summary.self_ty
        };
        if let Some(existing) = collapsed.iter_mut().find(|existing| same_type(existing)) {
            if descriptor_is_newer(&descriptor, existing) {
                *existing = descriptor;
            }
        } else {
            collapsed.push(descriptor);
        }
    }
    collapsed.sort_by(|a, b| {
        (
            a.summary.struct_name.as_str(),
            a.summary.self_ty.as_str(),
            a.summary.asset_id.as_str(),
            a.summary.path.as_os_str(),
        )
            .cmp(&(
                b.summary.struct_name.as_str(),
                b.summary.self_ty.as_str(),
                b.summary.asset_id.as_str(),
                b.summary.path.as_os_str(),
            ))
    });
    *matches = collapsed;
}

fn descriptor_is_newer(candidate: &ComponentDescriptor, existing: &ComponentDescriptor) -> bool {
    let candidate_modified = descriptor_modified(&candidate.summary.path);
    let existing_modified = descriptor_modified(&existing.summary.path);
    match (candidate_modified, existing_modified) {
        (Some(candidate), Some(existing)) => candidate > existing,
        (Some(_), None) => true,
        (None, Some(_)) => false,
        (None, None) => candidate.summary.asset_id > existing.summary.asset_id,
    }
}

fn descriptor_modified(path: &std::path::Path) -> Option<std::time::SystemTime> {
    std::fs::metadata(path)
        .ok()
        .and_then(|metadata| metadata.modified().ok())
}

pub(crate) fn parse_descriptor_json(
    path: std::path::PathBuf,
    text: &str,
) -> Result<Option<ComponentDescriptor>, String> {
    let value: serde_json::Value = serde_json::from_str(text).map_err(|err| err.to_string())?;
    let Some(summary) = parse_summary_value(path, &value)? else {
        return Ok(None);
    };
    let inbound_inputs = parse_inbound_inputs(&value)?;
    let prop_fields = parse_prop_fields(&inbound_inputs)?;
    let query_initializers =
        parse_query_initializers(optional_array_field(&value, "query_initializers")?)?;
    let mutation_initializers =
        parse_mutation_initializers(optional_array_field(&value, "mutation_initializers")?)?;
    let island = parse_island(&value)?;
    Ok(Some(ComponentDescriptor {
        summary,
        island,
        prop_fields,
        inbound_inputs,
        query_initializers,
        mutation_initializers,
    }))
}

#[cfg(test)]
pub(crate) fn to_json(
    asset_id: &str,
    struct_name: &str,
    self_ty: &str,
    island: &Island,
    js_source: &str,
) -> String {
    let inbound_inputs = test_inbound_inputs(island);
    to_json_with_initializers(
        asset_id,
        struct_name,
        self_ty,
        island,
        js_source,
        true,
        &[],
        &inbound_inputs,
        &[],
        &[],
    )
}

#[cfg(test)]
fn test_inbound_inputs(island: &Island) -> crate::analyze::InboundInputMetadata {
    let source_anchor = rsjs_ir::SourceAnchor {
        start_line: 1,
        start_column: 1,
    };
    let mut inputs = crate::analyze::InboundInputMetadata::default();
    for slot in &island.callback_slots {
        let id = inputs.fields.len() as u32;
        let value = inputs.values.len() as u32;
        let crossing = inputs.crossings.len() as u32;
        inputs.crossings.push(crate::analyze::InboundCrossing {
            id: crossing,
            field: id,
            kind: crate::analyze::InboundCrossingKind::CallbackResolveCurrent,
            source_anchor,
        });
        inputs.fields.push(crate::analyze::InboundField {
            id,
            name: slot.name.clone(),
            role: crate::analyze::InboundFieldRole::Callback,
            value: Some(value),
            callback_shape: Some(crate::analyze::InboundCallbackShape::Required),
            crossing,
            source_anchor,
        });
        inputs.values.push(crate::analyze::InboundValue {
            id: value,
            field: id,
            source_anchor,
        });
    }
    inputs
}

#[allow(clippy::too_many_arguments)]
#[cfg(test)]
pub(crate) fn to_json_with_initializers(
    asset_id: &str,
    struct_name: &str,
    self_ty: &str,
    island: &Island,
    js_source: &str,
    client_origin_allowed: bool,
    signal_initializers: &[crate::emit_rust::SignalInitializer],
    inbound_inputs: &crate::analyze::InboundInputMetadata,
    query_initializers: &[crate::emit_rust::QueryInitializer],
    mutation_initializers: &[crate::emit_rust::MutationInitializer],
) -> String {
    to_json_with_initializers_and_provider_sites(
        asset_id,
        struct_name,
        self_ty,
        island,
        js_source,
        client_origin_allowed,
        signal_initializers,
        inbound_inputs,
        &[],
        query_initializers,
        mutation_initializers,
    )
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn to_json_with_initializers_and_provider_sites(
    asset_id: &str,
    struct_name: &str,
    self_ty: &str,
    island: &Island,
    js_source: &str,
    client_origin_allowed: bool,
    signal_initializers: &[crate::emit_rust::SignalInitializer],
    inbound_inputs: &crate::analyze::InboundInputMetadata,
    shared_state_provider_sites: &[crate::analyze::DirectSharedStateProviderSite],
    query_initializers: &[crate::emit_rust::QueryInitializer],
    mutation_initializers: &[crate::emit_rust::MutationInitializer],
) -> String {
    let mut out = String::new();
    out.push('{');
    let _ = write!(
        out,
        "\"schema_version\":{DESCRIPTOR_SCHEMA_VERSION},\"format\":\"rsjs.component_descriptor\",\
         \"client_ir_version\":{CLIENT_IR_SCHEMA_VERSION},\"asset_id\":"
    );
    push_json_string(&mut out, asset_id);
    out.push_str(",\"js_hash\":");
    push_json_string(&mut out, &prefixed_sha256_hex(js_source.as_bytes()));
    out.push_str(",\"struct_name\":");
    push_json_string(&mut out, struct_name);
    out.push_str(",\"self_ty\":");
    push_json_string(&mut out, self_ty);
    let _ = write!(
        out,
        ",\"is_async\":{is_async},\"client_origin_permission\":",
        is_async = if island.is_async { "true" } else { "false" }
    );
    push_json_string(
        &mut out,
        if client_origin_allowed {
            ComponentOriginPermission::OriginCapable.as_str()
        } else {
            ComponentOriginPermission::ParticipantOnly.as_str()
        },
    );

    push_callback_slots(&mut out, island);
    push_inferred_fields(&mut out, island, signal_initializers, inbound_inputs);
    push_inbound_inputs(&mut out, inbound_inputs);
    push_shared_state_provider_sites(&mut out, shared_state_provider_sites);
    push_shared_stores(&mut out, island);
    push_signals(&mut out, island);
    push_node_refs(&mut out, island);
    push_expr_arena(&mut out, island);
    push_handlers(&mut out, island);
    push_intervals(&mut out, island);
    push_view_tree(&mut out, island);
    push_keyed_collections(&mut out, island);
    push_bindings(&mut out, island);
    push_queries(&mut out, island);
    push_mutations(&mut out, island);
    push_query_initializers(&mut out, query_initializers);
    push_mutation_initializers(&mut out, mutation_initializers);

    let _ = write!(
        out,
        ",\"counts\":{{\"fields\":{fields},\"values\":{values},\"prop_sites\":{prop_sites},\
         \"crossings\":{crossings},\"node_ref_fields\":{node_ref_fields},\
         \"projection_fields\":{projection_fields},\
         \"shared_state_provider_sites\":{shared_state_provider_sites},\"signals\":{signals},\
         \"handlers\":{handlers},\"callback_slots\":{callback_slots},\"node_refs\":{node_refs},\
         \"exprs\":{exprs},\"nodes\":{nodes}",
        fields = inbound_inputs.fields.len()
            + inferred_non_input_field_count(island, signal_initializers, inbound_inputs),
        values = inbound_inputs.values.len(),
        prop_sites = inbound_inputs.prop_sites.len(),
        crossings = inbound_inputs.crossings.len(),
        node_ref_fields = inbound_inputs.node_ref_fields.len(),
        projection_fields = inbound_inputs.projection_fields.len(),
        shared_state_provider_sites = shared_state_provider_sites.len(),
        signals = island.signals.len(),
        handlers = island.handlers.len(),
        callback_slots = island.callback_slots.len(),
        node_refs = island.node_refs.len(),
        exprs = island.exprs.len(),
        nodes = island.view_tree.len()
    );
    if !island.lists.is_empty() {
        let _ = write!(
            out,
            ",\"keyed_collections\":{count}",
            count = island.lists.len()
        );
    }
    out.push('}');

    let descriptor_hash = prefixed_sha256_hex(out.as_bytes());
    out.push_str(",\"descriptor_hash\":");
    push_json_string(&mut out, &descriptor_hash);
    out.push('}');
    out
}

fn push_shared_state_provider_sites(
    out: &mut String,
    sites: &[crate::analyze::DirectSharedStateProviderSite],
) {
    out.push_str(",\"shared_state_provider_sites\":[");
    for (index, site) in sites.iter().enumerate() {
        debug_assert_eq!(site.id.index() as usize, index);
        comma(out, index);
        let _ = write!(
            out,
            "{{\"id\":{id},\"child_edge\":{child_edge},\"site_node\":{site_node},\
             \"state_rust_type\":",
            id = site.id.index(),
            child_edge = site.child_edge,
            site_node = site.site_node.0
        );
        push_json_string(out, &site.state_ty.to_token_stream().to_string());
        out.push_str(",\"topology\":\"static_single_wrapperless\",\"source_anchor\":");
        push_source_anchor(out, site.source_anchor);
        out.push('}');
    }
    out.push(']');
}

fn parse_summary_value(
    path: std::path::PathBuf,
    value: &serde_json::Value,
) -> Result<Option<ComponentDescriptorSummary>, String> {
    if value.get("format").and_then(|v| v.as_str()) != Some("rsjs.component_descriptor") {
        return Ok(None);
    }
    let schema_version = value
        .get("schema_version")
        .and_then(|v| v.as_u64())
        .ok_or_else(|| "missing numeric schema_version".to_string())?;
    if schema_version != DESCRIPTOR_SCHEMA_VERSION {
        return Err(schema_version_rejection(Some(schema_version)));
    }
    let client_ir_version = value
        .get("client_ir_version")
        .and_then(|v| v.as_u64())
        .ok_or_else(|| "missing numeric client_ir_version".to_string())?;
    if client_ir_version != u64::from(CLIENT_IR_SCHEMA_VERSION) {
        return Err(format!(
            "unsupported rsjs client IR version {client_ir_version}; this compiler supports \
             {CLIENT_IR_SCHEMA_VERSION}"
        ));
    }

    let asset_id = required_str(value, "asset_id")?.to_string();
    let struct_name = required_str(value, "struct_name")?.to_string();
    let self_ty = required_str(value, "self_ty")?.to_string();
    let js_hash = required_hash(value, "js_hash")?.to_string();
    let descriptor_hash = required_hash(value, "descriptor_hash")?.to_string();
    let is_async = value
        .get("is_async")
        .and_then(|v| v.as_bool())
        .ok_or_else(|| "missing boolean is_async".to_string())?;
    let client_origin_permission = ComponentOriginPermission::parse(value)?;

    let mut handler_fields = Vec::new();
    let mut prop_fields = Vec::new();
    let mut server_prop_fields = Vec::new();
    let mut static_slot_fields = Vec::new();
    let fields = value
        .get("fields")
        .and_then(|v| v.as_array())
        .ok_or_else(|| "missing fields array".to_string())?;
    for (index, field) in fields.iter().enumerate() {
        expect_dense(index, id_field(field, "id")?, "component fields")?;
        match field.get("kind").and_then(|v| v.as_str()) {
            Some("handler") => {
                let name = field
                    .get("name")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| "handler field missing name".to_string())?;
                handler_fields.push(name.to_string());
            }
            Some("prop") => {
                let name = field
                    .get("name")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| "prop field missing name".to_string())?;
                prop_fields.push(name.to_string());
            }
            Some("server_prop") => {
                let name = field
                    .get("name")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| "server_prop field missing name".to_string())?;
                server_prop_fields.push(name.to_string());
            }
            Some("static_slot") => {
                let name = field
                    .get("name")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| "static_slot field missing name".to_string())?;
                static_slot_fields.push(name.to_string());
            }
            _ => {}
        }
    }
    handler_fields.sort();
    handler_fields.dedup();
    prop_fields.sort();
    prop_fields.dedup();
    server_prop_fields.sort();
    server_prop_fields.dedup();
    static_slot_fields.sort();
    static_slot_fields.dedup();

    Ok(Some(ComponentDescriptorSummary {
        path,
        asset_id,
        struct_name,
        self_ty,
        is_async,
        client_origin_permission,
        handler_fields,
        prop_fields,
        server_prop_fields,
        static_slot_fields,
        js_hash,
        descriptor_hash,
    }))
}

fn parse_prop_fields(inputs: &ParsedInboundInputs) -> Result<Vec<ComponentPropField>, String> {
    let mut out = Vec::new();
    for site in &inputs.prop_sites {
        let signal = match site.target {
            crate::analyze::InboundPropTarget::LiveSignal(signal)
            | crate::analyze::InboundPropTarget::SignalSeed(signal) => signal,
            crate::analyze::InboundPropTarget::SharedStateProviderSeed
            | crate::analyze::InboundPropTarget::ExplicitIslandBoundary => continue,
        };
        let field = inputs
            .fields
            .iter()
            .find(|field| field.id == site.field)
            .ok_or_else(|| {
                format!(
                    "prop site {} refers to unknown field {}",
                    site.id, site.field
                )
            })?;
        out.push(ComponentPropField {
            name: field.name.clone(),
            signal,
        });
    }
    out.sort_by(|a, b| a.name.cmp(&b.name).then(a.signal.0.cmp(&b.signal.0)));
    out.dedup_by(|a, b| a.name == b.name && a.signal == b.signal);
    Ok(out)
}

fn parse_inbound_inputs(value: &serde_json::Value) -> Result<ParsedInboundInputs, String> {
    let mut fields = Vec::new();
    let mut field_names = std::collections::BTreeSet::new();
    for (index, field) in array_field(value, "fields")?.iter().enumerate() {
        let id = id_field(field, "id")?;
        expect_dense(index, id, "component fields")?;
        let name = string_field(field, "name")?.to_string();
        if !field_names.insert(name.clone()) {
            return Err(format!(
                "component field name `{name}` is declared more than once"
            ));
        }
        let role = match string_field(field, "kind")? {
            "prop" => crate::analyze::InboundFieldRole::Value,
            "handler" => crate::analyze::InboundFieldRole::Callback,
            "node_ref" => crate::analyze::InboundFieldRole::NodeRef,
            "deferred_projection" => crate::analyze::InboundFieldRole::DeferredProjection,
            "server_prop" | "static_slot" => continue,
            other => {
                return Err(format!(
                    "unsupported schema-v9 component field kind `{other}`"
                ))
            }
        };
        let (value, callback_shape) = match role {
            crate::analyze::InboundFieldRole::Value => (Some(id_field(field, "value_id")?), None),
            crate::analyze::InboundFieldRole::Callback => {
                if string_field(field, "dispatch")? != "resolve_current" {
                    return Err(format!(
                        "callback field `{name}` must use dispatch `resolve_current`"
                    ));
                }
                let slot = id_field(field, "callback_slot")?;
                if !array_field(value, "callback_slots")?
                    .iter()
                    .any(|candidate| {
                        id_field(candidate, "id").ok() == Some(slot)
                            && string_field(candidate, "name").ok() == Some(name.as_str())
                    })
                {
                    return Err(format!(
                        "callback field `{name}` refers to unknown or differently named callback slot {slot}"
                    ));
                }
                let callback_shape = match string_field(field, "callback_shape")? {
                    "required" => crate::analyze::InboundCallbackShape::Required,
                    "optional" => crate::analyze::InboundCallbackShape::Optional,
                    other => {
                        return Err(format!(
                            "callback field `{name}` has unsupported callback_shape `{other}`"
                        ))
                    }
                };
                (Some(id_field(field, "value_id")?), Some(callback_shape))
            }
            crate::analyze::InboundFieldRole::NodeRef => (Some(id_field(field, "value_id")?), None),
            crate::analyze::InboundFieldRole::DeferredProjection => {
                (Some(id_field(field, "value_id")?), None)
            }
        };
        fields.push(ParsedInboundField {
            id,
            name,
            role,
            value,
            callback_shape,
            crossing: id_field(field, "crossing_id")?,
            source_anchor: required_source_anchor(field)?,
        });
    }
    for (index, field) in fields.iter().enumerate() {
        expect_dense(index, field.id, "inbound component fields")?;
    }

    let inputs = object_field(value, "inbound_inputs")?;
    let mut values = Vec::new();
    let values_json = array_field(inputs, "values")?;
    if values_json.len() != fields.len() {
        return Err(format!(
            "schema-v9 inbound values must be one-to-one with inbound fields; found {} values for {} fields",
            values_json.len(),
            fields.len()
        ));
    }
    for (index, value) in values_json.iter().enumerate() {
        let id = id_field(value, "id")?;
        expect_dense(index, id, "inbound component values")?;
        let field = id_field(value, "field_id")?;
        let field_schema = fields
            .iter()
            .find(|candidate| candidate.id == field)
            .ok_or_else(|| format!("inbound value {id} refers to unknown field {field}"))?;
        if field_schema.value != Some(id) {
            return Err(format!(
                "inbound value {id} does not match field `{}` value_id {:?}",
                field_schema.name, field_schema.value
            ));
        }
        values.push(ParsedInboundValue {
            id,
            field,
            source_anchor: required_source_anchor(value)?,
        });
    }

    let mut crossings = Vec::new();
    for (index, crossing) in array_field(inputs, "crossings")?.iter().enumerate() {
        let id = id_field(crossing, "id")?;
        expect_dense(index, id, "inbound crossing hooks")?;
        let field = id_field(crossing, "field_id")?;
        let field_schema = fields
            .iter()
            .find(|candidate| candidate.id == field)
            .ok_or_else(|| format!("inbound crossing {id} refers to unknown field {field}"))?;
        if field_schema.crossing != id {
            return Err(format!(
                "inbound crossing {id} does not match field `{}` crossing_id {}",
                field_schema.name, field_schema.crossing
            ));
        }
        let kind = match string_field(crossing, "kind")? {
            "value" => crate::analyze::InboundCrossingKind::Value,
            "callback_resolve_current" => {
                crate::analyze::InboundCrossingKind::CallbackResolveCurrent
            }
            "node_ref_alias" => crate::analyze::InboundCrossingKind::NodeRefAlias,
            "deferred_projection" => crate::analyze::InboundCrossingKind::DeferredProjection,
            other => return Err(format!("unsupported inbound crossing kind `{other}`")),
        };
        let expected = match field_schema.role {
            crate::analyze::InboundFieldRole::Value => crate::analyze::InboundCrossingKind::Value,
            crate::analyze::InboundFieldRole::Callback => {
                crate::analyze::InboundCrossingKind::CallbackResolveCurrent
            }
            crate::analyze::InboundFieldRole::NodeRef => {
                crate::analyze::InboundCrossingKind::NodeRefAlias
            }
            crate::analyze::InboundFieldRole::DeferredProjection => {
                crate::analyze::InboundCrossingKind::DeferredProjection
            }
        };
        if kind != expected {
            return Err(format!(
                "inbound crossing {id} kind `{}` conflicts with field `{}` role `{}`",
                kind.as_str(),
                field_schema.name,
                field_schema.role.as_str(),
            ));
        }
        crossings.push(ParsedInboundCrossing {
            id,
            field,
            kind,
            source_anchor: required_source_anchor(crossing)?,
        });
    }
    if crossings.len() != fields.len() {
        return Err(format!(
            "inbound crossing hooks must be one-to-one with inbound fields; found {} crossings for {} fields",
            crossings.len(),
            fields.len()
        ));
    }

    let declared_node_refs = array_field(value, "node_refs")?
        .iter()
        .map(|entry| id_field(entry, "id"))
        .collect::<Result<std::collections::BTreeSet<_>, _>>()?;
    let mut node_ref_fields = Vec::new();
    let mut node_ref_field_ids = std::collections::BTreeSet::new();
    let mut inbound_node_ref_ids = std::collections::BTreeSet::new();
    for (index, handle) in array_field(inputs, "node_ref_fields")?.iter().enumerate() {
        let id = id_field(handle, "id")?;
        expect_dense(index, id, "inbound node-ref fields")?;
        let field = id_field(handle, "field_id")?;
        let value_id = id_field(handle, "value_id")?;
        let crossing = id_field(handle, "crossing_id")?;
        let node_ref = id_field(handle, "node_ref_id")?;
        let field_schema = fields
            .iter()
            .find(|candidate| candidate.id == field)
            .ok_or_else(|| {
                format!("inbound node-ref field {id} refers to unknown field {field}")
            })?;
        if field_schema.role != crate::analyze::InboundFieldRole::NodeRef {
            return Err(format!(
                "inbound node-ref field {id} refers to non-node-ref field `{}`",
                field_schema.name
            ));
        }
        if field_schema.value != Some(value_id) || field_schema.crossing != crossing {
            return Err(format!(
                "inbound node-ref field {id} disagrees with field `{}` value/crossing identity",
                field_schema.name
            ));
        }
        if !declared_node_refs.contains(&node_ref) {
            return Err(format!(
                "inbound node-ref field {id} refers to unknown node ref {node_ref}"
            ));
        }
        if !node_ref_field_ids.insert(field) {
            return Err(format!(
                "component field `{}` has more than one inbound node-ref identity",
                field_schema.name
            ));
        }
        if !inbound_node_ref_ids.insert(node_ref) {
            return Err(format!(
                "inbound node ref {node_ref} is aliased by more than one component field"
            ));
        }
        let bind_target = handle
            .get("bind_target")
            .map(|value| {
                let value = value.as_str().ok_or_else(|| {
                    format!("inbound node-ref field {id} bind_target must be a string")
                })?;
                match value {
                    "element" => Ok(crate::analyze::NodeRefBindTarget::Element),
                    "html_element" => Ok(crate::analyze::NodeRefBindTarget::HtmlElement),
                    "input" => Ok(crate::analyze::NodeRefBindTarget::Input),
                    "textarea" => Ok(crate::analyze::NodeRefBindTarget::TextArea),
                    "select" => Ok(crate::analyze::NodeRefBindTarget::Select),
                    "form" => Ok(crate::analyze::NodeRefBindTarget::Form),
                    "dialog" => Ok(crate::analyze::NodeRefBindTarget::Dialog),
                    other => Err(format!(
                        "inbound node-ref field {id} has unsupported bind_target `{other}`"
                    )),
                }
            })
            .transpose()?;
        node_ref_fields.push(ParsedInboundNodeRefField {
            field,
            value: value_id,
            crossing,
            node_ref: NodeRefId(node_ref),
            bind_target,
            source_anchor: required_source_anchor(handle)?,
        });
    }

    let declared_signals = array_field(value, "signals")?
        .iter()
        .map(|entry| id_field(entry, "id"))
        .collect::<Result<std::collections::BTreeSet<_>, _>>()?;
    let mut projection_fields = Vec::new();
    let mut projection_field_ids = std::collections::BTreeSet::new();
    for (index, projection) in array_field(inputs, "projection_fields")?.iter().enumerate() {
        let id = id_field(projection, "id")?;
        expect_dense(index, id, "inbound deferred projections")?;
        let field = id_field(projection, "field_id")?;
        let value_id = id_field(projection, "value_id")?;
        let crossing = id_field(projection, "crossing_id")?;
        let signal = id_field(projection, "signal_id")?;
        let node_ref = id_field(projection, "node_ref_id")?;
        let field_schema = fields
            .iter()
            .find(|candidate| candidate.id == field)
            .ok_or_else(|| {
                format!("inbound deferred projection {id} refers to unknown field {field}")
            })?;
        if field_schema.value != Some(value_id) || field_schema.crossing != crossing {
            return Err(format!(
                "inbound deferred projection {id} disagrees with field `{}` value/crossing identity",
                field_schema.name
            ));
        }
        if !projection_field_ids.insert(field) {
            return Err(format!(
                "component field `{}` has more than one deferred projection identity",
                field_schema.name
            ));
        }
        if !declared_signals.contains(&signal) {
            return Err(format!(
                "inbound deferred projection {id} refers to unknown signal {signal}"
            ));
        }
        if !declared_node_refs.contains(&node_ref) {
            return Err(format!(
                "inbound deferred projection {id} refers to unknown node ref {node_ref}"
            ));
        }
        projection_fields.push(ParsedInboundProjectionField {
            field,
            value: value_id,
            crossing,
            signal: SignalId(signal),
            node_ref: NodeRefId(node_ref),
            source_anchor: required_source_anchor(projection)?,
        });
    }

    let mut prop_sites = Vec::new();
    for (index, site) in array_field(inputs, "prop_sites")?.iter().enumerate() {
        let id = id_field(site, "id")?;
        expect_dense(index, id, "inbound prop-use sites")?;
        let field = id_field(site, "field_id")?;
        let value_id = id_field(site, "value_id")?;
        let crossing = id_field(site, "crossing_id")?;
        let field_schema = fields
            .iter()
            .find(|candidate| candidate.id == field)
            .ok_or_else(|| format!("inbound prop site {id} refers to unknown field {field}"))?;
        if field_schema.role != crate::analyze::InboundFieldRole::Value {
            return Err(format!(
                "inbound prop site {id} refers to callback field `{}`",
                field_schema.name
            ));
        }
        if field_schema.value != Some(value_id) {
            return Err(format!(
                "inbound prop site {id} value_id {value_id} does not match field `{}`",
                field_schema.name
            ));
        }
        if field_schema.crossing != crossing {
            return Err(format!(
                "inbound prop site {id} crossing_id {crossing} does not match field `{}`",
                field_schema.name
            ));
        }
        if !values.iter().any(|value| value.id == value_id) {
            return Err(format!(
                "inbound prop site {id} refers to unknown value {value_id}"
            ));
        }
        if !crossings.iter().any(|hook| hook.id == crossing) {
            return Err(format!(
                "inbound prop site {id} refers to unknown crossing {crossing}"
            ));
        }

        let temporal_value = object_field(site, "temporal")?;
        let temporal = match string_field(temporal_value, "kind")? {
            "live_read" => {
                if temporal_value.get("site_id").is_some() {
                    return Err(format!(
                        "inbound prop site {id} LiveRead mode must not carry site_id"
                    ));
                }
                crate::analyze::InboundTemporalMode::LiveRead
            }
            "seed_once" => {
                let seed_site = id_field(temporal_value, "site_id")?;
                if seed_site != id {
                    return Err(format!(
                        "inbound prop site {id} SeedOnce references different site {seed_site}"
                    ));
                }
                crate::analyze::InboundTemporalMode::SeedOnce
            }
            "query_key_dependency" => {
                if temporal_value.get("site_id").is_some() {
                    return Err(format!(
                        "inbound prop site {id} QueryKeyDependency mode must not carry site_id"
                    ));
                }
                crate::analyze::InboundTemporalMode::QueryKeyDependency
            }
            "forbidden_provider_seed" => {
                if temporal_value.get("site_id").is_some() {
                    return Err(format!(
                        "inbound prop site {id} forbidden provider-seed mode must not carry site_id"
                    ));
                }
                crate::analyze::InboundTemporalMode::ForbiddenProviderSeed
            }
            "forbidden_boundary_crossing" => {
                if temporal_value.get("site_id").is_some() {
                    return Err(format!(
                        "inbound prop site {id} forbidden boundary mode must not carry site_id"
                    ));
                }
                crate::analyze::InboundTemporalMode::ForbiddenBoundaryCrossing
            }
            other => return Err(format!("unsupported inbound temporal mode `{other}`")),
        };
        let target_value = object_field(site, "target")?;
        let target = match string_field(target_value, "kind")? {
            "live_signal" => {
                if !matches!(
                    temporal,
                    crate::analyze::InboundTemporalMode::LiveRead
                        | crate::analyze::InboundTemporalMode::QueryKeyDependency
                ) {
                    return Err(format!(
                        "inbound prop site {id} targets a live signal without current-value semantics"
                    ));
                }
                let signal = id_field(target_value, "signal_id")?;
                if !array_field(value, "signals")?
                    .iter()
                    .any(|decl| id_field(decl, "id").ok() == Some(signal))
                {
                    return Err(format!(
                        "inbound prop site {id} targets unknown signal {signal}"
                    ));
                }
                crate::analyze::InboundPropTarget::LiveSignal(SignalId(signal))
            }
            "signal_seed" => {
                if temporal != crate::analyze::InboundTemporalMode::SeedOnce {
                    return Err(format!(
                        "inbound prop site {id} targets a signal seed without SeedOnce semantics"
                    ));
                }
                let signal = id_field(target_value, "signal_id")?;
                if !array_field(value, "signals")?
                    .iter()
                    .any(|decl| id_field(decl, "id").ok() == Some(signal))
                {
                    return Err(format!(
                        "inbound prop site {id} targets unknown signal {signal}"
                    ));
                }
                crate::analyze::InboundPropTarget::SignalSeed(SignalId(signal))
            }
            "explicit_island_boundary" => {
                if temporal != crate::analyze::InboundTemporalMode::ForbiddenBoundaryCrossing {
                    return Err(format!(
                        "inbound prop site {id} targets an explicit island without forbidden-boundary semantics"
                    ));
                }
                crate::analyze::InboundPropTarget::ExplicitIslandBoundary
            }
            "shared_state_provider_seed" => {
                if temporal != crate::analyze::InboundTemporalMode::ForbiddenProviderSeed {
                    return Err(format!(
                        "inbound prop site {id} targets SharedStateProvider initialization without forbidden-provider-seed semantics"
                    ));
                }
                crate::analyze::InboundPropTarget::SharedStateProviderSeed
            }
            other => return Err(format!("unsupported inbound prop target `{other}`")),
        };
        prop_sites.push(ParsedInboundPropSite {
            id,
            field,
            value: value_id,
            crossing,
            temporal,
            target,
            source_anchor: required_source_anchor(site)?,
        });
    }

    for field in &fields {
        match field.role {
            crate::analyze::InboundFieldRole::Value => {
                if !prop_sites.iter().any(|site| site.field == field.id) {
                    return Err(format!(
                        "inbound value field `{}` has no prop-use site",
                        field.name
                    ));
                }
            }
            crate::analyze::InboundFieldRole::Callback => {
                if field.value.is_none() || field.callback_shape.is_none() {
                    return Err(format!(
                        "callback field `{}` is missing its symbolic value or callback shape",
                        field.name
                    ));
                }
                if prop_sites.iter().any(|site| site.field == field.id) {
                    return Err(format!(
                        "callback field `{}` unexpectedly has a value prop-use site",
                        field.name
                    ));
                }
            }
            crate::analyze::InboundFieldRole::NodeRef => {
                if field.value.is_none() || field.callback_shape.is_some() {
                    return Err(format!(
                        "node-ref field `{}` must carry one symbolic value and no callback shape",
                        field.name
                    ));
                }
                if prop_sites.iter().any(|site| site.field == field.id) {
                    return Err(format!(
                        "node-ref field `{}` unexpectedly has a temporal value prop-use site",
                        field.name
                    ));
                }
                if !node_ref_fields
                    .iter()
                    .any(|node_ref| node_ref.field == field.id)
                {
                    return Err(format!(
                        "node-ref field `{}` has no child-local node-ref alias identity",
                        field.name
                    ));
                }
            }
            crate::analyze::InboundFieldRole::DeferredProjection => {
                if field.value.is_none() || field.callback_shape.is_some() {
                    return Err(format!(
                        "deferred projection field `{}` must carry one symbolic value and no callback shape",
                        field.name
                    ));
                }
                if prop_sites.iter().any(|site| site.field == field.id) {
                    return Err(format!(
                        "deferred projection field `{}` unexpectedly has a concrete temporal prop-use site",
                        field.name
                    ));
                }
                if !projection_fields
                    .iter()
                    .any(|projection| projection.field == field.id)
                {
                    return Err(format!(
                        "deferred projection field `{}` has no latent role identities",
                        field.name
                    ));
                }
            }
        }
    }

    Ok(ParsedInboundInputs {
        fields,
        values,
        prop_sites,
        crossings,
        node_ref_fields,
        projection_fields,
    })
}

fn required_source_anchor(value: &serde_json::Value) -> Result<rsjs_ir::SourceAnchor, String> {
    let anchor = object_field(value, "source_anchor")?;
    let start_line = anchor
        .get("start_line")
        .and_then(serde_json::Value::as_u64)
        .ok_or_else(|| "source_anchor.start_line must be an unsigned integer".to_string())?;
    let start_column = anchor
        .get("start_column")
        .and_then(serde_json::Value::as_u64)
        .ok_or_else(|| "source_anchor.start_column must be an unsigned integer".to_string())?;
    Ok(rsjs_ir::SourceAnchor {
        start_line: usize::try_from(start_line)
            .map_err(|_| "source_anchor.start_line exceeds usize::MAX".to_string())?,
        start_column: usize::try_from(start_column)
            .map_err(|_| "source_anchor.start_column exceeds usize::MAX".to_string())?,
    })
}

fn parse_island(value: &serde_json::Value) -> Result<Island, String> {
    let mut island = Island::new();
    island.shared_stores = parse_shared_stores(optional_array_field(value, "shared_stores")?)?;
    island.signals = parse_signals(array_field(value, "signals")?)?;
    island.handlers = parse_handlers(array_field(value, "handlers")?)?;
    island.callback_slots = parse_callback_slots(array_field(value, "callback_slots")?)?;
    island.node_refs = parse_node_refs(optional_array_field(value, "node_refs")?)?;
    island.bindings = parse_bindings(object_field(value, "bindings")?)?;
    island.exprs = parse_expr_arena(array_field(value, "exprs")?)?;
    island.view_tree = parse_view_tree(object_field(value, "view_tree")?)?;
    island.lists = parse_keyed_collections(optional_array_field(value, "keyed_collections")?)?;
    island.is_async = bool_field(value, "is_async")?;
    island.client_module_imports = Vec::new();
    island.component_fragment_imports = Vec::new();
    island.component_fragment_mounts = Vec::new();
    island.queries = parse_queries(array_field(value, "queries")?)?;
    island.mutations = parse_mutations(array_field(value, "mutations")?)?;
    island.intervals = parse_intervals(optional_array_field(value, "intervals")?)?;
    Ok(island)
}

fn parse_shared_stores(values: &[serde_json::Value]) -> Result<Vec<SharedStoreDecl>, String> {
    let mut out = Vec::with_capacity(values.len());
    for (idx, value) in values.iter().enumerate() {
        let id = id_field(value, "id")?;
        expect_dense(idx, id, "shared_stores")?;
        out.push(SharedStoreDecl {
            id: SharedStoreId(id),
            source_name: string_field(value, "source_name")?.to_string(),
            rust_type: string_field(value, "rust_type")?.to_string(),
            source_anchor: source_anchor_field(value),
        });
    }
    Ok(out)
}

fn parse_callback_slots(values: &[serde_json::Value]) -> Result<Vec<CallbackSlot>, String> {
    let mut out = Vec::with_capacity(values.len());
    for (idx, value) in values.iter().enumerate() {
        let id = id_field(value, "id")?;
        expect_dense(idx, id, "callback_slots")?;
        out.push(CallbackSlot {
            id: rsjs_ir::CallbackSlotId(id),
            name: string_field(value, "name")?.to_string(),
        });
    }
    Ok(out)
}

fn parse_signals(values: &[serde_json::Value]) -> Result<Vec<SignalDecl>, String> {
    let mut out = Vec::with_capacity(values.len());
    let mut previous_id = None;
    for value in values {
        let id = id_field(value, "id")?;
        if previous_id.is_some_and(|previous| id <= previous) {
            return Err(format!(
                "signals must be ordered by unique id; previous id {}, got {id}",
                previous_id.unwrap()
            ));
        }
        previous_id = Some(id);
        let init = parse_value(field(value, "init")?)?;
        // Hand-built fixtures may omit "ty"; default to the seed
        // literal's client shape, matching `IslandBuilder::signal`.
        let ty = match value.get("ty") {
            Some(ty) => parse_signal_decl_type(ty)?,
            None => ClientType::of_value(&init),
        };
        let shared = match value.get("shared") {
            Some(shared) => Some(SharedFieldRef {
                store: SharedStoreId(id_field(shared, "store")?),
                field: string_field(shared, "field")?.to_string(),
            }),
            None => None,
        };
        out.push(SignalDecl {
            id: SignalId(id),
            source_name: value
                .get("source_name")
                .and_then(serde_json::Value::as_str)
                .map(str::to_string),
            source_anchor: source_anchor_field(value),
            init,
            ty,
            integer_domain: value
                .get("integer_domain")
                .and_then(serde_json::Value::as_str)
                .and_then(rsjs_ir::IntegerDomain::from_name),
            derived: value
                .get("derived")
                .and_then(|v| v.as_u64())
                .map(|body| rsjs_ir::ExprId(body as u32)),
            shared,
        });
    }
    Ok(out)
}

fn parse_signal_decl_type(value: &serde_json::Value) -> Result<ClientType, String> {
    parse_client_type_json(value).map_err(|err| format!("signal `ty` field: {err}"))
}

fn parse_node_refs(values: &[serde_json::Value]) -> Result<Vec<NodeRefDecl>, String> {
    let mut out = Vec::with_capacity(values.len());
    for (idx, value) in values.iter().enumerate() {
        let id = id_field(value, "id")?;
        expect_dense(idx, id, "node_refs")?;
        out.push(NodeRefDecl {
            id: NodeRefId(id),
            source_name: value
                .get("source_name")
                .and_then(serde_json::Value::as_str)
                .map(str::to_string),
            source_anchor: source_anchor_field(value),
        });
    }
    Ok(out)
}

fn parse_expr_arena(values: &[serde_json::Value]) -> Result<ExprArena, String> {
    let mut arena = ExprArena::new();
    for (idx, value) in values.iter().enumerate() {
        let id = id_field(value, "id")?;
        expect_dense(idx, id, "exprs")?;
        let expr = parse_client_expr_json(field(value, "expr")?).map_err(String::from)?;
        let allocated = arena.alloc(expr);
        if allocated.0 != id {
            return Err(format!("expr arena allocation drifted at id {id}"));
        }
    }
    Ok(arena)
}

fn parse_handlers(values: &[serde_json::Value]) -> Result<Vec<EventHandler>, String> {
    let mut out = Vec::with_capacity(values.len());
    let mut previous_id = None;
    for value in values {
        let id = id_field(value, "id")?;
        if previous_id.is_some_and(|previous| id <= previous) {
            return Err(format!(
                "handlers must be ordered by unique id; previous id {}, got {id}",
                previous_id.unwrap()
            ));
        }
        previous_id = Some(id);
        let body_ir = parse_handler_body_ir(value)?;
        let mut handler = match value.get("debounce_ms").and_then(|v| v.as_u64()) {
            Some(ms) => EventHandler::from_ir_debounced(EventHandlerId(id), ms, body_ir),
            None => EventHandler::from_ir(EventHandlerId(id), body_ir),
        };
        handler.source_name = value
            .get("source_name")
            .and_then(serde_json::Value::as_str)
            .map(str::to_string);
        handler.source_anchor = source_anchor_field(value);
        out.push(handler);
    }
    Ok(out)
}

fn parse_intervals(values: &[serde_json::Value]) -> Result<Vec<rsjs_ir::IntervalDecl>, String> {
    let mut out = Vec::with_capacity(values.len());
    for value in values {
        let ms = value
            .get("ms")
            .and_then(|v| v.as_u64())
            .ok_or_else(|| "interval entry missing integer ms".to_string())?;
        let body_ir = parse_handler_body_ir(value)?;
        out.push(rsjs_ir::IntervalDecl::from_ir(ms, body_ir));
    }
    Ok(out)
}

fn parse_handler_body_ir(value: &serde_json::Value) -> Result<ClientBlock, String> {
    let body_ir_value = field(value, "body_ir")?;
    let version = id_field(value, "body_ir_version")?;
    if version != CLIENT_IR_SCHEMA_VERSION {
        return Err(format!("unsupported handler body_ir_version {version}"));
    }
    parse_client_block_json(body_ir_value).map_err(String::from)
}

fn parse_view_tree(value: &serde_json::Value) -> Result<ViewTree, String> {
    let mut tree = ViewTree::new();
    for (idx, entry) in array_field(value, "nodes")?.iter().enumerate() {
        let id = id_field(entry, "id")?;
        expect_dense(idx, id, "view_tree.nodes")?;
        let allocated = tree.alloc(parse_view_node(field(entry, "node")?)?);
        if allocated.0 != id {
            return Err(format!("view tree allocation drifted at id {id}"));
        }
    }
    match field(value, "root")? {
        serde_json::Value::Null => {}
        other => tree.set_root(NodeId(as_u32(other, "view_tree.root")?)),
    }
    Ok(tree)
}

fn parse_keyed_collections(values: &[serde_json::Value]) -> Result<Vec<ListDecl>, String> {
    let mut lists = Vec::with_capacity(values.len());
    for (idx, value) in values.iter().enumerate() {
        let id = id_field(value, "id")?;
        expect_dense(idx, id, "keyed_collections")?;
        let source = object_field(value, "source")?;
        let source = match string_field(source, "kind")? {
            "signal" => ListSource::Signal(SignalId(id_field(source, "id")?)),
            "expression" => ListSource::Expr(ExprId(id_field(source, "id")?)),
            other => {
                return Err(format!(
                    "keyed_collections[{idx}].source has unsupported kind `{other}`"
                ));
            }
        };
        lists.push(ListDecl {
            container: NodeId(id_field(value, "container")?),
            loop_node: NodeId(id_field(value, "loop_node")?),
            item_root: NodeId(id_field(value, "item_root")?),
            source,
            key: parse_list_item_expr(field(value, "key")?)?,
            key_tokens: string_field(value, "key_tokens")?.to_owned(),
            item: parse_list_template_node(field(value, "item")?)?,
        });
    }
    Ok(lists)
}

fn parse_list_item_expr(value: &serde_json::Value) -> Result<ListItemExpr, String> {
    match string_field(value, "kind")? {
        "item" => Ok(ListItemExpr::Item),
        "field" => {
            let name = string_field(value, "name")?;
            if name.is_empty() {
                return Err("keyed-list item field name cannot be empty".to_owned());
            }
            Ok(ListItemExpr::ItemField(name.to_owned()))
        }
        other => Err(format!(
            "unsupported keyed-list item expression kind `{other}`"
        )),
    }
}

fn parse_list_template_node(value: &serde_json::Value) -> Result<ListTemplateNode, String> {
    match string_field(value, "kind")? {
        "element" => {
            let tag = string_field(value, "tag")?;
            if tag.is_empty() {
                return Err("keyed-list item element tag cannot be empty".to_owned());
            }
            let static_attrs = array_field(value, "static_attrs")?
                .iter()
                .map(|attr| {
                    Ok((
                        string_field(attr, "name")?.to_owned(),
                        string_field(attr, "value")?.to_owned(),
                    ))
                })
                .collect::<Result<Vec<_>, String>>()?;
            let dynamic_attrs = array_field(value, "dynamic_attrs")?
                .iter()
                .map(|attr| {
                    Ok((
                        string_field(attr, "name")?.to_owned(),
                        parse_list_item_expr(field(attr, "value")?)?,
                    ))
                })
                .collect::<Result<Vec<_>, String>>()?;
            let children = array_field(value, "children")?
                .iter()
                .map(parse_list_template_node)
                .collect::<Result<Vec<_>, String>>()?;
            Ok(ListTemplateNode::Element {
                tag: tag.to_owned(),
                static_attrs,
                dynamic_attrs,
                children,
            })
        }
        "text" => Ok(ListTemplateNode::Text(
            string_field(value, "text")?.to_owned(),
        )),
        "slot" => Ok(ListTemplateNode::Slot(parse_list_item_expr(field(
            value, "value",
        )?)?)),
        other => Err(format!(
            "unsupported keyed-list template node kind `{other}`"
        )),
    }
}

fn parse_bindings(value: &serde_json::Value) -> Result<Bindings, String> {
    let mut bindings = Bindings::new();
    for entry in array_field(value, "text")? {
        bindings.text_bindings.insert(
            NodeId(id_field(entry, "node")?),
            ExprId(id_field(entry, "expr")?),
        );
    }
    for entry in array_field(value, "events")? {
        bindings.event_bindings.insert(
            (
                NodeId(id_field(entry, "node")?),
                parse_event_name(string_field(entry, "event")?)?,
            ),
            EventHandlerId(id_field(entry, "handler")?),
        );
    }
    for entry in array_field(value, "conditionals")? {
        bindings
            .conditional_sections
            .entry(NodeId(id_field(entry, "parent")?))
            .or_default()
            .push(ConditionalSection {
                condition: ExprId(id_field(entry, "condition")?),
                then_children: parse_node_id_array(array_field(entry, "then")?)?,
                else_children: parse_node_id_array(array_field(entry, "else")?)?,
            });
    }
    for entry in array_field(value, "inputs")? {
        let node = NodeId(id_field(entry, "node")?);
        bindings.input_bindings.insert(
            node,
            InputBinding {
                node,
                kind: parse_input_kind(string_field(entry, "kind")?)?,
                signal: SignalId(id_field(entry, "signal")?),
            },
        );
    }
    for entry in array_field(value, "attrs")? {
        let node = NodeId(id_field(entry, "node")?);
        let name = string_field(entry, "name")?.to_string();
        bindings.attr_bindings.insert(
            (node, name.clone()),
            AttrBinding {
                node,
                name,
                kind: parse_attr_binding_kind(string_field(entry, "kind")?)?,
                expr: ExprId(id_field(entry, "expr")?),
            },
        );
    }
    for entry in optional_array_field(value, "refs")? {
        let node = NodeId(id_field(entry, "node")?);
        bindings.node_ref_bindings.insert(
            node,
            NodeRefBinding {
                node,
                node_ref: NodeRefId(id_field(entry, "ref")?),
            },
        );
    }
    Ok(bindings)
}

fn parse_queries(values: &[serde_json::Value]) -> Result<Vec<QueryDecl>, String> {
    let mut out = Vec::with_capacity(values.len());
    for (idx, value) in values.iter().enumerate() {
        let id = id_field(value, "id")?;
        expect_dense(idx, id, "queries")?;
        let options = object_field(value, "options")?;
        out.push(QueryDecl {
            id: QueryId(id),
            source_name: value
                .get("source_name")
                .and_then(serde_json::Value::as_str)
                .map(str::to_string),
            source_anchor: source_anchor_field(value),
            key_expr: ExprId(id_field(value, "key_expr")?),
            server_initial: ExprId(id_field_or(value, "server_initial", "server_fetcher")?),
            client_fetcher: ExprId(id_field(value, "client_fetcher")?),
            data_ty: match value.get("data_ty") {
                Some(value) => rsjs_ir::parse_client_type_json(value)
                    .map_err(|err| format!("invalid query data_ty: {err}"))?,
                None => ClientType::Unknown,
            },
            options: QueryOptions {
                interval_ms: option_u32_field(options, "interval_ms")?,
                refetch_on_focus: bool_field(options, "refetch_on_focus")?,
                download_progress: optional_bool_field(options, "download_progress")?
                    .unwrap_or(false),
                stream_text: optional_bool_field(options, "stream_text")?.unwrap_or(false),
                retry_count: id_field(options, "retry_count")?,
                stale_time_ms: id_field(options, "stale_time_ms")?,
            },
        });
    }
    Ok(out)
}

fn parse_query_initializers(
    values: &[serde_json::Value],
) -> Result<Vec<ComponentQueryInitializer>, String> {
    let mut out = Vec::with_capacity(values.len());
    for value in values {
        let id = QueryId(id_field(value, "id")?);
        let expr = expr_tokens_field(value, "expr")?;
        let key_expr = match value.get("key_expr") {
            Some(serde_json::Value::Null) | None => None,
            Some(_) => Some(expr_tokens_field(value, "key_expr")?),
        };
        out.push(ComponentQueryInitializer { id, expr, key_expr });
    }
    out.sort_by_key(|init| init.id.0);
    Ok(out)
}

fn parse_mutation_initializers(
    values: &[serde_json::Value],
) -> Result<Vec<ComponentMutationInitializer>, String> {
    let mut out = Vec::with_capacity(values.len());
    for value in values {
        let id = MutationId(id_field(value, "id")?);
        let key_expr = expr_tokens_field(value, "key_expr")?;
        let invalidates_expr = match value.get("invalidates_expr") {
            Some(serde_json::Value::Null) | None => None,
            Some(_) => Some(expr_tokens_field(value, "invalidates_expr")?),
        };
        out.push(ComponentMutationInitializer {
            id,
            key_expr,
            invalidates_expr,
        });
    }
    out.sort_by_key(|init| init.id.0);
    Ok(out)
}

fn parse_mutations(values: &[serde_json::Value]) -> Result<Vec<MutationDecl>, String> {
    let mut out = Vec::with_capacity(values.len());
    for (idx, value) in values.iter().enumerate() {
        let id = id_field(value, "id")?;
        expect_dense(idx, id, "mutations")?;
        let options = object_field(value, "options")?;
        out.push(MutationDecl {
            id: MutationId(id),
            source_name: value
                .get("source_name")
                .and_then(serde_json::Value::as_str)
                .map(str::to_string),
            source_anchor: source_anchor_field(value),
            key_expr: ExprId(id_field(value, "key_expr")?),
            server_runner: ExprId(id_field(value, "server_runner")?),
            client_runner: ExprId(id_field(value, "client_runner")?),
            invalidates: array_field(value, "invalidates")?
                .iter()
                .map(|v| as_u32(v, "invalidates").map(ExprId))
                .collect::<Result<Vec<_>, _>>()?,
            options: MutationOptions {
                retry_count: id_field(options, "retry_count")?,
                on_success_invalidate_strategy: parse_invalidation_strategy(string_field(
                    options,
                    "on_success_invalidate_strategy",
                )?)?,
            },
        });
    }
    Ok(out)
}

fn parse_view_node(value: &serde_json::Value) -> Result<ViewNode, String> {
    match string_field(value, "kind")? {
        "element" => {
            let static_attrs = array_field(value, "static_attrs")?
                .iter()
                .map(|entry| {
                    Ok((
                        string_field(entry, "name")?.to_string(),
                        string_field(entry, "value")?.to_string(),
                    ))
                })
                .collect::<Result<Vec<_>, String>>()?;
            let server_attrs = optional_array_field(value, "server_attrs")?
                .iter()
                .map(|entry| {
                    Ok(ServerAttr {
                        name: string_field(entry, "name")?.to_string(),
                        tokens: string_field(entry, "tokens")?.to_string(),
                        source: server_surface_source_field(entry),
                        source_anchor: None,
                    })
                })
                .collect::<Result<Vec<_>, String>>()?;
            let payload_attrs = optional_array_field(value, "payload_attrs")?
                .iter()
                .map(|entry| {
                    Ok(PayloadAttr {
                        name: string_field(entry, "name")?.to_string(),
                        tokens: string_field(entry, "tokens")?.to_string(),
                        label: entry
                            .get("label")
                            .and_then(serde_json::Value::as_str)
                            .map(str::to_string),
                        source_anchor: source_anchor_field(entry),
                    })
                })
                .collect::<Result<Vec<_>, String>>()?;
            Ok(ViewNode::Element {
                tag: string_field(value, "tag")?.to_string(),
                children: parse_node_id_array(array_field(value, "children")?)?,
                static_attrs,
                server_attrs,
                payload_attrs,
            })
        }
        "text_binding_slot" => Ok(ViewNode::TextBindingSlot),
        "static_text" => Ok(ViewNode::StaticText(
            string_field(value, "text")?.to_string(),
        )),
        "server_only" => Ok(ViewNode::ServerOnly(ServerOnlyNode {
            tokens: string_field(value, "tokens")?.to_string(),
            source: server_surface_source_field(value),
            source_anchor: None,
        })),
        "for_loop" => Ok(ViewNode::ForLoop {
            attrs: string_field(value, "attrs")?.to_string(),
            label: string_field(value, "label")?.to_string(),
            pat: string_field(value, "pat")?.to_string(),
            expr: string_field(value, "expr")?.to_string(),
            body_prefix: string_field(value, "body_prefix")?.to_string(),
            children: parse_node_id_array(array_field(value, "children")?)?,
        }),
        other => Err(format!("unsupported view node kind {other}")),
    }
}

fn parse_value(value: &serde_json::Value) -> Result<Value, String> {
    match string_field(value, "kind")? {
        "bool" => Ok(Value::Bool(bool_field(value, "value")?)),
        "int" => Ok(Value::Int(i64_field(value, "value")?)),
        "bigint" => Ok(Value::BigInt(i128_field(value, "value")?)),
        "float" => match field(value, "value")? {
            serde_json::Value::Number(number) => number
                .as_f64()
                .map(Value::Float)
                .ok_or_else(|| "float value is not representable as f64".to_string()),
            serde_json::Value::String(s) => s
                .parse::<f64>()
                .map(Value::Float)
                .map_err(|err| err.to_string()),
            _ => Err("float value must be a number or string".to_string()),
        },
        "string" => Ok(Value::String(string_field(value, "value")?.to_string())),
        "null" => Ok(Value::Null),
        other => Err(format!("unsupported value kind {other}")),
    }
}

fn descriptor_matches(
    summary: &ComponentDescriptorSummary,
    last_segment: &str,
    canonical_path: &str,
) -> bool {
    if summary.struct_name == last_segment || last_type_segment(&summary.self_ty) == last_segment {
        return true;
    }
    strip_ws(&summary.self_ty) == canonical_path
}

fn descriptor_dirs() -> Vec<std::path::PathBuf> {
    let phase = match crate::compilation_phase() {
        Ok(phase) => phase,
        // `expand_rsjs` reports the malformed phase environment before
        // analysis starts. Fail closed here as well so a diagnostic helper can
        // never consult ambient descriptor files under an invalid phase.
        Err(_) => return Vec::new(),
    };
    descriptor_dirs_for_phase(phase, plain_descriptor_dirs)
}

fn descriptor_dirs_for_phase(
    phase: crate::CompilationPhase,
    discover_plain_dirs: impl FnOnce() -> Vec<std::path::PathBuf>,
) -> Vec<std::path::PathBuf> {
    if phase.allows_legacy_filesystem_artifacts() {
        discover_plain_dirs()
    } else {
        Vec::new()
    }
}

fn plain_descriptor_dirs() -> Vec<std::path::PathBuf> {
    let mut dirs = Vec::new();
    if let Some(extra_dirs) = std::env::var_os("RSJS_COMPONENT_DESCRIPTOR_DIRS") {
        for dir in std::env::split_paths(&extra_dirs) {
            dirs.push(dir);
        }
    }
    if let Ok(out) = std::env::var("OUT_DIR") {
        dirs.push(
            std::path::PathBuf::from(out)
                .join("rsjs")
                .join("components"),
        );
    }
    if let Ok(target) = std::env::var("CARGO_TARGET_DIR") {
        dirs.push(
            std::path::PathBuf::from(target)
                .join("rsjs")
                .join("components"),
        );
    }
    dirs.push(
        std::path::PathBuf::from("target")
            .join("rsjs")
            .join("components"),
    );
    if let Ok(manifest_dir) = std::env::var("CARGO_MANIFEST_DIR") {
        let manifest_dir = std::path::PathBuf::from(manifest_dir);
        dirs.push(manifest_dir.join("target").join("rsjs").join("components"));
        if let Some(parent) = manifest_dir.parent() {
            dirs.push(parent.join("target").join("rsjs").join("components"));
        }
    }
    dirs.sort();
    dirs.dedup();
    dirs
}

fn required_str<'a>(value: &'a serde_json::Value, field: &str) -> Result<&'a str, String> {
    value
        .get(field)
        .and_then(|v| v.as_str())
        .ok_or_else(|| format!("missing string {field}"))
}

fn required_hash<'a>(value: &'a serde_json::Value, field: &str) -> Result<&'a str, String> {
    let hash = required_str(value, field)?;
    let suffix = hash
        .strip_prefix("sha256:")
        .ok_or_else(|| format!("{field} must start with sha256:"))?;
    if suffix.len() != 64 || !suffix.chars().all(|ch| ch.is_ascii_hexdigit()) {
        return Err(format!("{field} must be a sha256: hex digest"));
    }
    Ok(hash)
}

fn canonical_path(path: &syn::Path) -> String {
    strip_ws(&path.to_token_stream().to_string())
}

fn strip_ws(value: &str) -> String {
    value.chars().filter(|ch| !ch.is_whitespace()).collect()
}

fn last_type_segment(value: &str) -> &str {
    let value = value
        .rsplit("::")
        .next()
        .unwrap_or(value)
        .split('<')
        .next()
        .unwrap_or(value)
        .trim();
    value
}

fn signal_initializer_prop_field(expr: &syn::Expr) -> Option<String> {
    crate::analyze::inbound_signal_self_field(expr).map(|(name, _)| name)
}

fn expr_is_self_path(expr: &syn::Expr) -> bool {
    matches!(
        expr,
        syn::Expr::Path(syn::ExprPath { path, .. })
            if path.segments.len() == 1
                && path
                    .segments
                    .first()
                    .map(|segment| segment.ident == "self")
                    .unwrap_or(false)
    )
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ServerPropField {
    name: String,
    source: &'static str,
}

fn server_only_prop_fields(
    island: &Island,
    signal_initializers: &[crate::emit_rust::SignalInitializer],
) -> Vec<ServerPropField> {
    let mut fields = server_only_prop_field_uses(island);
    fields.extend(
        signal_initializers
            .iter()
            .filter_map(|initializer| signal_initializer_prop_field(&initializer.expr))
            .map(|name| ServerPropField {
                name,
                source: "dead_signal_initializer",
            }),
    );
    fields.sort_by(|left, right| {
        left.name
            .cmp(&right.name)
            .then(server_prop_source_rank(left.source).cmp(&server_prop_source_rank(right.source)))
            .then(left.source.cmp(right.source))
    });
    fields.dedup_by(|left, right| left.name == right.name);
    fields
}

fn server_only_prop_field_uses(island: &Island) -> Vec<ServerPropField> {
    let mut fields = Vec::new();
    for idx in 0..island.view_tree.len() {
        let node = NodeId(idx as u32);
        match island.view_tree.get(node) {
            ViewNode::ServerOnly(server) => {
                collect_server_surface_prop_fields(&server.tokens, server.source, &mut fields)
            }
            ViewNode::Element { server_attrs, .. } => {
                for attr in server_attrs {
                    collect_server_surface_prop_fields(&attr.tokens, attr.source, &mut fields);
                }
            }
            ViewNode::ForLoop {
                expr, body_prefix, ..
            } => {
                collect_self_prop_fields_in_expr_tokens(expr, "server_only_usage", &mut fields);
                collect_self_prop_fields_in_statement_tokens(
                    body_prefix,
                    "server_only_usage",
                    &mut fields,
                );
            }
            _ => {}
        }
    }
    fields
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct StaticSlotField {
    name: String,
}

/// Infer the v7 static-only slot contract.
///
/// The defining component must render `self.<field>` exactly once as a plain,
/// inferred SSR child outside every conditional or loop-controlled subtree.
/// Other field uses, signal seeding, and callback use disqualify the field.
/// This proves that the child preserves the supplied range for its entire
/// mounted lifetime; the call-site transport separately proves the initial
/// range is a fully static `Exact(1)` fragment.
fn immutable_static_slot_fields(
    island: &Island,
    signal_initializers: &[crate::emit_rust::SignalInitializer],
) -> Vec<StaticSlotField> {
    use std::collections::{BTreeMap, BTreeSet};

    let mut use_counts = BTreeMap::<String, usize>::new();
    for field in server_only_prop_field_uses(island) {
        *use_counts.entry(field.name).or_default() += 1;
    }

    let signal_fields = signal_initializers
        .iter()
        .filter_map(|initializer| signal_initializer_prop_field(&initializer.expr))
        .collect::<BTreeSet<_>>();
    let callback_fields = island
        .callback_slots
        .iter()
        .map(|slot| slot.name.clone())
        .collect::<BTreeSet<_>>();
    let lifecycle_owned_nodes = lifecycle_owned_view_nodes(island);

    let mut fields = Vec::new();
    for idx in 0..island.view_tree.len() {
        let node_id = NodeId(idx as u32);
        if lifecycle_owned_nodes.contains(&node_id) {
            continue;
        }
        let ViewNode::ServerOnly(server) = island.view_tree.get(node_id) else {
            continue;
        };
        if server.source != ServerSurfaceSource::InferredSimpleSsr {
            continue;
        }
        let Ok(expr) = syn::parse_str::<syn::Expr>(&server.tokens) else {
            continue;
        };
        let Some(name) = direct_self_field_expr_name(&expr) else {
            continue;
        };
        if use_counts.get(&name) != Some(&1)
            || signal_fields.contains(&name)
            || callback_fields.contains(&name)
        {
            continue;
        }
        fields.push(StaticSlotField { name });
    }
    fields.sort_by(|left, right| left.name.cmp(&right.name));
    fields.dedup_by(|left, right| left.name == right.name);
    fields
}

fn direct_self_field_expr_name(expr: &syn::Expr) -> Option<String> {
    let expr = match expr {
        syn::Expr::Paren(paren) => return direct_self_field_expr_name(&paren.expr),
        syn::Expr::Group(group) => return direct_self_field_expr_name(&group.expr),
        syn::Expr::Reference(reference) => return direct_self_field_expr_name(&reference.expr),
        expr => expr,
    };
    let syn::Expr::Field(field) = expr else {
        return None;
    };
    direct_self_field_name(field)
}

fn lifecycle_owned_view_nodes(island: &Island) -> std::collections::HashSet<NodeId> {
    use std::collections::HashSet;

    let mut roots = HashSet::new();
    for sections in island.bindings.conditional_sections.values() {
        for section in sections {
            roots.extend(section.then_children.iter().copied());
            roots.extend(section.else_children.iter().copied());
        }
    }
    for idx in 0..island.view_tree.len() {
        let node_id = NodeId(idx as u32);
        if matches!(island.view_tree.get(node_id), ViewNode::ForLoop { .. }) {
            roots.insert(node_id);
        }
    }

    let mut owned = HashSet::new();
    let mut stack = roots.into_iter().collect::<Vec<_>>();
    while let Some(node_id) = stack.pop() {
        if !owned.insert(node_id) {
            continue;
        }
        match island.view_tree.get(node_id) {
            ViewNode::Element { children, .. } | ViewNode::ForLoop { children, .. } => {
                stack.extend(children.iter().copied());
            }
            _ => {}
        }
    }
    owned
}

fn server_prop_field_source(source: ServerSurfaceSource) -> &'static str {
    match source {
        ServerSurfaceSource::InferredSimpleSsr => "inferred_simple_ssr",
        ServerSurfaceSource::InferredOpaqueSsr => "inferred_opaque_ssr",
        ServerSurfaceSource::DirectComponentSite => "direct_component_site",
        ServerSurfaceSource::KeyedListItem => "keyed_list_item",
        ServerSurfaceSource::ExplicitIslandBoundary => "explicit_island_boundary",
        ServerSurfaceSource::ExplicitBoundary | ServerSurfaceSource::ExplicitSimpleSsrBoundary => {
            "server_only_usage"
        }
        _ => "server_only_usage",
    }
}

fn server_prop_source_rank(source: &str) -> usize {
    match source {
        "server_only_usage" => 0,
        "inferred_simple_ssr" => 1,
        "inferred_opaque_ssr" => 2,
        "direct_component_site" => 3,
        "keyed_list_item" => 4,
        "explicit_island_boundary" => 5,
        _ => 6,
    }
}

fn collect_server_surface_prop_fields(
    tokens: &str,
    source: ServerSurfaceSource,
    fields: &mut Vec<ServerPropField>,
) {
    let Ok(tokens) = tokens.parse::<proc_macro2::TokenStream>() else {
        return;
    };
    let Ok(expr) = syn::parse2::<syn::Expr>(tokens) else {
        return;
    };
    let source = server_prop_field_source(source);
    let expr = server_prop_inner_expr(&expr).unwrap_or(&expr);
    let mut visitor = ServerOnlyPropVisitor { fields, source };
    visitor.visit_expr(expr);
}

fn collect_self_prop_fields_in_expr_tokens(
    tokens: &str,
    source: &'static str,
    fields: &mut Vec<ServerPropField>,
) {
    let Ok(tokens) = tokens.parse::<proc_macro2::TokenStream>() else {
        return;
    };
    let Ok(expr) = syn::parse2::<syn::Expr>(tokens) else {
        return;
    };
    let mut visitor = ServerOnlyPropVisitor { fields, source };
    visitor.visit_expr(&expr);
}

fn collect_self_prop_fields_in_statement_tokens(
    tokens: &str,
    source: &'static str,
    fields: &mut Vec<ServerPropField>,
) {
    if tokens.trim().is_empty() {
        return;
    }
    let Ok(tokens) = tokens.parse::<proc_macro2::TokenStream>() else {
        return;
    };
    let mut block_tokens = proc_macro2::TokenStream::new();
    proc_macro2::Group::new(proc_macro2::Delimiter::Brace, tokens).to_tokens(&mut block_tokens);
    let Ok(block) = syn::parse2::<syn::Block>(block_tokens) else {
        return;
    };
    let mut visitor = ServerOnlyPropVisitor { fields, source };
    visitor.visit_block(&block);
}

fn server_prop_inner_expr(expr: &syn::Expr) -> Option<&syn::Expr> {
    let syn::Expr::Call(call) = expr else {
        return None;
    };
    let syn::Expr::Path(syn::ExprPath {
        path, qself: None, ..
    }) = call.func.as_ref()
    else {
        return None;
    };
    path.segments
        .last()
        .filter(|seg| seg.ident == "server_only" || seg.ident == "server_attr")?;
    if call.args.len() != 1 {
        return None;
    }
    call.args.first()
}

struct ServerOnlyPropVisitor<'a> {
    fields: &'a mut Vec<ServerPropField>,
    source: &'static str,
}

impl<'ast> Visit<'ast> for ServerOnlyPropVisitor<'_> {
    fn visit_expr_field(&mut self, field: &'ast syn::ExprField) {
        if let Some(name) = direct_self_field_name(field) {
            self.fields.push(ServerPropField {
                name,
                source: self.source,
            });
        }
        visit::visit_expr_field(self, field);
    }

    fn visit_expr_macro(&mut self, expr: &'ast syn::ExprMacro) {
        visit::visit_expr_macro(self, expr);
        if !crate::is_text_intrinsic_path(&expr.mac.path) {
            return;
        }

        // Syn does not visit expressions inside macro token streams. `text!`
        // has a deliberately small `LitStr, Expr, ...` input grammar, so its
        // placeholder arguments are safe to inspect for `self.field` server
        // props. Malformed input is left for text!'s own diagnostic.
        let parser = syn::punctuated::Punctuated::<syn::Expr, syn::Token![,]>::parse_terminated;
        let Ok(args) = syn::parse::Parser::parse2(parser, expr.mac.tokens.clone()) else {
            return;
        };
        if !matches!(
            args.first(),
            Some(syn::Expr::Lit(syn::ExprLit {
                lit: syn::Lit::Str(_),
                ..
            }))
        ) {
            return;
        }
        for argument in args {
            self.visit_expr(&argument);
        }
    }
}

fn direct_self_field_name(field: &syn::ExprField) -> Option<String> {
    if !expr_is_self_path(&field.base) {
        return None;
    }
    match &field.member {
        syn::Member::Named(ident) => Some(ident.to_string()),
        syn::Member::Unnamed(_) => None,
    }
}

fn parse_node_id_array(values: &[serde_json::Value]) -> Result<Vec<NodeId>, String> {
    values
        .iter()
        .map(|value| as_u32(value, "node id").map(NodeId))
        .collect()
}

fn parse_event_name(value: &str) -> Result<EventName, String> {
    EventName::from_descriptor_name(value).ok_or_else(|| format!("unsupported event name {value}"))
}

fn parse_input_kind(value: &str) -> Result<InputKind, String> {
    match value {
        "value" => Ok(InputKind::Value),
        "checked" => Ok(InputKind::Checked),
        "selected" => Ok(InputKind::Selected),
        other => Err(format!("unsupported input kind {other}")),
    }
}

fn parse_attr_binding_kind(value: &str) -> Result<AttrBindingKind, String> {
    match value {
        "value" => Ok(AttrBindingKind::Value),
        "boolean" => Ok(AttrBindingKind::Boolean),
        "url" => Ok(AttrBindingKind::Url),
        other => Err(format!("unsupported attr binding kind {other}")),
    }
}

fn parse_invalidation_strategy(value: &str) -> Result<InvalidationStrategy, String> {
    match value {
        "refetch" => Ok(InvalidationStrategy::Refetch),
        "reset" => Ok(InvalidationStrategy::Reset),
        other => Err(format!("unsupported invalidation strategy {other}")),
    }
}

fn expect_dense(index: usize, id: u32, label: &str) -> Result<(), String> {
    if index as u32 == id {
        Ok(())
    } else {
        Err(format!(
            "{label} must be dense and ordered; expected id {index}, got {id}"
        ))
    }
}

fn field<'a>(value: &'a serde_json::Value, field: &str) -> Result<&'a serde_json::Value, String> {
    value
        .get(field)
        .ok_or_else(|| format!("missing field {field}"))
}

fn optional_field<'a>(
    value: &'a serde_json::Value,
    field: &str,
) -> Result<Option<&'a serde_json::Value>, String> {
    match value.get(field) {
        Some(serde_json::Value::Null) => Ok(None),
        Some(value) => Ok(Some(value)),
        None => Ok(None),
    }
}

fn object_field<'a>(
    value: &'a serde_json::Value,
    field: &str,
) -> Result<&'a serde_json::Value, String> {
    let value = self::field(value, field)?;
    value
        .as_object()
        .ok_or_else(|| format!("{field} must be an object"))?;
    Ok(value)
}

fn array_field<'a>(
    value: &'a serde_json::Value,
    field: &str,
) -> Result<&'a Vec<serde_json::Value>, String> {
    self::field(value, field)?
        .as_array()
        .ok_or_else(|| format!("{field} must be an array"))
}

fn optional_array_field<'a>(
    value: &'a serde_json::Value,
    field: &str,
) -> Result<&'a [serde_json::Value], String> {
    match value.get(field) {
        Some(value) => value
            .as_array()
            .map(Vec::as_slice)
            .ok_or_else(|| format!("{field} must be an array")),
        None => Ok(&[]),
    }
}

fn string_field<'a>(value: &'a serde_json::Value, field: &str) -> Result<&'a str, String> {
    self::field(value, field)?
        .as_str()
        .ok_or_else(|| format!("{field} must be a string"))
}

fn server_surface_source_field(value: &serde_json::Value) -> ServerSurfaceSource {
    match value.get("source").and_then(serde_json::Value::as_str) {
        Some("explicit_boundary") | None => ServerSurfaceSource::ExplicitBoundary,
        Some("explicit_simple_ssr_boundary") => ServerSurfaceSource::ExplicitSimpleSsrBoundary,
        Some("inferred_simple_ssr") => ServerSurfaceSource::InferredSimpleSsr,
        Some("inferred_opaque_ssr") => ServerSurfaceSource::InferredOpaqueSsr,
        Some("direct_component_site") => ServerSurfaceSource::DirectComponentSite,
        Some("keyed_list_item") => ServerSurfaceSource::KeyedListItem,
        Some("explicit_island_boundary") => ServerSurfaceSource::ExplicitIslandBoundary,
        Some(_) => ServerSurfaceSource::ExplicitBoundary,
    }
}

fn source_anchor_field(value: &serde_json::Value) -> Option<rsjs_ir::SourceAnchor> {
    let anchor = value.get("source_anchor")?.as_object()?;
    let start_line = anchor.get("start_line")?.as_u64()? as usize;
    let start_column = anchor.get("start_column")?.as_u64()? as usize;
    Some(rsjs_ir::SourceAnchor {
        start_line,
        start_column,
    })
}

fn expr_tokens_field(value: &serde_json::Value, field: &str) -> Result<syn::Expr, String> {
    let tokens = string_field(value, field)?;
    let tokens = tokens
        .parse::<proc_macro2::TokenStream>()
        .map_err(|err| format!("{field} must parse as Rust expression tokens: {err}"))?;
    syn::parse2::<syn::Expr>(tokens)
        .map_err(|err| format!("{field} must parse as Rust expression: {err}"))
}

fn bool_field(value: &serde_json::Value, field: &str) -> Result<bool, String> {
    self::field(value, field)?
        .as_bool()
        .ok_or_else(|| format!("{field} must be a bool"))
}

fn optional_bool_field(value: &serde_json::Value, field: &str) -> Result<Option<bool>, String> {
    match optional_field(value, field)? {
        Some(value) => value
            .as_bool()
            .map(Some)
            .ok_or_else(|| format!("{field} must be a bool")),
        None => Ok(None),
    }
}

fn i64_field(value: &serde_json::Value, field: &str) -> Result<i64, String> {
    self::field(value, field)?
        .as_i64()
        .ok_or_else(|| format!("{field} must be an i64"))
}

fn i128_field(value: &serde_json::Value, field: &str) -> Result<i128, String> {
    match self::field(value, field)? {
        serde_json::Value::String(value) => value
            .parse()
            .map_err(|_| format!("{field} must be an i128 decimal string")),
        serde_json::Value::Number(value) => value
            .as_i64()
            .map(i128::from)
            .or_else(|| value.as_u64().map(i128::from))
            .ok_or_else(|| format!("{field} must fit in i128")),
        _ => Err(format!("{field} must be an integer or decimal string")),
    }
}

fn id_field(value: &serde_json::Value, field: &str) -> Result<u32, String> {
    as_u32(self::field(value, field)?, field)
}

fn id_field_or(value: &serde_json::Value, preferred: &str, fallback: &str) -> Result<u32, String> {
    if value.get(preferred).is_some() {
        id_field(value, preferred)
    } else {
        id_field(value, fallback)
    }
}

fn option_u32_field(value: &serde_json::Value, field: &str) -> Result<Option<u32>, String> {
    match self::field(value, field)? {
        serde_json::Value::Null => Ok(None),
        other => as_u32(other, field).map(Some),
    }
}

fn as_u32(value: &serde_json::Value, label: &str) -> Result<u32, String> {
    let raw = value
        .as_u64()
        .ok_or_else(|| format!("{label} must be an unsigned integer"))?;
    u32::try_from(raw).map_err(|_| format!("{label} exceeds u32::MAX"))
}

fn push_callback_slots(out: &mut String, island: &Island) {
    out.push_str(",\"callback_slots\":[");
    for (i, slot) in island.callback_slots.iter().enumerate() {
        comma(out, i);
        out.push('{');
        let _ = write!(out, "\"id\":{id},\"name\":", id = slot.id.0);
        push_json_string(out, &slot.name);
        out.push('}');
    }
    out.push(']');
}

fn push_node_refs(out: &mut String, island: &Island) {
    out.push_str(",\"node_refs\":[");
    for (i, node_ref) in island.node_refs.iter().enumerate() {
        comma(out, i);
        out.push('{');
        let _ = write!(out, "\"id\":{id}", id = node_ref.id.0);
        if let Some(name) = &node_ref.source_name {
            out.push_str(",\"source_name\":");
            push_json_string(out, name);
        }
        if let Some(anchor) = node_ref.source_anchor {
            let _ = write!(
                out,
                ",\"source_anchor\":{{\"start_line\":{start_line},\"start_column\":{start_column}",
                start_line = anchor.start_line,
                start_column = anchor.start_column
            );
            out.push('}');
        }
        out.push('}');
    }
    out.push(']');
}

fn push_inferred_fields(
    out: &mut String,
    island: &Island,
    signal_initializers: &[crate::emit_rust::SignalInitializer],
    inbound_inputs: &crate::analyze::InboundInputMetadata,
) {
    out.push_str(",\"fields\":[");
    let mut count = 0usize;
    let static_slots = immutable_static_slot_fields(island, signal_initializers);
    let static_slot_names = static_slots
        .iter()
        .map(|field| field.name.clone())
        .collect::<std::collections::BTreeSet<_>>();
    let inbound_names = inbound_inputs
        .fields
        .iter()
        .map(|field| field.name.as_str())
        .collect::<std::collections::BTreeSet<_>>();

    for field in &inbound_inputs.fields {
        debug_assert_eq!(field.id as usize, count);
        comma(out, count);
        count += 1;
        out.push('{');
        let _ = write!(out, "\"id\":{id},\"name\":", id = field.id);
        push_json_string(out, &field.name);
        match field.role {
            crate::analyze::InboundFieldRole::Value => {
                let _ = write!(
                    out,
                    ",\"kind\":\"prop\",\"value_id\":{value_id}",
                    value_id = field.value.expect("value fields carry a dense value id")
                );
                let source = if inbound_inputs.prop_sites.iter().any(|site| {
                    site.field == field.id
                        && site.temporal == crate::analyze::InboundTemporalMode::SeedOnce
                }) {
                    "signal_initializer"
                } else if inbound_inputs.prop_sites.iter().any(|site| {
                    site.field == field.id
                        && matches!(
                            site.temporal,
                            crate::analyze::InboundTemporalMode::LiveRead
                                | crate::analyze::InboundTemporalMode::QueryKeyDependency
                        )
                }) {
                    "live_read"
                } else if inbound_inputs.prop_sites.iter().any(|site| {
                    site.field == field.id
                        && site.temporal
                            == crate::analyze::InboundTemporalMode::ForbiddenProviderSeed
                }) {
                    "shared_state_provider_seed"
                } else {
                    "explicit_island_boundary"
                };
                out.push_str(",\"source\":");
                push_json_string(out, source);
            }
            crate::analyze::InboundFieldRole::Callback => {
                let _ = write!(
                    out,
                    ",\"kind\":\"handler\",\"value_id\":{value_id}",
                    value_id = field.value.expect("callback fields carry a dense value id")
                );
                if let Some(slot) = island
                    .callback_slots
                    .iter()
                    .find(|slot| slot.name == field.name)
                {
                    let _ = write!(out, ",\"callback_slot\":{id}", id = slot.id.0);
                }
                out.push_str(",\"dispatch\":\"resolve_current\",\"callback_shape\":");
                push_json_string(
                    out,
                    field
                        .callback_shape
                        .expect("callback use records required/optional shape")
                        .as_str(),
                );
                out.push_str(",\"source\":\"callback_slot_usage\"");
            }
            crate::analyze::InboundFieldRole::NodeRef => {
                let _ = write!(
                    out,
                    ",\"kind\":\"node_ref\",\"value_id\":{value_id},\
                     \"source\":\"node_ref_handle_alias\"",
                    value_id = field.value.expect("node-ref fields carry a dense value id")
                );
            }
            crate::analyze::InboundFieldRole::DeferredProjection => {
                let _ = write!(
                    out,
                    ",\"kind\":\"deferred_projection\",\"value_id\":{value_id},\
                     \"source\":\"rustc_linked_child_field\"",
                    value_id = field
                        .value
                        .expect("deferred projections carry a dense value id")
                );
            }
        }
        let _ = write!(
            out,
            ",\"crossing_id\":{crossing_id},\"source_anchor\":",
            crossing_id = field.crossing
        );
        push_source_anchor(out, field.source_anchor);
        out.push('}');
    }
    for field in static_slots {
        if inbound_names.contains(field.name.as_str()) {
            continue;
        }
        comma(out, count);
        let field_id = count;
        count += 1;
        out.push('{');
        let _ = write!(out, "\"id\":{field_id},\"name\":");
        push_json_string(out, &field.name);
        out.push_str(
            ",\"kind\":\"static_slot\",\"source\":\"inferred_single_unconditional_server_slot\",\
             \"span\":\"exact_one\",\"permits_nested_owner\":false",
        );
        out.push('}');
    }
    for field in server_only_prop_fields(island, signal_initializers) {
        if static_slot_names.contains(&field.name) || inbound_names.contains(field.name.as_str()) {
            continue;
        }
        comma(out, count);
        let field_id = count;
        count += 1;
        out.push('{');
        let _ = write!(out, "\"id\":{field_id},\"name\":");
        push_json_string(out, &field.name);
        out.push_str(",\"kind\":\"server_prop\",\"source\":");
        push_json_string(out, field.source);
        out.push('}');
    }
    out.push(']');
}

fn inferred_non_input_field_count(
    island: &Island,
    signal_initializers: &[crate::emit_rust::SignalInitializer],
    inbound_inputs: &crate::analyze::InboundInputMetadata,
) -> usize {
    let inbound_names = inbound_inputs
        .fields
        .iter()
        .map(|field| field.name.as_str())
        .collect::<std::collections::BTreeSet<_>>();
    let static_slots = immutable_static_slot_fields(island, signal_initializers);
    let static_slot_names = static_slots
        .iter()
        .map(|field| field.name.as_str())
        .collect::<std::collections::BTreeSet<_>>();
    let static_count = static_slots
        .iter()
        .filter(|field| !inbound_names.contains(field.name.as_str()))
        .count();
    let server_count = server_only_prop_fields(island, signal_initializers)
        .iter()
        .filter(|field| {
            !inbound_names.contains(field.name.as_str())
                && !static_slot_names.contains(field.name.as_str())
        })
        .count();
    static_count + server_count
}

fn push_inbound_inputs(out: &mut String, inputs: &crate::analyze::InboundInputMetadata) {
    out.push_str(",\"inbound_inputs\":{\"values\":[");
    for (index, value) in inputs.values.iter().enumerate() {
        debug_assert_eq!(value.id as usize, index);
        comma(out, index);
        let _ = write!(
            out,
            "{{\"id\":{id},\"field_id\":{field_id},\"source_anchor\":",
            id = value.id,
            field_id = value.field
        );
        push_source_anchor(out, value.source_anchor);
        out.push('}');
    }
    out.push_str("],\"prop_sites\":[");
    for (index, site) in inputs.prop_sites.iter().enumerate() {
        debug_assert_eq!(site.id as usize, index);
        comma(out, index);
        let _ = write!(
            out,
            "{{\"id\":{id},\"field_id\":{field_id},\"value_id\":{value_id},\
             \"crossing_id\":{crossing_id},\"temporal\":{{\"kind\":",
            id = site.id,
            field_id = site.field,
            value_id = site.value,
            crossing_id = site.crossing
        );
        push_json_string(out, site.temporal.as_str());
        if site.temporal == crate::analyze::InboundTemporalMode::SeedOnce {
            let _ = write!(out, ",\"site_id\":{site_id}", site_id = site.id);
        }
        out.push_str("},\"target\":{");
        match site.target {
            crate::analyze::InboundPropTarget::LiveSignal(signal) => {
                let _ = write!(
                    out,
                    "\"kind\":\"live_signal\",\"signal_id\":{id}",
                    id = signal.0
                );
            }
            crate::analyze::InboundPropTarget::SignalSeed(signal) => {
                let _ = write!(
                    out,
                    "\"kind\":\"signal_seed\",\"signal_id\":{id}",
                    id = signal.0
                );
            }
            crate::analyze::InboundPropTarget::SharedStateProviderSeed => {
                out.push_str("\"kind\":\"shared_state_provider_seed\"");
            }
            crate::analyze::InboundPropTarget::ExplicitIslandBoundary => {
                out.push_str("\"kind\":\"explicit_island_boundary\"");
            }
        }
        out.push_str("},\"source_anchor\":");
        push_source_anchor(out, site.source_anchor);
        out.push('}');
    }
    out.push_str("],\"crossings\":[");
    for (index, crossing) in inputs.crossings.iter().enumerate() {
        debug_assert_eq!(crossing.id as usize, index);
        comma(out, index);
        let _ = write!(
            out,
            "{{\"id\":{id},\"field_id\":{field_id},\"kind\":",
            id = crossing.id,
            field_id = crossing.field
        );
        push_json_string(out, crossing.kind.as_str());
        out.push_str(",\"source_anchor\":");
        push_source_anchor(out, crossing.source_anchor);
        out.push('}');
    }
    out.push_str("],\"node_ref_fields\":[");
    for (index, field) in inputs.node_ref_fields.iter().enumerate() {
        comma(out, index);
        let _ = write!(
            out,
            "{{\"id\":{index},\"field_id\":{field_id},\"value_id\":{value_id},\
             \"crossing_id\":{crossing_id},\"node_ref_id\":{node_ref_id}",
            field_id = field.field,
            value_id = field.value,
            crossing_id = field.crossing,
            node_ref_id = field.node_ref.0
        );
        if let Some(target) = field.bind_target {
            out.push_str(",\"bind_target\":");
            push_json_string(out, target.as_str());
        }
        out.push_str(",\"source_anchor\":");
        push_source_anchor(out, field.source_anchor);
        out.push('}');
    }
    out.push_str("],\"projection_fields\":[");
    for (index, projection) in inputs.projection_fields.iter().enumerate() {
        comma(out, index);
        let _ = write!(
            out,
            "{{\"id\":{index},\"field_id\":{field_id},\"value_id\":{value_id},\
             \"crossing_id\":{crossing_id},\"signal_id\":{signal_id},\
             \"node_ref_id\":{node_ref_id},\"source_anchor\":",
            field_id = projection.field,
            value_id = projection.value,
            crossing_id = projection.crossing,
            signal_id = projection.signal.0,
            node_ref_id = projection.node_ref.0
        );
        push_source_anchor(out, projection.source_anchor);
        out.push('}');
    }
    out.push_str("]}");
}

fn push_source_anchor(out: &mut String, anchor: rsjs_ir::SourceAnchor) {
    let _ = write!(
        out,
        "{{\"start_line\":{start_line},\"start_column\":{start_column}",
        start_line = anchor.start_line,
        start_column = anchor.start_column
    );
    out.push('}');
}

fn push_signals(out: &mut String, island: &Island) {
    out.push_str(",\"signals\":[");
    for (i, sig) in island.signals.iter().enumerate() {
        comma(out, i);
        out.push('{');
        let _ = write!(out, "\"id\":{id}", id = sig.id.0);
        if let Some(name) = &sig.source_name {
            out.push_str(",\"source_name\":");
            push_json_string(out, name);
        }
        if let Some(anchor) = sig.source_anchor {
            let _ = write!(
                out,
                ",\"source_anchor\":{{\"start_line\":{start_line},\"start_column\":{start_column}",
                start_line = anchor.start_line,
                start_column = anchor.start_column
            );
            out.push('}');
        }
        out.push_str(",\"init\":");
        push_value(out, &sig.init);
        out.push_str(",\"ty\":");
        rsjs_ir::write_client_type_json(out, &sig.ty);
        if let Some(domain) = sig.integer_domain {
            out.push_str(",\"integer_domain\":");
            push_json_string(out, domain.as_str());
        }
        if let Some(body) = sig.derived {
            let _ = write!(out, ",\"derived\":{derived}", derived = body.0);
        }
        if let Some(shared) = &sig.shared {
            let _ = write!(
                out,
                ",\"shared\":{{\"store\":{store},\"field\":",
                store = shared.store.0
            );
            push_json_string(out, &shared.field);
            out.push('}');
        }
        out.push('}');
    }
    out.push(']');
}

fn push_shared_stores(out: &mut String, island: &Island) {
    out.push_str(",\"shared_stores\":[");
    for (i, store) in island.shared_stores.iter().enumerate() {
        comma(out, i);
        out.push('{');
        let _ = write!(out, "\"id\":{id},\"source_name\":", id = store.id.0);
        push_json_string(out, &store.source_name);
        out.push_str(",\"rust_type\":");
        push_json_string(out, &store.rust_type);
        if let Some(anchor) = store.source_anchor {
            let _ = write!(
                out,
                ",\"source_anchor\":{{\"start_line\":{start_line},\"start_column\":{start_column}",
                start_line = anchor.start_line,
                start_column = anchor.start_column
            );
            out.push('}');
        }
        out.push('}');
    }
    out.push(']');
}

fn push_expr_arena(out: &mut String, island: &Island) {
    out.push_str(",\"exprs\":[");
    for idx in 0..island.exprs.len() {
        comma(out, idx);
        out.push('{');
        let _ = write!(out, "\"id\":{idx},\"expr\":");
        write_client_expr_json(out, island.exprs.client_expr(rsjs_ir::ExprId(idx as u32)));
        out.push('}');
    }
    out.push(']');
}

fn push_handlers(out: &mut String, island: &Island) {
    out.push_str(",\"handlers\":[");
    for (i, handler) in island.handlers.iter().enumerate() {
        comma(out, i);
        out.push('{');
        let _ = write!(out, "\"id\":{id}", id = handler.id.0);
        if let Some(name) = &handler.source_name {
            out.push_str(",\"source_name\":");
            push_json_string(out, name);
        }
        if let Some(anchor) = handler.source_anchor {
            let _ = write!(
                out,
                ",\"source_anchor\":{{\"start_line\":{start_line},\"start_column\":{start_column}",
                start_line = anchor.start_line,
                start_column = anchor.start_column
            );
            out.push('}');
        }
        if let Some(ms) = handler.debounce_ms() {
            let _ = write!(out, ",\"debounce_ms\":{ms}");
        }
        let _ = write!(
            out,
            ",\"body_ir_version\":{CLIENT_IR_SCHEMA_VERSION},\"body_ir\":"
        );
        write_client_block_json(out, handler.body_ir());
        out.push('}');
    }
    out.push(']');
}

fn push_intervals(out: &mut String, island: &Island) {
    if island.intervals.is_empty() {
        return;
    }
    out.push_str(",\"intervals\":[");
    for (i, interval) in island.intervals.iter().enumerate() {
        comma(out, i);
        out.push('{');
        let _ = write!(
            out,
            "\"ms\":{ms},\"body_ir_version\":{CLIENT_IR_SCHEMA_VERSION},\"body_ir\":",
            ms = interval.ms
        );
        write_client_block_json(out, interval.body_ir());
        out.push('}');
    }
    out.push(']');
}

fn push_view_tree(out: &mut String, island: &Island) {
    out.push_str(",\"view_tree\":{\"root\":");
    match island.view_tree.root() {
        Some(root) => out.push_str(&root.0.to_string()),
        None => out.push_str("null"),
    }
    out.push_str(",\"nodes\":[");
    for idx in 0..island.view_tree.len() {
        comma(out, idx);
        out.push('{');
        let _ = write!(out, "\"id\":{idx},\"node\":");
        push_view_node(out, island.view_tree.get(rsjs_ir::NodeId(idx as u32)));
        out.push('}');
    }
    out.push_str("]}");
}

fn push_keyed_collections(out: &mut String, island: &Island) {
    if island.lists.is_empty() {
        return;
    }
    out.push_str(",\"keyed_collections\":[");
    for (index, list) in island.lists.iter().enumerate() {
        comma(out, index);
        let _ = write!(
            out,
            "{{\"id\":{index},\"container\":{container},\"loop_node\":{loop_node},\
             \"item_root\":{item_root},\"source\":",
            container = list.container.0,
            loop_node = list.loop_node.0,
            item_root = list.item_root.0
        );
        match list.source {
            ListSource::Signal(signal) => {
                let _ = write!(out, "{{\"kind\":\"signal\",\"id\":{id}", id = signal.0);
            }
            ListSource::Expr(expression) => {
                let _ = write!(
                    out,
                    "{{\"kind\":\"expression\",\"id\":{id}",
                    id = expression.0
                );
            }
        }
        out.push('}');
        out.push_str(",\"key\":");
        push_list_item_expr(out, &list.key);
        out.push_str(",\"key_tokens\":");
        push_json_string(out, &list.key_tokens);
        out.push_str(",\"item\":");
        push_list_template_node(out, &list.item);
        out.push('}');
    }
    out.push(']');
}

fn push_list_item_expr(out: &mut String, expression: &ListItemExpr) {
    match expression {
        ListItemExpr::Item => out.push_str("{\"kind\":\"item\"}"),
        ListItemExpr::ItemField(name) => {
            out.push_str("{\"kind\":\"field\",\"name\":");
            push_json_string(out, name);
            out.push('}');
        }
    }
}

fn push_list_template_node(out: &mut String, node: &ListTemplateNode) {
    match node {
        ListTemplateNode::Element {
            tag,
            static_attrs,
            dynamic_attrs,
            children,
        } => {
            out.push_str("{\"kind\":\"element\",\"tag\":");
            push_json_string(out, tag);
            out.push_str(",\"static_attrs\":[");
            for (index, (name, value)) in static_attrs.iter().enumerate() {
                comma(out, index);
                out.push_str("{\"name\":");
                push_json_string(out, name);
                out.push_str(",\"value\":");
                push_json_string(out, value);
                out.push('}');
            }
            out.push_str("],\"dynamic_attrs\":[");
            for (index, (name, value)) in dynamic_attrs.iter().enumerate() {
                comma(out, index);
                out.push_str("{\"name\":");
                push_json_string(out, name);
                out.push_str(",\"value\":");
                push_list_item_expr(out, value);
                out.push('}');
            }
            out.push_str("],\"children\":[");
            for (index, child) in children.iter().enumerate() {
                comma(out, index);
                push_list_template_node(out, child);
            }
            out.push_str("]}");
        }
        ListTemplateNode::Text(text) => {
            out.push_str("{\"kind\":\"text\",\"text\":");
            push_json_string(out, text);
            out.push('}');
        }
        ListTemplateNode::Slot(value) => {
            out.push_str("{\"kind\":\"slot\",\"value\":");
            push_list_item_expr(out, value);
            out.push('}');
        }
    }
}

fn push_bindings(out: &mut String, island: &Island) {
    out.push_str(",\"bindings\":{");
    push_text_bindings(out, island);
    push_event_bindings(out, island);
    push_conditional_sections(out, island);
    push_input_bindings(out, island);
    push_attr_bindings(out, island);
    push_node_ref_bindings(out, island);
    out.push('}');
}

fn push_text_bindings(out: &mut String, island: &Island) {
    out.push_str("\"text\":[");
    let mut entries: Vec<_> = island.bindings.text_bindings.iter().collect();
    entries.sort_by_key(|(node, _)| node.0);
    for (i, (node, expr)) in entries.into_iter().enumerate() {
        comma(out, i);
        out.push('{');
        let _ = write!(
            out,
            "\"node\":{node},\"expr\":{expr}",
            node = node.0,
            expr = expr.0
        );
        out.push('}');
    }
    out.push(']');
}

fn push_event_bindings(out: &mut String, island: &Island) {
    out.push_str(",\"events\":[");
    let mut entries: Vec<_> = island.bindings.event_bindings.iter().collect();
    entries.sort_by(|a, b| {
        let ((a_node, a_event), _) = a;
        let ((b_node, b_event), _) = b;
        (a_node.0, event_name(*a_event)).cmp(&(b_node.0, event_name(*b_event)))
    });
    for (i, ((node, event), handler)) in entries.into_iter().enumerate() {
        comma(out, i);
        out.push('{');
        let _ = write!(out, "\"node\":{node},\"event\":", node = node.0);
        push_json_string(out, event_name(*event));
        let _ = write!(out, ",\"handler\":{handler}", handler = handler.0);
        out.push('}');
    }
    out.push(']');
}

fn push_conditional_sections(out: &mut String, island: &Island) {
    out.push_str(",\"conditionals\":[");
    let mut entries: Vec<_> = island.bindings.conditional_sections.iter().collect();
    entries.sort_by_key(|(parent, _)| parent.0);
    let mut first = true;
    for (parent, sections) in entries {
        for section in sections {
            if !first {
                out.push(',');
            }
            first = false;
            out.push('{');
            let _ = write!(
                out,
                "\"parent\":{parent},\"condition\":{condition},\"then\":",
                parent = parent.0,
                condition = section.condition.0
            );
            push_node_id_array(out, &section.then_children);
            out.push_str(",\"else\":");
            push_node_id_array(out, &section.else_children);
            out.push('}');
        }
    }
    out.push(']');
}

fn push_input_bindings(out: &mut String, island: &Island) {
    out.push_str(",\"inputs\":[");
    let mut entries: Vec<&InputBinding> = island.bindings.input_bindings.values().collect();
    entries.sort_by_key(|binding| binding.node.0);
    for (i, binding) in entries.into_iter().enumerate() {
        comma(out, i);
        out.push('{');
        let _ = write!(out, "\"node\":{node},\"kind\":", node = binding.node.0);
        push_json_string(out, input_kind(binding.kind));
        let _ = write!(out, ",\"signal\":{signal}", signal = binding.signal.0);
        out.push('}');
    }
    out.push(']');
}

fn push_attr_bindings(out: &mut String, island: &Island) {
    out.push_str(",\"attrs\":[");
    let mut entries: Vec<&AttrBinding> = island.bindings.attr_bindings.values().collect();
    entries.sort_by(|a, b| (a.node.0, a.name.as_str()).cmp(&(b.node.0, b.name.as_str())));
    for (i, binding) in entries.into_iter().enumerate() {
        comma(out, i);
        out.push('{');
        let _ = write!(out, "\"node\":{node},\"name\":", node = binding.node.0);
        push_json_string(out, &binding.name);
        out.push_str(",\"kind\":");
        push_json_string(out, attr_binding_kind(binding.kind));
        let _ = write!(out, ",\"expr\":{expr}", expr = binding.expr.0);
        out.push('}');
    }
    out.push(']');
}

fn push_node_ref_bindings(out: &mut String, island: &Island) {
    out.push_str(",\"refs\":[");
    let mut entries: Vec<&NodeRefBinding> = island.bindings.node_ref_bindings.values().collect();
    entries.sort_by_key(|binding| binding.node.0);
    for (i, binding) in entries.into_iter().enumerate() {
        comma(out, i);
        out.push('{');
        let _ = write!(
            out,
            "\"node\":{node},\"ref\":{node_ref}",
            node = binding.node.0,
            node_ref = binding.node_ref.0
        );
        out.push('}');
    }
    out.push(']');
}

fn push_queries(out: &mut String, island: &Island) {
    out.push_str(",\"queries\":[");
    for (i, query) in island.queries.iter().enumerate() {
        comma(out, i);
        push_query(out, query);
    }
    out.push(']');
}

fn push_query(out: &mut String, query: &QueryDecl) {
    out.push('{');
    let _ = write!(out, "\"id\":{id}", id = query.id.0);
    if let Some(name) = &query.source_name {
        out.push_str(",\"source_name\":");
        push_json_string(out, name);
    }
    if let Some(anchor) = query.source_anchor {
        let _ = write!(
            out,
            ",\"source_anchor\":{{\"start_line\":{start_line},\"start_column\":{start_column}",
            start_line = anchor.start_line,
            start_column = anchor.start_column
        );
        out.push('}');
    }
    let _ = write!(
        out,
        ",\"key_expr\":{key_expr},\"server_initial\":{server_initial},\
         \"client_fetcher\":{client_fetcher},\"data_ty\":",
        key_expr = query.key_expr.0,
        server_initial = query.server_initial.0,
        client_fetcher = query.client_fetcher.0
    );
    rsjs_ir::write_client_type_json(&mut *out, &query.data_ty);
    out.push_str(",\"options\":{\"interval_ms\":");
    match query.options.interval_ms {
        Some(ms) => out.push_str(&ms.to_string()),
        None => out.push_str("null"),
    }
    let _ = write!(
        out,
        ",\"refetch_on_focus\":{refetch_on_focus},\"download_progress\":{download_progress},\
         \"stream_text\":{stream_text},\"retry_count\":{retry_count},\
         \"stale_time_ms\":{stale_time_ms}}}}}",
        refetch_on_focus = if query.options.refetch_on_focus {
            "true"
        } else {
            "false"
        },
        download_progress = if query.options.download_progress {
            "true"
        } else {
            "false"
        },
        stream_text = if query.options.stream_text {
            "true"
        } else {
            "false"
        },
        retry_count = query.options.retry_count,
        stale_time_ms = query.options.stale_time_ms
    );
}

fn push_mutations(out: &mut String, island: &Island) {
    out.push_str(",\"mutations\":[");
    for (i, mutation) in island.mutations.iter().enumerate() {
        comma(out, i);
        push_mutation(out, mutation);
    }
    out.push(']');
}

fn push_mutation(out: &mut String, mutation: &MutationDecl) {
    out.push('{');
    let _ = write!(out, "\"id\":{id}", id = mutation.id.0);
    if let Some(name) = &mutation.source_name {
        out.push_str(",\"source_name\":");
        push_json_string(out, name);
    }
    if let Some(anchor) = mutation.source_anchor {
        let _ = write!(
            out,
            ",\"source_anchor\":{{\"start_line\":{start_line},\"start_column\":{start_column}",
            start_line = anchor.start_line,
            start_column = anchor.start_column
        );
        out.push('}');
    }
    let _ = write!(
        out,
        ",\"key_expr\":{key_expr},\"server_runner\":{server_runner},\
         \"client_runner\":{client_runner},\"invalidates\":[",
        key_expr = mutation.key_expr.0,
        server_runner = mutation.server_runner.0,
        client_runner = mutation.client_runner.0
    );
    for (i, invalidates) in mutation.invalidates.iter().enumerate() {
        comma(out, i);
        out.push_str(&invalidates.0.to_string());
    }
    out.push(']');
    let _ = write!(
        out,
        ",\"options\":{{\"retry_count\":{retry_count},\"on_success_invalidate_strategy\":",
        retry_count = mutation.options.retry_count
    );
    push_json_string(
        out,
        match mutation.options.on_success_invalidate_strategy {
            rsjs_ir::InvalidationStrategy::Refetch => "refetch",
            rsjs_ir::InvalidationStrategy::Reset => "reset",
            _ => "unknown",
        },
    );
    out.push_str("}}");
}

fn push_query_initializers(
    out: &mut String,
    query_initializers: &[crate::emit_rust::QueryInitializer],
) {
    if query_initializers.is_empty() {
        return;
    }
    out.push_str(",\"query_initializers\":[");
    for (i, init) in query_initializers.iter().enumerate() {
        comma(out, i);
        out.push('{');
        let _ = write!(out, "\"id\":{id},\"expr\":", id = init.id.0);
        push_json_string(out, &init.expr.to_token_stream().to_string());
        out.push_str(",\"key_expr\":");
        if let Some(key_expr) = &init.key_expr {
            push_json_string(out, &key_expr.to_token_stream().to_string());
        } else {
            out.push_str("null");
        }
        out.push('}');
    }
    out.push(']');
}

fn push_mutation_initializers(
    out: &mut String,
    mutation_initializers: &[crate::emit_rust::MutationInitializer],
) {
    if mutation_initializers.is_empty() {
        return;
    }
    out.push_str(",\"mutation_initializers\":[");
    for (i, init) in mutation_initializers.iter().enumerate() {
        comma(out, i);
        out.push('{');
        let _ = write!(out, "\"id\":{id},\"key_expr\":", id = init.id.0);
        push_json_string(out, &init.key_expr.to_token_stream().to_string());
        out.push_str(",\"invalidates_expr\":");
        if let Some(invalidates_expr) = &init.invalidates_expr {
            push_json_string(out, &invalidates_expr.to_token_stream().to_string());
        } else {
            out.push_str("null");
        }
        out.push('}');
    }
    out.push(']');
}

fn push_view_node(out: &mut String, node: &ViewNode) {
    match node {
        ViewNode::Element {
            tag,
            children,
            static_attrs,
            server_attrs,
            payload_attrs,
        } => {
            out.push('{');
            out.push_str("\"kind\":\"element\",\"tag\":");
            push_json_string(out, tag);
            out.push_str(",\"children\":");
            push_node_id_array(out, children);
            out.push_str(",\"static_attrs\":[");
            for (i, (name, value)) in static_attrs.iter().enumerate() {
                comma(out, i);
                out.push('{');
                out.push_str("\"name\":");
                push_json_string(out, name);
                out.push_str(",\"value\":");
                push_json_string(out, value);
                out.push('}');
            }
            out.push(']');
            out.push_str(",\"server_attrs\":[");
            for (i, attr) in server_attrs.iter().enumerate() {
                comma(out, i);
                out.push('{');
                out.push_str("\"name\":");
                push_json_string(out, &attr.name);
                out.push_str(",\"tokens\":");
                push_json_string(out, &attr.tokens);
                out.push_str(",\"source\":");
                push_json_string(out, attr.source.as_str());
                out.push('}');
            }
            out.push(']');
            out.push_str(",\"payload_attrs\":[");
            for (i, attr) in payload_attrs.iter().enumerate() {
                comma(out, i);
                out.push('{');
                out.push_str("\"name\":");
                push_json_string(out, &attr.name);
                out.push_str(",\"tokens\":");
                push_json_string(out, &attr.tokens);
                if let Some(label) = &attr.label {
                    out.push_str(",\"label\":");
                    push_json_string(out, label);
                }
                if let Some(anchor) = attr.source_anchor {
                    let _ = write!(
                        out,
                        ",\"source_anchor\":{{\"start_line\":{start_line},\
                         \"start_column\":{start_column}",
                        start_line = anchor.start_line,
                        start_column = anchor.start_column
                    );
                    out.push('}');
                }
                out.push('}');
            }
            out.push_str("]}");
        }
        ViewNode::TextBindingSlot => {
            out.push_str("{\"kind\":\"text_binding_slot\"}");
        }
        ViewNode::StaticText(text) => {
            out.push_str("{\"kind\":\"static_text\",\"text\":");
            push_json_string(out, text);
            out.push('}');
        }
        ViewNode::ServerOnly(server) => {
            out.push_str("{\"kind\":\"server_only\",\"tokens\":");
            push_json_string(out, &server.tokens);
            out.push_str(",\"source\":");
            push_json_string(out, server.source.as_str());
            out.push('}');
        }
        ViewNode::ForLoop {
            attrs,
            label,
            pat,
            expr,
            body_prefix,
            children,
        } => {
            out.push('{');
            out.push_str("\"kind\":\"for_loop\",\"attrs\":");
            push_json_string(out, attrs);
            out.push_str(",\"label\":");
            push_json_string(out, label);
            out.push_str(",\"pat\":");
            push_json_string(out, pat);
            out.push_str(",\"expr\":");
            push_json_string(out, expr);
            out.push_str(",\"body_prefix\":");
            push_json_string(out, body_prefix);
            out.push_str(",\"children\":");
            push_node_id_array(out, children);
            out.push('}');
        }
        _ => {
            out.push_str("{\"kind\":\"unknown\"}");
        }
    }
}

fn push_value(out: &mut String, value: &Value) {
    match value {
        Value::Bool(v) => {
            let _ = write!(
                out,
                "{{\"kind\":\"bool\",\"value\":{value}",
                value = if *v { "true" } else { "false" }
            );
            out.push('}');
        }
        Value::Int(v) => {
            let _ = write!(out, "{{\"kind\":\"int\",\"value\":{v}");
            out.push('}');
        }
        Value::BigInt(v) => {
            out.push_str("{\"kind\":\"bigint\",\"value\":");
            push_json_string(out, &v.to_string());
            out.push('}');
        }
        Value::Float(v) => {
            out.push_str("{\"kind\":\"float\",\"value\":");
            if v.is_finite() {
                out.push_str(&v.to_string());
            } else {
                push_json_string(out, &v.to_string());
            }
            out.push('}');
        }
        Value::String(v) => {
            out.push_str("{\"kind\":\"string\",\"value\":");
            push_json_string(out, v);
            out.push('}');
        }
        Value::Null => {
            out.push_str("{\"kind\":\"null\",\"value\":null}");
        }
        _ => {
            out.push_str("{\"kind\":\"unknown\",\"value\":null}");
        }
    }
}

fn push_node_id_array(out: &mut String, ids: &[rsjs_ir::NodeId]) {
    out.push('[');
    for (i, id) in ids.iter().enumerate() {
        comma(out, i);
        out.push_str(&id.0.to_string());
    }
    out.push(']');
}

fn event_name(event: EventName) -> &'static str {
    event.as_descriptor_name()
}

fn input_kind(kind: InputKind) -> &'static str {
    match kind {
        InputKind::Value => "value",
        InputKind::Checked => "checked",
        InputKind::Selected => "selected",
        _ => "unknown",
    }
}

fn attr_binding_kind(kind: AttrBindingKind) -> &'static str {
    match kind {
        AttrBindingKind::Value => "value",
        AttrBindingKind::Boolean => "boolean",
        AttrBindingKind::Url => "url",
        _ => "unknown",
    }
}

fn comma(out: &mut String, index: usize) {
    if index > 0 {
        out.push(',');
    }
}

fn push_json_string(out: &mut String, value: &str) {
    out.push('"');
    for ch in value.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{08}' => out.push_str("\\b"),
            '\u{0c}' => out.push_str("\\f"),
            c if c <= '\u{1f}' => {
                use std::fmt::Write as _;
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

fn prefixed_sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    let digest = hasher.finalize();
    let mut out = String::with_capacity("sha256:".len() + digest.len() * 2);
    out.push_str("sha256:");
    for byte in digest {
        use std::fmt::Write as _;
        let _ = write!(out, "{byte:02x}");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use rsjs_ir::client::{
        ClientBinaryOp, ClientExpr, ClientLocalId, ClientSimpleClosure, ClientStmt, ClientType,
        NodeRefAccessor,
    };
    use rsjs_ir::{EventName, EventValue, IslandBuilder, QueryOptions, Value};

    #[test]
    fn staged_descriptor_lookup_is_identical_with_or_without_preseeded_mirrors() {
        let stale = std::path::PathBuf::from("/stale/target/rsjs/components");
        for phase in [
            crate::CompilationPhase::Descriptor,
            crate::CompilationPhase::Final,
        ] {
            let discoveries = std::cell::Cell::new(0_u8);
            let preseeded = descriptor_dirs_for_phase(phase, || {
                discoveries.set(discoveries.get() + 1);
                vec![stale.clone()]
            });
            let clean = descriptor_dirs_for_phase(phase, Vec::new);

            assert_eq!(preseeded, clean, "{phase:?} observed a stale mirror");
            assert!(preseeded.is_empty());
            assert_eq!(discoveries.get(), 0, "{phase:?} enumerated ambient paths");
        }
    }

    #[test]
    fn plain_descriptor_lookup_still_discovers_tooling_mirrors() {
        let expected = std::path::PathBuf::from("/tooling/target/rsjs/components");
        let discoveries = std::cell::Cell::new(0_u8);
        let actual = descriptor_dirs_for_phase(crate::CompilationPhase::Plain, || {
            discoveries.set(discoveries.get() + 1);
            vec![expected.clone()]
        });

        assert_eq!(actual, vec![expected]);
        assert_eq!(discoveries.get(), 1);
    }

    fn keyed_collection_descriptor_fixture() -> Island {
        let mut builder = IslandBuilder::new();
        let items = builder.signal(Value::Null);
        let items_expr = builder.expr(ClientExpr::SignalRead(items));

        let root = builder.element("main", None);

        let value_container = builder.element("ul", Some(root));
        builder.static_attr(
            value_container,
            "data-rsjs-list",
            &value_container.0.to_string(),
        );
        let value_loop = builder.for_loop(value_container, "", "", "item", "items . get ()", "");
        let value_root = builder.element("li", Some(value_loop));
        builder.text_node_static(value_root, "value: ");
        builder.server_only_node(value_root, "item");
        builder.keyed_list(ListDecl {
            container: value_container,
            loop_node: value_loop,
            item_root: value_root,
            source: ListSource::Signal(items),
            key: ListItemExpr::Item,
            key_tokens: "item".to_owned(),
            item: ListTemplateNode::Element {
                tag: "li".to_owned(),
                static_attrs: vec![(
                    "data-label".to_owned(),
                    "quoted \"value\"\\with\nline".to_owned(),
                )],
                dynamic_attrs: vec![("data-value".to_owned(), ListItemExpr::Item)],
                children: vec![
                    ListTemplateNode::Text("value: \"\\\n".to_owned()),
                    ListTemplateNode::Slot(ListItemExpr::Item),
                ],
            },
        });

        let row_container = builder.element("ol", Some(root));
        builder.static_attr(
            row_container,
            "data-rsjs-list",
            &row_container.0.to_string(),
        );
        let row_loop = builder.for_loop(row_container, "", "", "row", "rows . data ()", "");
        let row_root = builder.element("article", Some(row_loop));
        let row_label = builder.element("span", Some(row_root));
        builder.text_node_static(row_label, "row: ");
        builder.server_only_node(row_label, "row . title");
        builder.keyed_list(ListDecl {
            container: row_container,
            loop_node: row_loop,
            item_root: row_root,
            source: ListSource::Expr(items_expr),
            key: ListItemExpr::ItemField("id".to_owned()),
            key_tokens: "row . id".to_owned(),
            item: ListTemplateNode::Element {
                tag: "article".to_owned(),
                static_attrs: Vec::new(),
                dynamic_attrs: vec![(
                    "data-row".to_owned(),
                    ListItemExpr::ItemField("id".to_owned()),
                )],
                children: vec![ListTemplateNode::Element {
                    tag: "span".to_owned(),
                    static_attrs: vec![("class".to_owned(), "label".to_owned())],
                    dynamic_attrs: Vec::new(),
                    children: vec![
                        ListTemplateNode::Text("row: ".to_owned()),
                        ListTemplateNode::Slot(ListItemExpr::ItemField("title".to_owned())),
                    ],
                }],
            },
        });

        builder.set_root(root);
        builder.build()
    }

    #[test]
    fn schema_v9_round_trips_optional_keyed_collections() {
        let island = keyed_collection_descriptor_fixture();
        let json = to_json(
            "keyed_collection_probe",
            "KeyedCollectionProbe",
            "KeyedCollectionProbe",
            &island,
            "",
        );
        let value: serde_json::Value = serde_json::from_str(&json).expect("valid descriptor");

        assert_eq!(value["counts"]["keyed_collections"], 2);
        assert_eq!(value["keyed_collections"][0]["source"]["kind"], "signal");
        assert_eq!(
            value["keyed_collections"][1]["source"]["kind"],
            "expression"
        );
        assert_eq!(
            value["keyed_collections"][1]["item"]["children"][0]["children"][1]["value"],
            serde_json::json!({"kind": "field", "name": "title"})
        );

        let parsed = parse_descriptor_json(
            std::path::PathBuf::from("keyed_collection_probe.json"),
            &json,
        )
        .expect("keyed-collection descriptor parses")
        .expect("component descriptor");
        assert_eq!(parsed.island.lists, island.lists);

        let empty_json = to_json(
            "empty_keyed_collection_probe",
            "EmptyKeyedCollectionProbe",
            "EmptyKeyedCollectionProbe",
            &Island::new(),
            "",
        );
        let empty_value: serde_json::Value =
            serde_json::from_str(&empty_json).expect("valid empty descriptor");
        assert!(empty_value.get("keyed_collections").is_none());
        assert!(empty_value["counts"].get("keyed_collections").is_none());
        let empty_parsed = parse_descriptor_json(
            std::path::PathBuf::from("empty_keyed_collection_probe.json"),
            &empty_json,
        )
        .expect("the optional keyed-collection section may be absent")
        .expect("component descriptor");
        assert!(empty_parsed.island.lists.is_empty());
    }

    #[test]
    fn schema_v9_rejects_malformed_keyed_collections() {
        let island = keyed_collection_descriptor_fixture();
        let json = to_json(
            "malformed_keyed_collection_probe",
            "MalformedKeyedCollectionProbe",
            "MalformedKeyedCollectionProbe",
            &island,
            "",
        );
        let value: serde_json::Value = serde_json::from_str(&json).expect("valid descriptor");
        let parse_error = |value: &serde_json::Value| {
            parse_descriptor_json(
                std::path::PathBuf::from("malformed_keyed_collection_probe.json"),
                &value.to_string(),
            )
            .expect_err("malformed keyed-collection metadata must fail closed")
        };

        let mut non_array = value.clone();
        non_array["keyed_collections"] = serde_json::json!({});
        assert!(parse_error(&non_array).contains("keyed_collections must be an array"));

        let mut sparse = value.clone();
        sparse["keyed_collections"][0]["id"] = serde_json::json!(4);
        assert!(parse_error(&sparse)
            .contains("keyed_collections must be dense and ordered; expected id 0, got 4"));

        let mut bad_source = value.clone();
        bad_source["keyed_collections"][0]["source"]["kind"] = serde_json::json!("ambient");
        assert!(parse_error(&bad_source).contains("unsupported kind `ambient`"));

        let mut bad_key = value.clone();
        bad_key["keyed_collections"][0]["key"]["kind"] = serde_json::json!("call");
        assert!(
            parse_error(&bad_key).contains("unsupported keyed-list item expression kind `call`")
        );

        let mut empty_field = value.clone();
        empty_field["keyed_collections"][1]["key"]["name"] = serde_json::json!("");
        assert!(parse_error(&empty_field).contains("keyed-list item field name cannot be empty"));

        let mut empty_tag = value.clone();
        empty_tag["keyed_collections"][0]["item"]["tag"] = serde_json::json!("");
        assert!(parse_error(&empty_tag).contains("keyed-list item element tag cannot be empty"));

        let mut bad_child = value;
        bad_child["keyed_collections"][0]["item"]["children"][1]["kind"] =
            serde_json::json!("component");
        assert!(parse_error(&bad_child)
            .contains("unsupported keyed-list template node kind `component`"));
    }

    #[test]
    fn schema_v9_round_trips_direct_component_site_provenance() {
        let mut builder = IslandBuilder::new();
        let root = builder.element("main", None);
        builder.set_root(root);
        let site = builder.server_only_node_with_source(
            root,
            "ImportedBadge { label: title }",
            ServerSurfaceSource::DirectComponentSite,
        );
        let island = builder.build();

        let json = to_json(
            "direct_component_parent",
            "DirectComponentParent",
            "DirectComponentParent",
            &island,
            "",
        );
        let value: serde_json::Value = serde_json::from_str(&json).expect("valid descriptor");
        assert_eq!(
            value["view_tree"]["nodes"][site.0 as usize]["node"]["source"],
            "direct_component_site"
        );

        let parsed = parse_descriptor_json(
            std::path::PathBuf::from("direct_component_parent.json"),
            &json,
        )
        .expect("schema-v9 descriptor parses")
        .expect("component descriptor");
        let ViewNode::ServerOnly(parsed_site) = parsed.island.view_tree.get(site) else {
            panic!("direct component site must round-trip as a server node");
        };
        assert_eq!(parsed_site.source, ServerSurfaceSource::DirectComponentSite);
    }

    #[test]
    fn debug_descriptor_records_static_shared_state_provider_topology() {
        let mut builder = IslandBuilder::new();
        let root = builder.element("main", None);
        builder.set_root(root);
        let provider_node = builder.server_only_node_with_source(
            root,
            "rsjs::SharedStateProvider { state: StateAlias { enabled: true }, children: ImportedChild { label: \"static\" } }",
            ServerSurfaceSource::DirectComponentSite,
        );
        let island = builder.build();
        let providers = [crate::analyze::DirectSharedStateProviderSite {
            id: rsjs_ir::abi::ProviderSiteId::new(0),
            state_ty: syn::parse_quote!(StateAlias),
            child_edge: 2,
            site_node: provider_node,
            source_anchor: rsjs_ir::SourceAnchor {
                start_line: 14,
                start_column: 9,
            },
        }];

        let json = to_json_with_initializers_and_provider_sites(
            "provider_parent",
            "ProviderParent",
            "ProviderParent",
            &island,
            "",
            false,
            &[],
            &crate::analyze::InboundInputMetadata::default(),
            &providers,
            &[],
            &[],
        );
        let value: serde_json::Value = serde_json::from_str(&json).expect("valid descriptor");
        assert_eq!(
            value["shared_state_provider_sites"],
            serde_json::json!([{
                "id": 0,
                "child_edge": 2,
                "site_node": provider_node.0,
                "state_rust_type": "StateAlias",
                "topology": "static_single_wrapperless",
                "source_anchor": {
                    "start_line": 14,
                    "start_column": 9,
                },
            }])
        );
        assert_eq!(value["counts"]["shared_state_provider_sites"], 1);
        assert!(
            parse_descriptor_json(std::path::PathBuf::from("provider_parent.json"), &json)
                .expect("provider debug metadata must not invalidate the descriptor")
                .is_some()
        );
    }

    #[test]
    fn schema_v9_round_trips_live_read_signal_targets() {
        let mut builder = IslandBuilder::new();
        let signal = builder.signal(Value::String(String::new()));
        let island = builder.build();
        let anchor = rsjs_ir::SourceAnchor {
            start_line: 8,
            start_column: 21,
        };
        let inputs = crate::analyze::InboundInputMetadata {
            fields: vec![crate::analyze::InboundField {
                id: 0,
                name: "value".to_owned(),
                role: crate::analyze::InboundFieldRole::Value,
                value: Some(0),
                callback_shape: None,
                crossing: 0,
                source_anchor: anchor,
            }],
            values: vec![crate::analyze::InboundValue {
                id: 0,
                field: 0,
                source_anchor: anchor,
            }],
            prop_sites: vec![
                crate::analyze::InboundPropSite {
                    id: 0,
                    field: 0,
                    value: 0,
                    crossing: 0,
                    temporal: crate::analyze::InboundTemporalMode::LiveRead,
                    target: crate::analyze::InboundPropTarget::LiveSignal(signal),
                    source_anchor: anchor,
                },
                crate::analyze::InboundPropSite {
                    id: 1,
                    field: 0,
                    value: 0,
                    crossing: 0,
                    temporal: crate::analyze::InboundTemporalMode::LiveRead,
                    target: crate::analyze::InboundPropTarget::LiveSignal(signal),
                    source_anchor: anchor,
                },
                crate::analyze::InboundPropSite {
                    id: 2,
                    field: 0,
                    value: 0,
                    crossing: 0,
                    temporal: crate::analyze::InboundTemporalMode::QueryKeyDependency,
                    target: crate::analyze::InboundPropTarget::LiveSignal(signal),
                    source_anchor: anchor,
                },
            ],
            crossings: vec![crate::analyze::InboundCrossing {
                id: 0,
                field: 0,
                kind: crate::analyze::InboundCrossingKind::Value,
                source_anchor: anchor,
            }],
            node_ref_fields: vec![],
            projection_fields: vec![],
        };

        let json = to_json_with_initializers(
            "live_input_probe",
            "LiveInputProbe",
            "LiveInputProbe",
            &island,
            "",
            false,
            &[],
            &inputs,
            &[],
            &[],
        );
        let value: serde_json::Value = serde_json::from_str(&json).expect("valid descriptor");
        assert_eq!(value["fields"][0]["source"], "live_read");
        assert_eq!(
            value["inbound_inputs"]["prop_sites"][0]["temporal"],
            serde_json::json!({"kind": "live_read"})
        );
        assert_eq!(
            value["inbound_inputs"]["prop_sites"][0]["target"],
            serde_json::json!({"kind": "live_signal", "signal_id": 0})
        );
        assert_eq!(
            value["inbound_inputs"]["prop_sites"][1]["target"],
            value["inbound_inputs"]["prop_sites"][0]["target"],
            "template reads and pass-through props share one inbound live-value identity"
        );
        assert_eq!(
            value["inbound_inputs"]["prop_sites"][2]["temporal"],
            serde_json::json!({"kind": "query_key_dependency"})
        );
        assert_eq!(
            value["inbound_inputs"]["prop_sites"][2]["target"],
            value["inbound_inputs"]["prop_sites"][0]["target"],
            "query-key dependencies reuse the same current inbound signal"
        );

        let parsed =
            parse_descriptor_json(std::path::PathBuf::from("live_input_probe.json"), &json)
                .expect("schema-v9 descriptor parses")
                .expect("component descriptor");
        assert_eq!(parsed.prop_fields[0].name, "value");
        assert_eq!(parsed.prop_fields[0].signal, signal);
        assert_eq!(
            parsed.inbound_inputs.prop_sites[0].temporal,
            crate::analyze::InboundTemporalMode::LiveRead
        );
        assert_eq!(
            parsed.inbound_inputs.prop_sites[0].target,
            crate::analyze::InboundPropTarget::LiveSignal(signal)
        );
        assert_eq!(
            parsed.inbound_inputs.prop_sites[1].target,
            crate::analyze::InboundPropTarget::LiveSignal(signal)
        );
        assert_eq!(
            parsed.inbound_inputs.prop_sites[2].temporal,
            crate::analyze::InboundTemporalMode::QueryKeyDependency
        );
        assert_eq!(
            parsed.inbound_inputs.prop_sites[2].target,
            crate::analyze::InboundPropTarget::LiveSignal(signal)
        );

        let mut malformed = value;
        malformed["inbound_inputs"]["prop_sites"][2]["target"]["kind"] =
            serde_json::json!("signal_seed");
        let error = parse_descriptor_json(
            std::path::PathBuf::from("query_key_dependency_bad_target.json"),
            &malformed.to_string(),
        )
        .expect_err("a query-key dependency must target the reusable live signal");
        assert!(
            error.contains("without SeedOnce semantics"),
            "query-key target validation must fail closed: {error}"
        );
    }

    #[test]
    fn schema_v9_round_trips_required_node_ref_aliases_and_binding_targets() {
        let mut builder = IslandBuilder::new();
        let node_ref = builder.node_ref();
        let input = builder.element("input", None);
        builder.bind_node_ref(input, node_ref);
        builder.set_root(input);
        let island = builder.build();
        let anchor = rsjs_ir::SourceAnchor {
            start_line: 9,
            start_column: 17,
        };
        let inputs = crate::analyze::InboundInputMetadata {
            fields: vec![crate::analyze::InboundField {
                id: 0,
                name: "input_ref".to_owned(),
                role: crate::analyze::InboundFieldRole::NodeRef,
                value: Some(0),
                callback_shape: None,
                crossing: 0,
                source_anchor: anchor,
            }],
            values: vec![crate::analyze::InboundValue {
                id: 0,
                field: 0,
                source_anchor: anchor,
            }],
            prop_sites: vec![],
            crossings: vec![crate::analyze::InboundCrossing {
                id: 0,
                field: 0,
                kind: crate::analyze::InboundCrossingKind::NodeRefAlias,
                source_anchor: anchor,
            }],
            node_ref_fields: vec![crate::analyze::InboundNodeRefField {
                field: 0,
                value: 0,
                crossing: 0,
                node_ref,
                bind_target: Some(crate::analyze::NodeRefBindTarget::Input),
                exact_bind_target: Some(rsjs_ir::dom_registry::DomElementKind::HtmlInputElement),
                source_anchor: anchor,
            }],
            projection_fields: vec![],
        };

        let json = to_json_with_initializers(
            "input_ref_probe",
            "InputRefProbe",
            "InputRefProbe",
            &island,
            "",
            false,
            &[],
            &inputs,
            &[],
            &[],
        );
        let value: serde_json::Value = serde_json::from_str(&json).expect("valid descriptor");
        assert_eq!(value["fields"][0]["kind"], "node_ref");
        assert_eq!(value["fields"][0]["source"], "node_ref_handle_alias");
        assert_eq!(
            value["inbound_inputs"]["crossings"][0]["kind"],
            "node_ref_alias"
        );
        assert_eq!(
            value["inbound_inputs"]["node_ref_fields"][0]["bind_target"],
            "input"
        );

        let parsed = parse_descriptor_json(std::path::PathBuf::from("input_ref_probe.json"), &json)
            .expect("schema-v9 node-ref descriptor parses")
            .expect("component descriptor");
        assert_eq!(parsed.inbound_inputs.node_ref_fields.len(), 1);
        assert_eq!(
            parsed.inbound_inputs.node_ref_fields[0].bind_target,
            Some(crate::analyze::NodeRefBindTarget::Input)
        );
        assert_eq!(parsed.inbound_inputs.node_ref_fields[0].node_ref, node_ref);

        let mut optional = value;
        optional["fields"][0]["value_id"] = serde_json::Value::Null;
        let error = parse_descriptor_json(
            std::path::PathBuf::from("input_ref_probe_optional.json"),
            &optional.to_string(),
        )
        .expect_err("a node-ref field cannot lose its required value identity");
        assert!(
            error.contains("value_id must be an unsigned integer"),
            "required-handle validation must fail closed: {error}"
        );
    }

    #[test]
    fn schema_v9_round_trips_dense_latent_input_timing_and_callback_values() {
        let mut builder = IslandBuilder::new();
        let signal = builder.signal(Value::String(String::new()));
        builder.callback_slot("on_change");
        let island = builder.build();
        let anchor = rsjs_ir::SourceAnchor {
            start_line: 12,
            start_column: 17,
        };
        let inputs = crate::analyze::InboundInputMetadata {
            fields: vec![
                crate::analyze::InboundField {
                    id: 0,
                    name: "initial".to_owned(),
                    role: crate::analyze::InboundFieldRole::Value,
                    value: Some(0),
                    callback_shape: None,
                    crossing: 0,
                    source_anchor: anchor,
                },
                crate::analyze::InboundField {
                    id: 1,
                    name: "on_change".to_owned(),
                    role: crate::analyze::InboundFieldRole::Callback,
                    value: Some(1),
                    callback_shape: Some(crate::analyze::InboundCallbackShape::Required),
                    crossing: 1,
                    source_anchor: anchor,
                },
            ],
            values: vec![
                crate::analyze::InboundValue {
                    id: 0,
                    field: 0,
                    source_anchor: anchor,
                },
                crate::analyze::InboundValue {
                    id: 1,
                    field: 1,
                    source_anchor: anchor,
                },
            ],
            prop_sites: vec![
                crate::analyze::InboundPropSite {
                    id: 0,
                    field: 0,
                    value: 0,
                    crossing: 0,
                    temporal: crate::analyze::InboundTemporalMode::SeedOnce,
                    target: crate::analyze::InboundPropTarget::SignalSeed(signal),
                    source_anchor: anchor,
                },
                crate::analyze::InboundPropSite {
                    id: 1,
                    field: 0,
                    value: 0,
                    crossing: 0,
                    temporal: crate::analyze::InboundTemporalMode::ForbiddenBoundaryCrossing,
                    target: crate::analyze::InboundPropTarget::ExplicitIslandBoundary,
                    source_anchor: anchor,
                },
            ],
            crossings: vec![
                crate::analyze::InboundCrossing {
                    id: 0,
                    field: 0,
                    kind: crate::analyze::InboundCrossingKind::Value,
                    source_anchor: anchor,
                },
                crate::analyze::InboundCrossing {
                    id: 1,
                    field: 1,
                    kind: crate::analyze::InboundCrossingKind::CallbackResolveCurrent,
                    source_anchor: anchor,
                },
            ],
            node_ref_fields: vec![],
            projection_fields: vec![],
        };
        let json = to_json_with_initializers(
            "input_probe",
            "InputProbe",
            "InputProbe",
            &island,
            "",
            true,
            &[],
            &inputs,
            &[],
            &[],
        );
        let value: serde_json::Value = serde_json::from_str(&json).expect("valid descriptor");
        assert_eq!(value["schema_version"], 9);
        assert_eq!(value["fields"][1]["value_id"], 1);
        assert_eq!(value["fields"][1]["callback_shape"], "required");
        assert_eq!(
            value["inbound_inputs"]["prop_sites"][0]["temporal"],
            serde_json::json!({"kind": "seed_once", "site_id": 0})
        );
        assert_eq!(
            value["inbound_inputs"]["crossings"][1]["kind"],
            "callback_resolve_current"
        );

        let parsed = parse_descriptor_json(std::path::PathBuf::from("input_probe.json"), &json)
            .expect("schema-v9 descriptor parses")
            .expect("component descriptor");
        assert_eq!(parsed.inbound_inputs.fields.len(), 2);
        assert_eq!(parsed.inbound_inputs.values.len(), 2);
        assert_eq!(parsed.inbound_inputs.fields[1].value, Some(1));
        assert_eq!(
            parsed.inbound_inputs.fields[1].callback_shape,
            Some(crate::analyze::InboundCallbackShape::Required)
        );
        assert_eq!(
            parsed.inbound_inputs.prop_sites[0].temporal,
            crate::analyze::InboundTemporalMode::SeedOnce
        );
        assert_eq!(parsed.prop_fields[0].signal, signal);

        let mut missing_callback_value = value.clone();
        missing_callback_value["inbound_inputs"]["values"] =
            serde_json::json!([missing_callback_value["inbound_inputs"]["values"][0].clone()]);
        let error = parse_descriptor_json(
            std::path::PathBuf::from("input_probe_missing_callback_value.json"),
            &missing_callback_value.to_string(),
        )
        .expect_err("every callback field must own a dense symbolic value");
        assert!(error.contains("values must be one-to-one with inbound fields"));

        let mut missing_callback_shape = value.clone();
        missing_callback_shape["fields"][1]
            .as_object_mut()
            .expect("callback field object")
            .remove("callback_shape");
        let error = parse_descriptor_json(
            std::path::PathBuf::from("input_probe_missing_callback_shape.json"),
            &missing_callback_shape.to_string(),
        )
        .expect_err("callback required/optional shape is part of schema v9");
        assert!(error.contains("missing field callback_shape"));

        let mut aliased_callback_value = value.clone();
        aliased_callback_value["fields"][1]["value_id"] = serde_json::json!(0);
        let error = parse_descriptor_json(
            std::path::PathBuf::from("input_probe_aliased_callback_value.json"),
            &aliased_callback_value.to_string(),
        )
        .expect_err("two inbound fields cannot alias one symbolic value");
        assert!(error.contains("inbound value 1 does not match field `on_change` value_id"));

        let mut malformed = value;
        malformed["inbound_inputs"]["prop_sites"][0]["temporal"]["site_id"] = serde_json::json!(9);
        let error = parse_descriptor_json(
            std::path::PathBuf::from("input_probe_bad.json"),
            &malformed.to_string(),
        )
        .expect_err("SeedOnce must reference its exact dense prop site");
        assert!(error.contains("SeedOnce references different site 9"));
    }

    #[test]
    fn schema_v9_round_trips_forbidden_shared_state_provider_seed() {
        let mut builder = IslandBuilder::new();
        let root = builder.element("main", None);
        builder.text_node_static(root, "server topology");
        builder.set_root(root);
        let island = builder.build();
        let anchor = rsjs_ir::SourceAnchor {
            start_line: 19,
            start_column: 23,
        };
        let inputs = crate::analyze::InboundInputMetadata {
            fields: vec![crate::analyze::InboundField {
                id: 0,
                name: "mode".to_owned(),
                role: crate::analyze::InboundFieldRole::Value,
                value: Some(0),
                callback_shape: None,
                crossing: 0,
                source_anchor: anchor,
            }],
            values: vec![crate::analyze::InboundValue {
                id: 0,
                field: 0,
                source_anchor: anchor,
            }],
            prop_sites: vec![crate::analyze::InboundPropSite {
                id: 0,
                field: 0,
                value: 0,
                crossing: 0,
                temporal: crate::analyze::InboundTemporalMode::ForbiddenProviderSeed,
                target: crate::analyze::InboundPropTarget::SharedStateProviderSeed,
                source_anchor: anchor,
            }],
            crossings: vec![crate::analyze::InboundCrossing {
                id: 0,
                field: 0,
                kind: crate::analyze::InboundCrossingKind::Value,
                source_anchor: anchor,
            }],
            node_ref_fields: vec![],
            projection_fields: vec![],
        };
        let json = to_json_with_initializers(
            "provider_child",
            "ProviderChild",
            "ProviderChild",
            &island,
            "",
            false,
            &[],
            &inputs,
            &[],
            &[],
        );
        let value: serde_json::Value = serde_json::from_str(&json).expect("valid descriptor");
        assert_eq!(value["fields"][0]["source"], "shared_state_provider_seed");
        assert_eq!(
            value["inbound_inputs"]["prop_sites"][0]["temporal"],
            serde_json::json!({"kind": "forbidden_provider_seed"})
        );
        assert_eq!(
            value["inbound_inputs"]["prop_sites"][0]["target"],
            serde_json::json!({"kind": "shared_state_provider_seed"})
        );

        let parsed = parse_descriptor_json(std::path::PathBuf::from("provider_child.json"), &json)
            .expect("provider temporal contract parses")
            .expect("component descriptor");
        assert_eq!(
            parsed.inbound_inputs.prop_sites[0].temporal,
            crate::analyze::InboundTemporalMode::ForbiddenProviderSeed
        );
        assert_eq!(
            parsed.inbound_inputs.prop_sites[0].target,
            crate::analyze::InboundPropTarget::SharedStateProviderSeed
        );
        assert!(
            parsed.prop_fields.is_empty(),
            "a forbidden provider seed has no browser signal target"
        );

        let mut malformed = value;
        malformed["inbound_inputs"]["prop_sites"][0]["target"]["kind"] =
            serde_json::json!("explicit_island_boundary");
        let error = parse_descriptor_json(
            std::path::PathBuf::from("provider_child_bad_target.json"),
            &malformed.to_string(),
        )
        .expect_err("provider temporal mode must keep its exact structural target");
        assert!(
            error.contains("without forbidden-boundary semantics"),
            "target/temporal mismatch must fail closed: {error}"
        );
    }

    #[test]
    fn dead_signal_seed_downgrades_to_a_server_prop_without_a_dangling_target() {
        let mut builder = IslandBuilder::new();
        let signal = builder.signal(Value::Int(7));
        let root = builder.element("article", None);
        builder.text_node_static(root, "Static server output");
        builder.set_root(root);
        let mut island = builder.build();
        let anchor = rsjs_ir::SourceAnchor {
            start_line: 5,
            start_column: 29,
        };
        let mut inputs = crate::analyze::InboundInputMetadata {
            fields: vec![crate::analyze::InboundField {
                id: 0,
                name: "initial".to_owned(),
                role: crate::analyze::InboundFieldRole::Value,
                value: Some(0),
                callback_shape: None,
                crossing: 0,
                source_anchor: anchor,
            }],
            values: vec![crate::analyze::InboundValue {
                id: 0,
                field: 0,
                source_anchor: anchor,
            }],
            prop_sites: vec![crate::analyze::InboundPropSite {
                id: 0,
                field: 0,
                value: 0,
                crossing: 0,
                temporal: crate::analyze::InboundTemporalMode::SeedOnce,
                target: crate::analyze::InboundPropTarget::SignalSeed(signal),
                source_anchor: anchor,
            }],
            crossings: vec![crate::analyze::InboundCrossing {
                id: 0,
                field: 0,
                kind: crate::analyze::InboundCrossingKind::Value,
                source_anchor: anchor,
            }],
            node_ref_fields: vec![],
            projection_fields: vec![],
        };
        crate::analyze::prune_dead_signal_seed_inputs(&mut inputs, &[]);
        rsjs_ir::default_passes().run_all(&mut island);
        let signal_initializers = [crate::emit_rust::SignalInitializer {
            id: signal,
            expr: syn::parse_quote!(self.initial),
            explicit_ty: None,
        }];

        let json = to_json_with_initializers(
            "seed_only_card",
            "SeedOnlyCard",
            "SeedOnlyCard",
            &island,
            "",
            true,
            &signal_initializers,
            &inputs,
            &[],
            &[],
        );
        let value: serde_json::Value = serde_json::from_str(&json).expect("valid descriptor");
        assert_eq!(value["schema_version"], 9);
        assert_eq!(value["signals"], serde_json::json!([]));
        assert_eq!(value["inbound_inputs"]["values"], serde_json::json!([]));
        assert_eq!(value["inbound_inputs"]["prop_sites"], serde_json::json!([]));
        assert_eq!(value["inbound_inputs"]["crossings"], serde_json::json!([]));
        assert_eq!(
            value["fields"],
            serde_json::json!([{
                "id": 0,
                "name": "initial",
                "kind": "server_prop",
                "source": "dead_signal_initializer",
            }])
        );

        let parsed = parse_descriptor_json(std::path::PathBuf::from("seed_only_card.json"), &json)
            .expect("dead signal seed descriptor must remain internally valid")
            .expect("component descriptor");
        assert!(parsed.inbound_inputs.fields.is_empty());
        assert_eq!(parsed.summary.server_prop_fields, vec!["initial"]);
    }

    #[test]
    fn descriptor_json_contains_full_ir_sections() {
        let mut b = IslandBuilder::new();
        let signal = b.signal(Value::Int(0));
        let shared_store = b.shared_store(
            "workbench".to_string(),
            "crate::WorkbenchState".to_string(),
            Some(rsjs_ir::SourceAnchor {
                start_line: 12,
                start_column: 9,
            }),
        );
        let shared_signal =
            b.shared_field(shared_store, "button_bg".to_string(), ClientType::String);
        let root = b.element("div", None);
        b.static_attr(root, "class", "counter");
        b.set_root(root);
        let button = b.element("button", Some(root));
        b.text_node_static(button, "Increment");
        let text_expr = b.expr(ClientExpr::SignalRead(signal));
        b.bind_text(root, text_expr);
        let slot = b.callback_slot("on_change");
        let handler = b.handler_from_ir(ClientBlock::from_stmts(vec![
            ClientStmt::signal_update(
                signal,
                ClientSimpleClosure::new(
                    "count",
                    ClientExpr::binary(
                        ClientBinaryOp::Add,
                        ClientExpr::signal_read(signal),
                        ClientExpr::int(1),
                    ),
                ),
            ),
            ClientStmt::CallbackInvoke {
                callback: slot,
                payload: ClientExpr::signal_read(signal),
            },
        ]));
        b.bind_event(button, EventName::Click, handler);
        let key = b.expr(ClientExpr::Literal(Value::String("items".to_string())));
        let fetcher = b.expr(ClientExpr::Literal(Value::String("/api/items".to_string())));
        b.query(key, fetcher, fetcher, QueryOptions::default());

        let island = b.build();
        let json = to_json(
            "counter_1234",
            "Counter",
            "Counter",
            &island,
            "console.log('counter');",
        );
        let parsed: serde_json::Value = serde_json::from_str(&json).expect("valid JSON");
        let descriptor =
            parse_descriptor_json(std::path::PathBuf::from("counter_1234.json"), &json)
                .expect("descriptor parses")
                .expect("component descriptor");

        assert_eq!(parsed["schema_version"], 9);
        assert_eq!(parsed["client_origin_permission"], "origin_capable");
        assert_eq!(parsed["format"], "rsjs.component_descriptor");
        assert_eq!(parsed["client_ir_version"], CLIENT_IR_SCHEMA_VERSION);
        assert_hash_shape(&parsed["js_hash"]);
        assert_hash_shape(&parsed["descriptor_hash"]);
        assert_eq!(parsed["callback_slots"][0]["name"], "on_change");
        assert_eq!(parsed["fields"][0]["kind"], "handler");
        assert_eq!(parsed["shared_stores"][0]["source_name"], "workbench");
        assert_eq!(
            parsed["shared_stores"][0]["rust_type"],
            "crate::WorkbenchState"
        );
        assert_eq!(parsed["signals"][0]["init"]["value"], 0);
        assert_eq!(parsed["signals"][1]["shared"]["store"], 0);
        assert_eq!(parsed["signals"][1]["shared"]["field"], "button_bg");
        assert_eq!(island.signals[1].id, shared_signal);
        assert_eq!(parsed["exprs"][0]["expr"]["kind"], "signal_read");
        assert_eq!(
            parsed["handlers"][0].get("statements"),
            None,
            "descriptors should serialize v3 handler bodies only"
        );
        assert_eq!(
            parsed["handlers"][0]["body_ir_version"],
            CLIENT_IR_SCHEMA_VERSION
        );
        assert_eq!(parsed["handlers"][0]["body_ir"]["kind"], "block");
        assert_eq!(
            parsed["handlers"][0]["body_ir"]["stmts"][1]["kind"],
            "callback_invoke"
        );
        assert_eq!(parsed["view_tree"]["nodes"][0]["node"]["kind"], "element");
        assert_eq!(parsed["bindings"]["events"][0]["event"], "click");
        assert_eq!(parsed["queries"][0]["client_fetcher"], 2);
        assert_eq!(parsed["counts"]["callback_slots"], 1);
        assert_eq!(descriptor.summary.asset_id, "counter_1234");
        assert_eq!(
            descriptor.summary.client_origin_permission,
            ComponentOriginPermission::OriginCapable
        );
        assert_eq!(descriptor.island, island);
    }

    #[test]
    fn descriptor_round_trips_participant_only_origin_permission() {
        let island = Island::new();
        let json = to_json_with_initializers(
            "static_card_1234",
            "StaticCard",
            "StaticCard",
            &island,
            "",
            false,
            &[],
            &crate::analyze::InboundInputMetadata::default(),
            &[],
            &[],
        );
        let descriptor =
            parse_descriptor_json(std::path::PathBuf::from("static_card_1234.json"), &json)
                .expect("participant descriptor parses")
                .expect("component descriptor");

        assert_eq!(
            descriptor.summary.client_origin_permission,
            ComponentOriginPermission::ParticipantOnly
        );
    }

    #[test]
    fn descriptor_rejects_unknown_origin_permission() {
        let json = to_json(
            "static_card_1234",
            "StaticCard",
            "StaticCard",
            &Island::new(),
            "",
        );
        let mut value: serde_json::Value = serde_json::from_str(&json).expect("valid descriptor");
        value["client_origin_permission"] = serde_json::json!("sometimes");

        let error = parse_descriptor_json(
            std::path::PathBuf::from("static_card_1234.json"),
            &value.to_string(),
        )
        .expect_err("unknown origin permission should fail");

        assert!(error.contains("unsupported client_origin_permission `sometimes`"));
    }

    #[test]
    fn descriptor_parser_accepts_v3_only_handler_bodies() {
        let mut b = IslandBuilder::new();
        let signal = b.signal(Value::Int(0));
        let root = b.element("button", None);
        b.set_root(root);
        let handler = b.handler_from_ir(ClientBlock::from_stmts(vec![
            ClientStmt::let_value(
                ClientLocalId(0),
                "next",
                ClientType::Unknown,
                ClientExpr::EventRead(EventValue::TargetValue),
            ),
            ClientStmt::if_(
                ClientExpr::IsNonEmpty {
                    target: Box::new(ClientExpr::local(ClientLocalId(0))),
                },
                ClientBlock::from_stmts(vec![ClientStmt::signal_set(
                    signal,
                    ClientExpr::local(ClientLocalId(0)),
                )]),
            ),
        ]));
        b.bind_event(root, EventName::Input, handler);
        let island = b.build();
        let json = to_json("v3_only_1234", "V3Only", "V3Only", &island, "");
        let descriptor =
            parse_descriptor_json(std::path::PathBuf::from("v3_only_1234.json"), &json)
                .expect("descriptor parses")
                .expect("component descriptor");

        assert_eq!(descriptor.island, island);
    }

    #[test]
    fn descriptor_round_trips_handler_local_current_target_reads() {
        let mut b = IslandBuilder::new();
        let signal = b.signal(Value::String(String::new()));
        let root = b.element("input", None);
        b.set_root(root);
        let target = ClientLocalId(0);
        let handler = b.handler_from_ir(ClientBlock::from_stmts(vec![
            ClientStmt::let_value(
                target,
                "input",
                ClientType::DomElement,
                ClientExpr::EventCurrentTarget,
            ),
            ClientStmt::signal_set(
                signal,
                ClientExpr::DomRead {
                    target: Box::new(ClientExpr::Local(target)),
                    accessor: NodeRefAccessor::Value,
                },
            ),
        ]));
        b.bind_event(root, EventName::Input, handler);
        let island = b.build();
        let json = to_json("typed_dom_1234", "TypedDom", "TypedDom", &island, "");
        let descriptor =
            parse_descriptor_json(std::path::PathBuf::from("typed_dom_1234.json"), &json)
                .expect("descriptor parses")
                .expect("component descriptor");

        assert_eq!(descriptor.island, island);
    }

    #[test]
    fn descriptor_parser_preserves_v3_handler_block_as_source_of_truth() {
        let mut b = IslandBuilder::new();
        let signal = b.signal(Value::String(String::new()));
        let root = b.element("input", None);
        b.set_root(root);
        let handler = b.handler_from_ir(ClientBlock::from_stmts(vec![
            ClientStmt::let_value(
                ClientLocalId(0),
                "next",
                ClientType::Unknown,
                ClientExpr::EventRead(EventValue::TargetValue),
            ),
            ClientStmt::signal_set(signal, ClientExpr::local(ClientLocalId(0))),
        ]));
        b.bind_event(root, EventName::Input, handler);
        let island = b.build();
        let json = to_json("typed_v3_1234", "TypedV3", "TypedV3", &island, "");
        let mut parsed: serde_json::Value = serde_json::from_str(&json).expect("valid JSON");
        parsed["handlers"][0]["body_ir"]["stmts"][0]["ty"] =
            serde_json::json!({ "kind": "string" });
        let json = serde_json::to_string(&parsed).expect("valid JSON");

        let descriptor =
            parse_descriptor_json(std::path::PathBuf::from("typed_v3_1234.json"), &json)
                .expect("descriptor parses")
                .expect("component descriptor");

        let rsjs_ir::client::ClientStmt::Let { ty, .. } =
            &descriptor.island.handlers[0].body_ir().stmts[0]
        else {
            panic!("expected first body_ir statement to remain a let");
        };
        assert_eq!(
            ty,
            &rsjs_ir::client::ClientType::String,
            "descriptor parser should preserve the serialized v3 block instead of reconstructing it from HandlerBody"
        );
    }

    #[test]
    fn descriptor_json_round_trips_nested_control_flow_body_ir() {
        let mut b = IslandBuilder::new();
        let signal = b.signal(Value::String("idle".to_string()));
        let root = b.element("button", None);
        b.set_root(root);
        let handler = b.handler_from_ir(ClientBlock::from_stmts(vec![ClientStmt::if_else(
            ClientExpr::bool(true),
            ClientBlock::from_stmts(vec![ClientStmt::if_else(
                ClientExpr::bool(false),
                ClientBlock::from_stmts(vec![ClientStmt::signal_set(
                    signal,
                    ClientExpr::string("inner-then"),
                )]),
                ClientBlock::from_stmts(vec![ClientStmt::signal_set(
                    signal,
                    ClientExpr::string("inner-else"),
                )]),
            )]),
            ClientBlock::from_stmts(vec![ClientStmt::signal_set(
                signal,
                ClientExpr::string("outer-else"),
            )]),
        )]));
        b.bind_event(root, EventName::Click, handler);
        let island = b.build();
        let json = to_json(
            "control_flow_1234",
            "ControlFlow",
            "ControlFlow",
            &island,
            "",
        );
        let parsed: serde_json::Value = serde_json::from_str(&json).expect("valid JSON");
        assert_eq!(parsed["handlers"][0]["body_ir"]["kind"], "block");
        assert_eq!(parsed["handlers"][0]["body_ir"]["stmts"][0]["kind"], "if");
        assert_eq!(
            parsed["handlers"][0]["body_ir"]["stmts"][0]["then"]["stmts"][0]["kind"],
            "if"
        );
        assert_eq!(
            parsed["handlers"][0]["body_ir"]["stmts"][0]["then"]["stmts"][0]["else"]["stmts"][0]
                ["kind"],
            "signal_set"
        );

        let descriptor =
            parse_descriptor_json(std::path::PathBuf::from("control_flow_1234.json"), &json)
                .expect("descriptor parses")
                .expect("component descriptor");

        assert_eq!(descriptor.island, island);
    }

    #[test]
    fn descriptor_json_allows_sparse_handler_ids_after_dedup() {
        let mut b = IslandBuilder::new();
        let root = b.element("div", None);
        let first = b.element("button", Some(root));
        let second = b.element("button", Some(root));
        b.set_root(root);
        let h0 = b.handler_from_ir(ClientBlock::new());
        let _h1 = b.handler_from_ir(ClientBlock::new());
        let h2 = b.handler_from_ir(ClientBlock::new());
        b.bind_event(first, EventName::Click, h0);
        b.bind_event(second, EventName::KeyDown, h2);

        let mut island = b.build();
        island.handlers.remove(1);

        let json = to_json("sparse_1234", "Sparse", "Sparse", &island, "");
        let descriptor = parse_descriptor_json(std::path::PathBuf::from("sparse_1234.json"), &json)
            .expect("descriptor parses")
            .expect("component descriptor");

        assert_eq!(descriptor.island.handlers.len(), 2);
        assert_eq!(descriptor.island.handlers[0].id, EventHandlerId(0));
        assert_eq!(descriptor.island.handlers[1].id, EventHandlerId(2));
        assert_eq!(descriptor.island, island);
    }

    #[test]
    fn descriptor_json_allows_sparse_signal_ids_after_dead_signal_elimination() {
        let mut b = IslandBuilder::new();
        let _dead = b.signal(Value::Int(1));
        let live = b.signal(Value::Int(2));
        let mut island = b.build();
        island.signals.remove(0);

        let json = to_json(
            "sparse_signal_1234",
            "SparseSignal",
            "SparseSignal",
            &island,
            "",
        );
        let descriptor =
            parse_descriptor_json(std::path::PathBuf::from("sparse_signal_1234.json"), &json)
                .expect("descriptor parses")
                .expect("component descriptor");

        assert_eq!(descriptor.island.signals.len(), 1);
        assert_eq!(descriptor.island.signals[0].id, live);
        assert_eq!(descriptor.island, island);
    }

    #[test]
    fn descriptor_json_escapes_strings() {
        let mut b = IslandBuilder::new();
        let root = b.element("div", None);
        b.static_attr(root, "data-note", "\"quoted\"\nline");
        b.text_node_static(root, "</script>");
        b.set_root(root);

        let json = to_json("x", "Escapes", "Escapes", &b.build(), "");
        let parsed: serde_json::Value = serde_json::from_str(&json).expect("valid JSON");

        assert_eq!(
            parsed["view_tree"]["nodes"][0]["node"]["static_attrs"][0]["value"],
            "\"quoted\"\nline"
        );
        assert_eq!(parsed["view_tree"]["nodes"][1]["node"]["text"], "</script>");
    }

    #[test]
    fn descriptor_server_prop_fields_ignore_payload_attrs() {
        let mut b = IslandBuilder::new();
        let root = b.element("button", None);
        b.set_root(root);
        b.payload_attr_with_source(
            root,
            "data-rsjs-payload-p0",
            "self.secret",
            Some("self.secret".to_string()),
            None,
        );
        let handler = b.handler_from_ir(ClientBlock::from_stmts(vec![ClientStmt::Expr(
            ClientExpr::EventPayload {
                attr: "data-rsjs-payload-p0".to_string(),
            },
        )]));
        b.bind_event(root, EventName::Click, handler);

        let json = to_json("payload_1234", "Payload", "Payload", &b.build(), "");
        let parsed: serde_json::Value = serde_json::from_str(&json).expect("valid JSON");
        let fields = parsed["fields"]
            .as_array()
            .expect("fields should be an array");

        assert!(
            !fields
                .iter()
                .any(|field| field["kind"] == "server_prop"),
            "payload attrs are public crossing surfaces and must not be reported as server props: {json}"
        );
    }

    #[test]
    fn descriptor_server_prop_fields_include_loop_sources() {
        let mut b = IslandBuilder::new();
        let root = b.element("ul", None);
        b.set_root(root);
        let loop_node = b.for_loop(root, "", "", "item", "self.items.iter()", "");
        let button = b.element("button", Some(loop_node));
        b.payload_attr_with_source(
            button,
            "data-rsjs-payload-p0",
            "item.id",
            Some("item.id".to_string()),
            None,
        );

        let json = to_json("loop_1234", "Loop", "Loop", &b.build(), "");
        let parsed: serde_json::Value = serde_json::from_str(&json).expect("valid JSON");
        let fields = parsed["fields"]
            .as_array()
            .expect("fields should be an array");

        assert!(
            fields.iter().any(|field| field["kind"] == "server_prop"
                && field["name"] == "items"
                && field["source"] == "server_only_usage"),
            "loop source expressions should still be reported as server props: {json}"
        );
    }

    #[test]
    fn descriptor_infers_one_unconditional_self_field_as_immutable_static_slot() {
        let mut b = IslandBuilder::new();
        let root = b.element("section", None);
        b.set_root(root);
        b.server_only_node_with_source(
            root,
            "self.children",
            ServerSurfaceSource::InferredSimpleSsr,
        );

        let json = to_json(
            "disclosure_1234",
            "Disclosure",
            "Disclosure",
            &b.build(),
            "",
        );
        let parsed: serde_json::Value = serde_json::from_str(&json).expect("valid JSON");
        let fields = parsed["fields"]
            .as_array()
            .expect("fields should be an array");
        assert_eq!(
            fields,
            &[serde_json::json!({
                "id": 0,
                "name": "children",
                "kind": "static_slot",
                "source": "inferred_single_unconditional_server_slot",
                "span": "exact_one",
                "permits_nested_owner": false,
            })]
        );

        let summary =
            parse_descriptor_json(std::path::PathBuf::from("disclosure_1234.json"), &json)
                .expect("v8 descriptor parses")
                .expect("component descriptor")
                .summary;
        assert_eq!(summary.static_slot_fields, ["children"]);
        assert!(summary.server_prop_fields.is_empty());
    }

    #[test]
    fn descriptor_rejects_repeated_or_lifecycle_owned_self_fields_as_static_slots() {
        let mut repeated = IslandBuilder::new();
        let repeated_root = repeated.element("section", None);
        repeated.set_root(repeated_root);
        repeated.server_only_node_with_source(
            repeated_root,
            "self.children",
            ServerSurfaceSource::InferredSimpleSsr,
        );
        repeated.server_only_node_with_source(
            repeated_root,
            "self.children",
            ServerSurfaceSource::InferredSimpleSsr,
        );
        let repeated_json = to_json(
            "repeated_slot",
            "RepeatedSlot",
            "RepeatedSlot",
            &repeated.build(),
            "",
        );
        let repeated: serde_json::Value = serde_json::from_str(&repeated_json).expect("valid JSON");
        assert!(
            repeated["fields"]
                .as_array()
                .unwrap()
                .iter()
                .all(|field| field["kind"] != "static_slot"),
            "a static slot must render exactly once"
        );

        let mut conditional = IslandBuilder::new();
        let root = conditional.element("section", None);
        conditional.set_root(root);
        let branch = conditional.element("article", Some(root));
        let slot = conditional.server_only_node_with_source(
            branch,
            "self.children",
            ServerSurfaceSource::InferredSimpleSsr,
        );
        let condition = conditional.expr(ClientExpr::Literal(Value::Bool(true)));
        conditional.conditional_section(root, condition, vec![branch], Vec::new());
        let conditional_json = to_json(
            "conditional_slot",
            "ConditionalSlot",
            "ConditionalSlot",
            &conditional.build(),
            "",
        );
        let conditional: serde_json::Value =
            serde_json::from_str(&conditional_json).expect("valid JSON");
        assert!(
            conditional["fields"]
                .as_array()
                .unwrap()
                .iter()
                .all(|field| field["kind"] != "static_slot"),
            "a branch-owned slot can be reconstructed and must fail closed; slot node {slot:?}"
        );
    }

    #[test]
    fn summary_parser_validates_and_extracts_handler_fields() {
        let mut b = IslandBuilder::new();
        b.callback_slot("on_change");
        b.callback_slot("on_submit");
        let json = to_json(
            "calendar_1234",
            "Calendar",
            "ui :: Calendar",
            &b.build(),
            "",
        );
        let summary = parse_descriptor_json(std::path::PathBuf::from("calendar_1234.json"), &json)
            .expect("valid summary")
            .expect("component descriptor")
            .summary;

        assert_eq!(summary.asset_id, "calendar_1234");
        assert_eq!(summary.struct_name, "Calendar");
        assert_eq!(summary.handler_fields, ["on_change", "on_submit"]);
        assert!(descriptor_matches(&summary, "Calendar", "ui::Calendar"));
    }

    #[test]
    fn summary_parser_rejects_missing_client_ir_version() {
        let b = IslandBuilder::new();
        let json = to_json("legacy_meta", "LegacyMeta", "LegacyMeta", &b.build(), "");
        let mut parsed: serde_json::Value = serde_json::from_str(&json).expect("valid JSON");
        parsed
            .as_object_mut()
            .expect("descriptor JSON object")
            .remove("client_ir_version");
        let json = serde_json::to_string(&parsed).expect("valid JSON");

        let err = parse_descriptor_json(std::path::PathBuf::from("legacy_meta.json"), &json)
            .expect_err("missing Client IR version should be rejected");

        assert!(err.contains("missing numeric client_ir_version"));
    }

    #[test]
    fn summary_parser_rejects_stale_schema_version_with_rebuild_hint() {
        let b = IslandBuilder::new();
        let json = to_json("stale_meta", "StaleMeta", "StaleMeta", &b.build(), "");
        let mut parsed: serde_json::Value = serde_json::from_str(&json).expect("valid JSON");
        parsed["schema_version"] = serde_json::json!(2);
        let json = serde_json::to_string(&parsed).expect("valid JSON");

        let err = parse_descriptor_json(std::path::PathBuf::from("stale_meta.json"), &json)
            .expect_err("schema_version 2 descriptors should be rejected");

        assert!(
            err.contains("schema_version 2 is unsupported"),
            "got: {err}"
        );
        assert!(err.contains("legacy v1 expression IR"), "got: {err}");
        assert!(err.contains("Rebuild the crate"), "got: {err}");
    }

    #[test]
    fn summary_parser_describes_recent_stale_schema_without_claiming_legacy_ir() {
        let b = IslandBuilder::new();
        let json = to_json("stale_v8", "StaleV8", "StaleV8", &b.build(), "");
        let mut parsed: serde_json::Value = serde_json::from_str(&json).expect("valid JSON");
        parsed["schema_version"] = serde_json::json!(8);
        let json = serde_json::to_string(&parsed).expect("valid JSON");

        let err = parse_descriptor_json(std::path::PathBuf::from("stale_v8.json"), &json)
            .expect_err("schema_version 8 descriptors should be rejected");

        assert!(
            err.contains("schema_version 8 is unsupported"),
            "got: {err}"
        );
        assert!(
            err.contains("incompatible component field, temporal-input, or linker metadata"),
            "got: {err}"
        );
        assert!(!err.contains("legacy v1 expression IR"), "got: {err}");
    }

    #[test]
    fn summary_parser_rejects_future_schema_version_with_toolchain_hint() {
        let b = IslandBuilder::new();
        let json = to_json(
            "future_schema",
            "FutureSchema",
            "FutureSchema",
            &b.build(),
            "",
        );
        let mut parsed: serde_json::Value = serde_json::from_str(&json).expect("valid JSON");
        parsed["schema_version"] = serde_json::json!(10);
        let json = serde_json::to_string(&parsed).expect("valid JSON");

        let err = parse_descriptor_json(std::path::PathBuf::from("future_schema.json"), &json)
            .expect_err("schema_version 10 descriptors should be rejected");

        assert!(
            err.contains("schema_version 10 is unsupported"),
            "got: {err}"
        );
        assert!(err.contains("newer rsjs compiler"), "got: {err}");
    }

    #[test]
    fn summary_parser_rejects_stale_and_future_client_ir_versions() {
        for version in [CLIENT_IR_SCHEMA_VERSION - 1, CLIENT_IR_SCHEMA_VERSION + 1] {
            let b = IslandBuilder::new();
            let json = to_json(
                "mismatched_meta",
                "MismatchedMeta",
                "MismatchedMeta",
                &b.build(),
                "",
            );
            let mut parsed: serde_json::Value = serde_json::from_str(&json).expect("valid JSON");
            parsed["client_ir_version"] = serde_json::json!(version);
            let json = serde_json::to_string(&parsed).expect("valid JSON");

            let err =
                parse_descriptor_json(std::path::PathBuf::from("mismatched_meta.json"), &json)
                    .expect_err("a different Client IR major version should be rejected");

            assert!(
                err.contains(&format!("unsupported rsjs client IR version {version}")),
                "unexpected diagnostic: {err}"
            );
        }
    }

    #[test]
    fn summary_parser_skips_non_component_json() {
        let summary = parse_descriptor_json(
            std::path::PathBuf::from("other.json"),
            r#"{"format":"something_else"}"#,
        )
        .expect("valid non-component JSON");

        assert!(summary.is_none());
    }

    #[test]
    fn descriptor_lookup_collapses_stale_versions_of_same_type() {
        let mut matches = vec![
            descriptor_fixture("component_aaaa", "Widget", "Widget"),
            descriptor_fixture("component_zzzz", "Widget", "Widget"),
        ];

        collapse_stale_descriptor_versions(&mut matches);

        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].summary.asset_id, "component_zzzz");
    }

    fn descriptor_fixture(asset_id: &str, struct_name: &str, self_ty: &str) -> ComponentDescriptor {
        ComponentDescriptor {
            summary: ComponentDescriptorSummary {
                path: std::path::PathBuf::from(format!("{asset_id}.json")),
                asset_id: asset_id.to_string(),
                struct_name: struct_name.to_string(),
                self_ty: self_ty.to_string(),
                is_async: false,
                client_origin_permission: ComponentOriginPermission::OriginCapable,
                handler_fields: Vec::new(),
                prop_fields: Vec::new(),
                server_prop_fields: Vec::new(),
                static_slot_fields: Vec::new(),
                js_hash: prefixed_sha256_hex(asset_id.as_bytes()),
                descriptor_hash: prefixed_sha256_hex(self_ty.as_bytes()),
            },
            island: Island::new(),
            prop_fields: Vec::new(),
            inbound_inputs: ParsedInboundInputs::default(),
            query_initializers: Vec::new(),
            mutation_initializers: Vec::new(),
        }
    }

    fn assert_hash_shape(value: &serde_json::Value) {
        let hash = value.as_str().expect("hash field should be a string");
        let suffix = hash
            .strip_prefix("sha256:")
            .expect("hash should use a sha256: prefix");
        assert_eq!(suffix.len(), 64);
        assert!(suffix.chars().all(|ch| ch.is_ascii_hexdigit()));
    }
}
