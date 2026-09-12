// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Cross-check between the declarative browser-helper registry in
//! `rsjs_ir::browser_registry` and the hand-written SSR stubs in
//! `rsjs/src/lib.rs`.
//!
//! What this catches:
//!
//! - A registry entry added without an SSR stub: the analyzer would
//!   accept `rsjs::<module>::<method>()` and emit working JS, but the
//!   island would fail to compile server-side because the Rust path
//!   does not exist.
//! - A stub function added to a migrated module without a registry
//!   entry: the helper would compile as plain Rust, then the analyzer
//!   would reject it (or silently treat it as server-only), shipping
//!   a path that looks supported but is not.
//!
//! The stub side is recovered from the `rsjs` source text rather than
//! reflection because the stubs are deliberately plain `pub fn`
//! items. The parse leans on the rustfmt-stable layout of
//! `src/lib.rs`: top-level `pub mod <name> {` blocks closed by a
//! brace in column zero, with stub functions declared at four-space
//! indent. Helper methods on types (`&self`) are ignored.

use std::collections::{BTreeMap, BTreeSet};

use rsjs_ir::browser_registry::REGISTRY;

/// Extract `module -> { fn names }` for top-level `pub mod` blocks in
/// `rsjs/src/lib.rs`.
fn stub_modules() -> BTreeMap<String, BTreeSet<String>> {
    let source = include_str!("../src/lib.rs");
    let mut modules: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut current: Option<String> = None;
    for line in source.lines() {
        if let Some(rest) = line.strip_prefix("pub mod ") {
            if let Some(name) = rest.strip_suffix(" {") {
                current = Some(name.to_owned());
                modules.entry(name.to_owned()).or_default();
                continue;
            }
        }
        if line == "}" {
            current = None;
            continue;
        }
        let Some(module) = current.as_deref() else {
            continue;
        };
        // Stub functions sit at exactly four-space indent; methods on
        // nested types are deeper or take `self`.
        let Some(decl) = line.strip_prefix("    pub fn ") else {
            continue;
        };
        if decl.contains("self") {
            continue;
        }
        let name: String = decl
            .chars()
            .take_while(|c| c.is_alphanumeric() || *c == '_')
            .collect();
        if !name.is_empty() {
            modules.entry(module.to_owned()).or_default().insert(name);
        }
    }
    modules
}

#[test]
fn every_registry_entry_has_an_ssr_stub() {
    let modules = stub_modules();
    for spec in REGISTRY {
        let stubs = modules.get(spec.module).unwrap_or_else(|| {
            panic!(
                "registry entry {} expects a `pub mod {}` stub module in rsjs/src/lib.rs",
                spec.id, spec.module
            )
        });
        assert!(
            stubs.contains(spec.method),
            "registry entry {} has no SSR stub `rsjs::{}::{}`; add one to rsjs/src/lib.rs",
            spec.id,
            spec.module,
            spec.method
        );
    }
}

#[test]
fn every_stub_in_a_migrated_module_has_a_registry_entry() {
    let modules = stub_modules();
    // A module is "migrated" once any of its helpers is in the
    // registry; from then on the registry is its single source of
    // truth and stray stubs are a bug.
    let migrated: BTreeSet<&str> = REGISTRY.iter().map(|spec| spec.module).collect();
    let registered: BTreeSet<(&str, &str)> = REGISTRY
        .iter()
        .map(|spec| (spec.module, spec.method))
        .collect();
    for module in &migrated {
        let stubs = modules
            .get(*module)
            .unwrap_or_else(|| panic!("missing stub module `pub mod {module}`"));
        assert!(
            !stubs.is_empty(),
            "stub parse found no functions in migrated module {module}; \
             the layout assumptions in this test need updating"
        );
        for name in stubs {
            assert!(
                registered.contains(&(module, name.as_str())),
                "stub rsjs::{module}::{name} is not in rsjs_ir::browser_registry::REGISTRY; \
                 migrated families must declare every helper in the registry"
            );
        }
    }
}

#[test]
fn stub_parse_sees_known_modules() {
    // Guard the source-text parse itself: if the layout of
    // rsjs/src/lib.rs changes enough to break extraction, fail loudly
    // here instead of letting the cross-checks pass vacuously.
    let modules = stub_modules();
    for (module, method) in [
        ("window", "inner_width"),
        ("local_storage", "get_item"),
        ("session_storage", "set_item"),
        ("clipboard", "write_text"),
        ("navigator", "language"),
    ] {
        assert!(
            modules
                .get(module)
                .is_some_and(|stubs| stubs.contains(method)),
            "expected stub rsjs::{module}::{method} to be visible to the parse"
        );
    }
}
