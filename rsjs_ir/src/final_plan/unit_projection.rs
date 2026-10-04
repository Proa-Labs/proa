// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Cargo dependencies for the complete final selection of one descriptor unit.

use super::*;

pub const UNIT_PROJECTION_FILE_SUFFIX: &str = "final-unit-plan.json";

/// Canonical bytes consumed by every final RSJS expansion in this unit.
///
/// The global plan remains the validation authority. This projection excludes
/// its ID, unrelated definitions, and dense graph ordinals. Every occurrence
/// of every local definition is included, including multi-owner participants.
/// Callers publish these exact bytes under their SHA-256 and track that hash.
pub fn unit_projection_bytes(
    cargo_unit_id: [u8; 32],
    plan_id: [u8; 32],
    bytes: &[u8],
) -> Result<Vec<u8>, String> {
    unit_projection_bytes_for_units([cargo_unit_id], plan_id, bytes)?
        .remove(&cargo_unit_id)
        .ok_or_else(|| "requested unit projection was not generated".to_owned())
}

/// Prepare a unit set with one global JSON decode. Non-RSJS compiler units
/// receive an empty local selection without repeatedly parsing the full plan.
pub fn unit_projection_bytes_for_units(
    units: impl IntoIterator<Item = [u8; 32]>,
    plan_id: [u8; 32],
    bytes: &[u8],
) -> Result<BTreeMap<[u8; 32], Vec<u8>>, String> {
    let value = validate_plan_header(bytes, &hex_id(&plan_id))?;
    let root = object(&value, "final RSJS plan")?;
    let components = definition_json_records(root, "components")?;
    let helpers = definition_json_records(root, "helpers")?;
    let helper_codegen = definition_json_records(root, "helper_codegen")?;
    let mut projections = BTreeMap::new();
    for cargo_unit_id in units.into_iter().collect::<BTreeSet<_>>() {
        projections.insert(
            cargo_unit_id,
            project_unit_records(
                cargo_unit_id,
                plan_id,
                bytes,
                &components,
                &helpers,
                &helper_codegen,
            )?,
        );
    }
    Ok(projections)
}

fn project_unit_records(
    cargo_unit_id: [u8; 32],
    plan_id: [u8; 32],
    bytes: &[u8],
    components: &BTreeMap<DefinitionId, &Value>,
    helpers: &BTreeMap<DefinitionId, &Value>,
    helper_codegen: &BTreeMap<DefinitionId, &Value>,
) -> Result<Vec<u8>, String> {
    let mut component_projections = Vec::new();
    for definition in components
        .keys()
        .filter(|id| id.cargo_unit_id == cargo_unit_id)
    {
        let selection =
            validate_component_plan_bytes(cargo_unit_id, definition.record_id, plan_id, bytes)?;
        component_projections.push(serde_json::json!({
            "record": hex_id(&definition.record_id),
            "selection": component_projection(&selection)?,
        }));
    }
    let mut helper_projections = Vec::new();
    for (definition, descriptor) in helpers
        .iter()
        .filter(|(id, _)| id.cargo_unit_id == cargo_unit_id)
    {
        validate_helper_plan_bytes(cargo_unit_id, definition.record_id, plan_id, bytes)?;
        helper_projections.push(serde_json::json!({
            "descriptor": descriptor,
            "codegen": helper_codegen.get(definition).ok_or_else(||
                "validated helper has no exact codegen record".to_owned())?,
        }));
    }
    canonical_plan_json_bytes(&serde_json::json!({
        "format": "proa.rsjs.final_unit_plan",
        "schema_version": 1,
        "cargo_unit_id": hex_id(&cargo_unit_id),
        "components": component_projections,
        "helpers": helper_projections,
    }))
}

fn runtime_coordinate(
    selection: &FinalComponentSelection,
    candidate: u32,
) -> Result<Value, String> {
    let identity = selection.runtime_site(candidate)?;
    Ok(serde_json::json!({
        "scope": hex_id(&identity.scope_id),
        "site": hex_id(&identity.site_id),
        "candidate": identity.candidate_id,
    }))
}

/// Serialize the typed consumed records, replacing candidate references with
/// complete stable coordinates. Producer-local value/field/control IDs stay
/// unchanged. Owner ordinals are replaced separately at the site boundary.
fn occurrence_record<T: Serialize>(
    selection: &FinalComponentSelection,
    record: &T,
) -> Result<Value, String> {
    let mut value = serde_json::to_value(record)
        .map_err(|error| format!("failed to serialize final selection: {error}"))?;
    normalize_candidates(selection, &mut value)?;
    Ok(value)
}

fn normalize_candidates(
    selection: &FinalComponentSelection,
    value: &mut Value,
) -> Result<(), String> {
    match value {
        Value::Object(fields) => {
            for (name, value) in fields {
                if (name == "candidate_id" || name.ends_with("_candidate_id")) && !value.is_null() {
                    let candidate = value
                        .as_u64()
                        .and_then(|id| u32::try_from(id).ok())
                        .ok_or_else(|| format!("invalid internal candidate in {name}"))?;
                    *value = runtime_coordinate(selection, candidate)?;
                } else {
                    normalize_candidates(selection, value)?;
                }
            }
        }
        Value::Array(values) => {
            for value in values {
                normalize_candidates(selection, value)?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn sorted_occurrence_records<'a, T: Serialize + 'a>(
    selection: &FinalComponentSelection,
    records: impl Iterator<Item = &'a T>,
) -> Result<Vec<Value>, String> {
    let mut records = records
        .map(|record| {
            let value = occurrence_record(selection, record)?;
            Ok((canonical_plan_json_bytes(&value)?, value))
        })
        .collect::<Result<Vec<_>, String>>()?;
    records.sort_by(|left, right| left.0.cmp(&right.0));
    Ok(records.into_iter().map(|(_, value)| value).collect())
}

fn component_projection(selection: &FinalComponentSelection) -> Result<Value, String> {
    let mut sites = Vec::new();
    for site in &selection.sites {
        // Serialize the complete site record, then replace its owner ordinal.
        // An ordinal must never survive in a Cargo dependency or emitted ABI.
        let mut value = occurrence_record(selection, site)?;
        value["owner_role"] = match site.owner_role {
            FinalOwnerRole::ServerOnly => serde_json::json!({"kind": "server_only"}),
            FinalOwnerRole::Root { .. } => serde_json::json!({
                "kind": "root", "root": runtime_coordinate(selection, site.candidate_id)?,
            }),
            FinalOwnerRole::Member {
                root_candidate_id, ..
            } => serde_json::json!({
                "kind": "member", "root": runtime_coordinate(selection, root_candidate_id)?,
            }),
        };
        sites.push((canonical_plan_json_bytes(&value)?, value));
    }
    sites.sort_by(|left, right| left.0.cmp(&right.0));
    let mut children = selection
        .child_sites
        .iter()
        .map(|(&(parent, edge), &child)| {
            let value = serde_json::json!({
                "parent": runtime_coordinate(selection, parent)?,
                "edge": edge,
                "child": runtime_coordinate(selection, child)?,
            });
            Ok((canonical_plan_json_bytes(&value)?, value))
        })
        .collect::<Result<Vec<_>, String>>()?;
    children.sort_by(|left, right| left.0.cmp(&right.0));
    let mut owners = linked_owner_module_plans(selection)?
        .into_iter()
        .map(|plan| {
            let aliases = linked_owner_node_ref_aliases(selection, &plan)?;
            let js = crate::emit_linked_owner_module_with_node_refs(&plan, &aliases)
                .map_err(|error| format!("failed to project linked owner: {error}"))?;
            Ok((
                plan.owner_asset_id.clone(),
                serde_json::json!({
                    "asset": plan.owner_asset_id,
                    "js": js,
                    "uses_queries": plan.uses_queries,
                    "uses_mutations": plan.uses_mutations,
                }),
            ))
        })
        .collect::<Result<Vec<_>, String>>()?;
    owners.sort_by(|left, right| left.0.cmp(&right.0));
    Ok(serde_json::json!({
        // This generation commits all fragment codegen, selected export
        // unions, helper source bindings, bodies, signatures and protocol.
        "fragment": selection.fragment_asset_id,
        "standalone": selection.standalone_candidate_id.map(|id|
            runtime_coordinate(selection, id)).transpose()?,
        "sites": sites.into_iter().map(|(_, value)| value).collect::<Vec<_>>(),
        "children": children.into_iter().map(|(_, value)| value).collect::<Vec<_>>(),
        "live_props": sorted_occurrence_records(selection,
            selection.live_props_by_child.values().flatten())?,
        "live_callbacks": sorted_occurrence_records(selection,
            selection.live_callbacks_by_child.values().flatten())?,
        "providers": sorted_occurrence_records(selection,
            selection.shared_state_providers_by_host.values().flatten())?,
        "requirements": sorted_occurrence_records(selection,
            selection.shared_state_requirements_by_consumer.values().flatten())?,
        "node_ref_parents": sorted_occurrence_records(selection,
            selection.node_ref_aliases_by_parent.values().flatten())?,
        "node_ref_children": sorted_occurrence_records(selection,
            selection.node_ref_aliases_by_child.values().flatten())?,
        "owners": owners.into_iter().map(|(_, value)| value).collect::<Vec<_>>(),
    }))
}
