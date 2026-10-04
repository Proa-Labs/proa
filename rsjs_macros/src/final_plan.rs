// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Load exact linked plans and emit Cargo-tracked final expansion witnesses.
//! Byte validation and owner/helper selection live in rsjs_ir::final_plan.

use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use sha2::{Digest, Sha256};
use std::path::PathBuf;

pub(crate) use rsjs_ir::final_plan::*;

const FINAL_PHASE_ENV: &str = "RSJS_FINAL_PHASE";
const FINAL_PLAN_PATH_ENV: &str = "RSJS_LINK_PLAN_PATH";
const FINAL_PLAN_ID_ENV: &str = "RSJS_PLAN_ID";
const FINAL_PLAN_FILE_HASH_ENV: &str = "RSJS_LINK_PLAN_FILE_HASH";
const FINAL_UNIT_MAP_HASH_ENV: &str = "PROA_RSJS_FINAL_UNIT_MAP_HASH";
const PLAN_FILE_SUFFIX: &str = "semantic-plan.json";

/// Validate and select the exact component and its concrete render sites.
///
/// Local checking and descriptor compilation return `None`. Final compilation
/// is fail-closed: all tracked plan inputs and the original descriptor-unit
/// identity must be present and valid.
pub(crate) fn validate_current_component(
    cargo_unit_id: [u8; 32],
    record_id: [u8; 32],
) -> Result<Option<FinalComponentSelection>, String> {
    let Some((plan_id, bytes)) = load_current_plan(DefinitionKind::Component, cargo_unit_id)?
    else {
        return Ok(None);
    };
    validate_component_plan_bytes(cargo_unit_id, record_id, plan_id, &bytes).map(Some)
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
    let Some((plan_id, bytes)) = load_current_plan(DefinitionKind::Helper, cargo_unit_id)? else {
        return Ok(None);
    };
    validate_helper_plan_bytes(cargo_unit_id, record_id, plan_id, &bytes).map(Some)
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
                })
            })
        }
        DefinitionKind::Helper => validate_current_helper(cargo_unit_id, record_id),
    }
}

fn load_current_plan(
    kind: DefinitionKind,
    cargo_unit_id: [u8; 32],
) -> Result<Option<([u8; 32], Vec<u8>)>, String> {
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
    validate_file_hash(&bytes)?;
    validate_plan_header(&bytes, &plan_id_text)?;
    Ok(Some((plan_id, bytes)))
}

/// Emit a retained build-ID witness into the final server artifact.
pub(crate) fn witness_tokens<S: PlanSelection>(label: &str, selection: Option<&S>) -> TokenStream {
    // `load_current_plan` must use `std::env::var` while the proc macro is
    // executing, which Cargo cannot observe. Emit built-in `env!` reads into
    // every final expansion so rustc records the wrapper-owned inputs in its
    // dep-info. The map witness tracks only this unit; exact global plan
    // inputs remain tracked because owner IDs currently include the PlanId.
    // A plan change then invalidates only crates that expand RSJS
    // instead of requiring a cold target directory for the whole graph.
    let cargo_invalidation = cargo_invalidation_tokens(matches!(
        std::env::var(FINAL_PHASE_ENV),
        Ok(value) if value == "1"
    ));
    let Some(selection) = selection else {
        return cargo_invalidation;
    };
    let ident = format_ident!("__RSJS_FINAL_PLAN_WITNESS_{}", label);
    let bytes = selection
        .plan_id()
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

fn cargo_invalidation_tokens(final_phase: bool) -> TokenStream {
    if !final_phase {
        return TokenStream::new();
    }
    let plan_id_env = proc_macro2::Literal::string(FINAL_PLAN_ID_ENV);
    let plan_file_hash_env = proc_macro2::Literal::string(FINAL_PLAN_FILE_HASH_ENV);
    let unit = std::env::var("RSJS_CARGO_UNIT_ID").ok();
    let tracking_name = unit_mapping_tracking_name(unit.as_deref());
    let unit_map_hash_env = proc_macro2::Literal::string(&tracking_name);
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

fn validate_file_hash(bytes: &[u8]) -> Result<(), String> {
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
    Ok(())
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
        assert!(cargo_invalidation_tokens(false).is_empty());

        let tokens = cargo_invalidation_tokens(true).to_string();
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
