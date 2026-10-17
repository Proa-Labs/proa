// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Load exact linked plans and emit Cargo-tracked final expansion witnesses.
//! Byte validation and owner/helper selection live in rsjs_ir::final_plan.

use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use sha2::{Digest, Sha256};
use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};

pub(crate) use rsjs_ir::final_plan::*;

const FINAL_PHASE_ENV: &str = "RSJS_FINAL_PHASE";
const FINAL_PLAN_PATH_ENV: &str = "RSJS_LINK_PLAN_PATH";
const FINAL_PLAN_ID_ENV: &str = "RSJS_PLAN_ID";
const FINAL_PLAN_FILE_HASH_ENV: &str = "RSJS_LINK_PLAN_FILE_HASH";
const FINAL_UNIT_MAP_HASH_ENV: &str = "PROA_RSJS_FINAL_UNIT_MAP_HASH";
const PLAN_FILE_SUFFIX: &str = "semantic-plan.json";
const UNIT_PLAN_DIRECTORY_ENV: &str = "PROA_RSJS_FINAL_UNIT_PLAN_DIRECTORY";
const UNIT_PLAN_ENV_PREFIX: &str = "PROA_RSJS_FINAL_UNIT_PLAN_";

/// Validate and select the exact component and its concrete render sites.
///
/// Local checking and descriptor compilation return `None`. Final compilation
/// is fail-closed: all tracked plan inputs and the original descriptor-unit
/// identity must be present and valid.
pub(crate) fn validate_current_component(
    cargo_unit_id: [u8; 32],
    record_id: [u8; 32],
) -> Result<Option<FinalComponentSelection>, String> {
    let Some((plan_id, dependency_id, bytes)) =
        load_current_plan(DefinitionKind::Component, cargo_unit_id)?
    else {
        return Ok(None);
    };
    let mut selection = validate_component_plan_bytes(cargo_unit_id, record_id, plan_id, &bytes)?;
    selection.dependency_id = dependency_id;
    Ok(Some(selection))
}

/// Validate the exact helper definition without imposing component-site
/// requirements.
///
/// Helpers are definition-level records. A helper-only final unit must not
/// fail merely because it has no component occurrence table.
pub(crate) fn validate_current_helper(
    cargo_unit_id: [u8; 32],
    record_id: [u8; 32],
) -> Result<Option<FinalPlanSelection>, String> {
    let Some((plan_id, dependency_id, bytes)) =
        load_current_plan(DefinitionKind::Helper, cargo_unit_id)?
    else {
        return Ok(None);
    };
    let mut selection = validate_helper_plan_bytes(cargo_unit_id, record_id, plan_id, &bytes)?;
    selection.dependency_id = dependency_id;
    Ok(Some(selection))
}

/// Compatibility selector while component codegen is migrated to consume the
/// richer [`FinalComponentSelection`].
pub(crate) fn validate_current_definition(
    kind: DefinitionKind,
    cargo_unit_id: [u8; 32],
    record_id: [u8; 32],
) -> Result<Option<FinalPlanSelection>, String> {
    match kind {
        DefinitionKind::Component => {
            validate_current_component(cargo_unit_id, record_id).map(|selection| {
                selection.map(|selection| FinalPlanSelection {
                    plan_id: selection.plan_id,
                    dependency_id: selection.dependency_id,
                })
            })
        }
        DefinitionKind::Helper => validate_current_helper(cargo_unit_id, record_id),
    }
}

fn load_current_plan(
    kind: DefinitionKind,
    cargo_unit_id: [u8; 32],
) -> Result<Option<([u8; 32], [u8; 32], Vec<u8>)>, String> {
    match std::env::var(FINAL_PHASE_ENV) {
        Err(std::env::VarError::NotPresent) => return Ok(None),
        Err(std::env::VarError::NotUnicode(_)) => {
            return Err(format!(
                "`{FINAL_PHASE_ENV}` must be valid Unicode and equal to `1`"
            ));
        }
        Ok(value) if value == "1" => {}
        Ok(value) => {
            return Err(format!(
                "`{FINAL_PHASE_ENV}` must equal `1` during final RSJS compilation; received `{value}`"
            ));
        }
    }

    if cargo_unit_id == [0; 32] {
        return Err(format!(
            "final RSJS {} expansion has no descriptor Cargo-unit identity; \
             invoke the final compilation through `cargo proa build` so its \
             rustc wrapper can map this unit to the descriptor pass",
            kind.label()
        ));
    }

    let plan_id_text = required_env(FINAL_PLAN_ID_ENV)?;
    let plan_id = decode_lower_hex_id(FINAL_PLAN_ID_ENV, &plan_id_text)?;
    let plan_path = PathBuf::from(required_env(FINAL_PLAN_PATH_ENV)?);
    if !plan_path.is_absolute() {
        return Err(format!(
            "`{FINAL_PLAN_PATH_ENV}` must name an absolute content-addressed plan; received `{}`",
            plan_path.display()
        ));
    }
    let expected_name = format!("{plan_id_text}.{PLAN_FILE_SUFFIX}");
    if plan_path.file_name().and_then(|name| name.to_str()) != Some(expected_name.as_str()) {
        return Err(format!(
            "final RSJS plan `{}` must be named `{expected_name}`",
            plan_path.display()
        ));
    }

    let bytes = std::fs::read(&plan_path).map_err(|error| {
        format!(
            "failed to read final RSJS plan `{}`: {error}",
            plan_path.display()
        )
    })?;
    let file_hash = validate_file_hash(&bytes)?;
    let dependency_id =
        validate_current_unit_projection(cargo_unit_id, plan_id, file_hash, &bytes)?
            .unwrap_or(plan_id);
    Ok(Some((plan_id, dependency_id, bytes)))
}

fn validate_current_unit_projection(
    cargo_unit_id: [u8; 32],
    plan_id: [u8; 32],
    file_hash: [u8; 32],
    plan_bytes: &[u8],
) -> Result<Option<[u8; 32]>, String> {
    let Some(directory) = std::env::var_os(UNIT_PLAN_DIRECTORY_ENV) else {
        return Ok(None);
    };
    let directory = PathBuf::from(directory);
    if !directory.is_absolute() {
        return Err(format!(
            "`{UNIT_PLAN_DIRECTORY_ENV}` must name an absolute directory"
        ));
    }
    let name = format!("{UNIT_PLAN_ENV_PREFIX}{}", hex_id(&cargo_unit_id));
    let encoded = std::env::var(&name)
        .map_err(|_| format!("final RSJS unit projection input `{name}` is missing or invalid"))?;
    let hash = decode_lower_hex_id("final unit projection hash", &encoded)?;
    let path = directory.join(format!("{encoded}.{UNIT_PROJECTION_FILE_SUFFIX}"));
    let metadata = std::fs::symlink_metadata(&path).map_err(|error| {
        format!(
            "failed to inspect final RSJS unit projection `{}`: {error}",
            path.display()
        )
    })?;
    if !metadata.file_type().is_file() {
        return Err("final RSJS unit projection must be a plain file".to_owned());
    }
    let bytes = std::fs::read(&path).map_err(|error| {
        format!(
            "failed to read final RSJS unit projection `{}`: {error}",
            path.display()
        )
    })?;
    validate_unit_projection_bytes(cargo_unit_id, plan_id, file_hash, plan_bytes, hash, &bytes)?;
    Ok(Some(hash))
}

fn validate_unit_projection_bytes(
    cargo_unit_id: [u8; 32],
    plan_id: [u8; 32],
    file_hash: [u8; 32],
    plan_bytes: &[u8],
    expected_hash: [u8; 32],
    bytes: &[u8],
) -> Result<(), String> {
    let actual: [u8; 32] = Sha256::digest(bytes).into();
    if actual != expected_hash {
        return Err("final RSJS unit projection bytes do not match the tracked hash".to_owned());
    }
    let expected = expected_unit_projection(cargo_unit_id, plan_id, file_hash, plan_bytes)?;
    if expected.as_ref() != bytes {
        return Err("final RSJS unit projection does not match the complete consumed selections in the current plan".to_owned());
    }
    Ok(())
}

type ProjectionKey = ([u8; 32], [u8; 32], [u8; 32]);
type ProjectionCache = VecDeque<(ProjectionKey, Arc<[u8]>)>;
const PROJECTION_CACHE_ENTRIES: usize = 16;
const PROJECTION_CACHE_BYTES: usize = 8 * 1024 * 1024;

fn expected_unit_projection(
    cargo_unit_id: [u8; 32],
    plan_id: [u8; 32],
    file_hash: [u8; 32],
    plan_bytes: &[u8],
) -> Result<Arc<[u8]>, String> {
    // The caller freshly reads and validates the full plan and projection on
    // every expansion. Cache only this pure derivation, keyed by that verified
    // full-file digest as well as the plan and unit identities. Helper macros
    // in the same rustc process then need not derive every component again.
    static CACHE: OnceLock<Mutex<ProjectionCache>> = OnceLock::new();
    let key = (cargo_unit_id, plan_id, file_hash);
    let cache = CACHE.get_or_init(|| Mutex::new(VecDeque::new()));
    {
        let entries = cache
            .lock()
            .map_err(|_| "final RSJS projection cache lock was poisoned".to_owned())?;
        if let Some((_, bytes)) = entries.iter().find(|(entry, _)| entry == &key) {
            return Ok(Arc::clone(bytes));
        }
    }
    let bytes: Arc<[u8]> = unit_projection_bytes(cargo_unit_id, plan_id, plan_bytes)?.into();
    if bytes.len() <= PROJECTION_CACHE_BYTES {
        let mut entries = cache
            .lock()
            .map_err(|_| "final RSJS projection cache lock was poisoned".to_owned())?;
        if let Some((_, existing)) = entries.iter().find(|(entry, _)| entry == &key) {
            return Ok(Arc::clone(existing));
        }
        while entries.len() >= PROJECTION_CACHE_ENTRIES
            || entries.iter().map(|(_, value)| value.len()).sum::<usize>() + bytes.len()
                > PROJECTION_CACHE_BYTES
        {
            entries.pop_front();
        }
        entries.push_back((key, Arc::clone(&bytes)));
    }
    Ok(bytes)
}

/// Emit a retained build-ID witness into the final server artifact.
pub(crate) fn witness_tokens<S: PlanSelection>(label: &str, selection: Option<&S>) -> TokenStream {
    // `load_current_plan` must use `std::env::var` while the proc macro is
    // executing, which Cargo cannot observe. Emit built-in `env!` reads into
    // every final expansion so rustc records the wrapper-owned inputs in its
    // dep-info. Semantic runners track this unit's complete projection and
    // mapping entry. Legacy runners keep the global plan dependencies.
    // A plan change then invalidates only crates that expand RSJS
    // instead of requiring a cold target directory for the whole graph.
    let unit = std::env::var("RSJS_CARGO_UNIT_ID").ok();
    let projection_name = std::env::var_os(UNIT_PLAN_DIRECTORY_ENV)
        .and_then(|_| unit_projection_tracking_name(unit.as_deref()));
    let cargo_invalidation = cargo_invalidation_tokens(
        matches!(
            std::env::var(FINAL_PHASE_ENV),
            Ok(value) if value == "1"
        ),
        projection_name.as_deref(),
    );
    let Some(selection) = selection else {
        return cargo_invalidation;
    };
    let ident = format_ident!("__RSJS_FINAL_PLAN_WITNESS_{}", label);
    let bytes = selection
        .dependency_id()
        .into_iter()
        .map(proc_macro2::Literal::u8_unsuffixed)
        .collect::<Vec<_>>();
    quote! {
        #cargo_invalidation

        #[doc(hidden)]
        #[used]
        #[allow(non_upper_case_globals)]
        static #ident: [u8; 32] = [#(#bytes),*];
    }
}

fn cargo_invalidation_tokens(final_phase: bool, projection_name: Option<&str>) -> TokenStream {
    if !final_phase {
        return TokenStream::new();
    }
    let plan_id_env = proc_macro2::Literal::string(FINAL_PLAN_ID_ENV);
    let plan_file_hash_env = proc_macro2::Literal::string(FINAL_PLAN_FILE_HASH_ENV);
    let unit = std::env::var("RSJS_CARGO_UNIT_ID").ok();
    let tracking_name = unit_mapping_tracking_name(unit.as_deref());
    let unit_map_hash_env = proc_macro2::Literal::string(&tracking_name);
    if let Some(name) = projection_name {
        let unit_plan_hash_env = proc_macro2::Literal::string(name);
        return quote! {
            const _: (&str, &str) = (
                ::core::env!(#unit_plan_hash_env),
                ::core::env!(#unit_map_hash_env),
            );
        };
    }
    quote! {
        const _: (&str, &str, &str) = (
            ::core::env!(#plan_id_env),
            ::core::env!(#plan_file_hash_env),
            ::core::env!(#unit_map_hash_env),
        );
    }
}

fn unit_mapping_tracking_name(unit: Option<&str>) -> String {
    match unit {
        Some(unit)
            if unit.len() == 64
                && unit
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)) =>
        {
            format!("PROA_RSJS_FINAL_UNIT_INPUT_{unit}")
        }
        _ => FINAL_UNIT_MAP_HASH_ENV.to_owned(),
    }
}

fn unit_projection_tracking_name(unit: Option<&str>) -> Option<String> {
    let mapping = unit_mapping_tracking_name(unit);
    mapping
        .strip_prefix("PROA_RSJS_FINAL_UNIT_INPUT_")
        .map(|unit| format!("{UNIT_PLAN_ENV_PREFIX}{unit}"))
}

fn validate_file_hash(bytes: &[u8]) -> Result<[u8; 32], String> {
    let encoded = required_env(FINAL_PLAN_FILE_HASH_ENV)?;
    let Some(hex) = encoded.strip_prefix("sha256:") else {
        return Err(format!(
            "`{FINAL_PLAN_FILE_HASH_ENV}` must be `sha256:` followed by 64 lowercase hexadecimal characters"
        ));
    };
    let expected = decode_lower_hex_id(FINAL_PLAN_FILE_HASH_ENV, hex)?;
    let actual: [u8; 32] = Sha256::digest(bytes).into();
    if actual != expected {
        return Err(format!(
            "final RSJS plan bytes do not match `{FINAL_PLAN_FILE_HASH_ENV}`; expected sha256:{}, found sha256:{}",
            hex_id(&expected),
            hex_id(&actual)
        ));
    }
    Ok(actual)
}

fn required_env(name: &'static str) -> Result<String, String> {
    std::env::var(name).map_err(|error| match error {
        std::env::VarError::NotPresent => format!(
            "final RSJS compilation is missing `{name}`; invoke it through `cargo proa build`"
        ),
        std::env::VarError::NotUnicode(_) => {
            format!("final RSJS environment variable `{name}` is not valid Unicode")
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn final_expansions_expose_every_plan_input_to_cargo_dep_info() {
        assert!(cargo_invalidation_tokens(false, None).is_empty());

        let tokens = cargo_invalidation_tokens(true, None).to_string();
        for name in [
            FINAL_PLAN_ID_ENV,
            FINAL_PLAN_FILE_HASH_ENV,
            &unit_mapping_tracking_name(std::env::var("RSJS_CARGO_UNIT_ID").ok().as_deref()),
        ] {
            assert!(
                tokens.contains(name),
                "missing Cargo env dependency `{name}`"
            );
        }
        assert_eq!(tokens.matches("env !").count(), 3);
    }

    #[test]
    fn projected_expansions_track_only_the_unit_selection_and_mapping() {
        let name = unit_projection_tracking_name(Some(&"11".repeat(32))).unwrap();
        let tokens = cargo_invalidation_tokens(true, Some(&name)).to_string();
        assert!(tokens.contains(&name));
        assert!(!tokens.contains(FINAL_PLAN_ID_ENV));
        assert!(!tokens.contains(FINAL_PLAN_FILE_HASH_ENV));
        assert_eq!(tokens.matches("env !").count(), 2);
        assert!(cargo_invalidation_tokens(false, Some(&name)).is_empty());
    }

    #[test]
    fn unit_projection_rejects_tampering_and_wrong_unit_even_with_matching_file_hash() {
        let unit = [1; 32];
        let plan_id = [2; 32];
        // Even an empty unit projection validates the complete plan graph.
        let plan_bytes = serde_json::to_vec(&serde_json::json!({
            "format": "proa.rsjs.semantic_link_plan", "schema_version": 15,
            "plan_id": hex_id(&plan_id), "components": [], "helpers": [], "helper_codegen": [],
            "component_codegen": [], "component_sites": [], "standalone_roots": [],
            "owners": [], "candidate_to_owner": [],
            "component_inputs": { "live_props": [], "live_callbacks": [] },
            "shared_state_providers": [], "shared_state_requirements": [], "node_ref_aliases": [],
        }))
        .unwrap();
        let bytes = unit_projection_bytes(unit, plan_id, &plan_bytes).unwrap();
        let hash: [u8; 32] = Sha256::digest(&bytes).into();
        let file_hash: [u8; 32] = Sha256::digest(&plan_bytes).into();
        validate_unit_projection_bytes(unit, plan_id, file_hash, &plan_bytes, hash, &bytes)
            .unwrap();
        let mut tampered = bytes.clone();
        tampered.push(b' ');
        assert!(validate_unit_projection_bytes(
            unit,
            plan_id,
            file_hash,
            &plan_bytes,
            hash,
            &tampered
        )
        .unwrap_err()
        .contains("tracked hash"));
        assert!(validate_unit_projection_bytes(
            [3; 32],
            plan_id,
            file_hash,
            &plan_bytes,
            hash,
            &bytes
        )
        .unwrap_err()
        .contains("consumed selections"));
        // Matching the file's own digest never admits changed projection
        // bytes, including after the expected projection has been cached.
        let tampered_hash = Sha256::digest(&tampered).into();
        assert!(validate_unit_projection_bytes(
            unit,
            plan_id,
            file_hash,
            &plan_bytes,
            tampered_hash,
            &tampered,
        )
        .unwrap_err()
        .contains("consumed selections"));

        // A different full plan must be derived and validated even when its
        // advertised plan ID is unchanged. Failed derivations are not cached.
        let mut changed: serde_json::Value = serde_json::from_slice(&plan_bytes).unwrap();
        changed["components"] = serde_json::json!([{"invalid": true}]);
        let changed = serde_json::to_vec(&changed).unwrap();
        assert!(validate_unit_projection_bytes(
            unit,
            plan_id,
            Sha256::digest(&changed).into(),
            &changed,
            hash,
            &bytes,
        )
        .is_err());
        validate_unit_projection_bytes(unit, plan_id, file_hash, &plan_bytes, hash, &bytes)
            .unwrap();
    }
}

#[cfg(test)]
mod tracking_tests {
    use super::*;
    #[test]
    fn unit_mapping_names_are_exact_and_independent() {
        assert_eq!(
            unit_mapping_tracking_name(Some(&"11".repeat(32))),
            format!("PROA_RSJS_FINAL_UNIT_INPUT_{}", "11".repeat(32))
        );
        assert_ne!(
            unit_mapping_tracking_name(Some(&"11".repeat(32))),
            unit_mapping_tracking_name(Some(&"22".repeat(32)))
        );
        assert_eq!(
            unit_mapping_tracking_name(Some("invalid")),
            FINAL_UNIT_MAP_HASH_ENV
        );
    }
}
