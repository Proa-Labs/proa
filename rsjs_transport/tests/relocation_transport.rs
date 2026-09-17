// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

#![cfg(any(
    target_vendor = "apple",
    target_os = "linux",
    target_os = "android",
    target_os = "freebsd",
    target_os = "netbsd",
    target_os = "openbsd",
    target_os = "dragonfly",
    target_os = "solaris",
    target_os = "illumos",
    target_os = "haiku",
    target_os = "redox",
    target_os = "windows"
))]

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use rsjs_transport::{
    decode_shared_state_schema_abi, extract_exact_artifacts, ComponentDefinitionAbi,
    ExtractionError, RelocationIdentity, SemanticIdentityKind, SharedStateSchemaAbiResolution,
    SharedStateSchemaCodec, TransportComponentOriginPermission, TransportComponentRenderKind,
};

#[test]
fn imported_alias_edges_resolve_to_the_defining_witness_across_an_rlib() {
    let fixture = tempfile::tempdir().expect("temporary fixture workspace");
    let repository = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("rsjs_transport lives under the workspace root");
    write_fixture(fixture.path(), repository);

    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let output = Command::new(cargo)
        .args(["build", "--workspace", "--offline", "--message-format=json"])
        .current_dir(fixture.path())
        .env("CARGO_TARGET_DIR", fixture.path().join("target"))
        .output()
        .expect("run fixture Cargo build");
    assert!(
        output.status.success(),
        "fixture build failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let artifacts = cargo_rlibs(&output.stdout);
    let component_rlib = artifacts
        .get("component_lib")
        .expect("component library rlib");
    let helper_rlib = artifacts.get("helper_lib").expect("helper library rlib");
    let consumer_rlib = artifacts.get("consumer").expect("consumer rlib");
    let duplicate_helper_rlib = artifacts
        .get("duplicate_helper")
        .expect("duplicate helper rlib");
    let missing_definition = extract_exact_artifacts([consumer_rlib]).unwrap_err();
    assert!(
        matches!(
            missing_definition,
            ExtractionError::UnresolvedWitness { .. }
        ),
        "the extractor must not discover an omitted neighboring rlib: {missing_definition}"
    );
    let missing_helper = extract_exact_artifacts([consumer_rlib, component_rlib]).unwrap_err();
    assert!(
        matches!(missing_helper, ExtractionError::UnresolvedHelper { .. }),
        "supplying the component rlib must not make an omitted helper rlib discoverable: \
         {missing_helper}"
    );
    let extracted = extract_exact_artifacts([consumer_rlib, component_rlib, helper_rlib])
        .expect("extract exact rlibs");

    assert_eq!(extracted.definitions.len(), 1);
    assert_eq!(
        extracted.definitions[0].component_abi,
        ComponentDefinitionAbi::new(
            TransportComponentRenderKind::Sync,
            TransportComponentOriginPermission::OriginCapable,
        ),
        "the extracted definition must preserve the producer witness projection"
    );
    assert_eq!(extracted.edges.len(), 2);
    assert_eq!(extracted.resolved_edges.len(), 2);
    assert_eq!(extracted.helper_definitions.len(), 1);
    assert_eq!(extracted.helper_edges.len(), 2);
    assert_eq!(extracted.resolved_helper_edges.len(), 2);
    assert_eq!(extracted.resolved_definitions.len(), 1);
    assert_eq!(extracted.resolved_helper_definitions.len(), 1);
    assert_eq!(extracted.semantic_identities.len(), 2);
    assert_eq!(extracted.shared_state_definitions.len(), 1);
    assert_eq!(extracted.resolved_shared_state_definitions.len(), 1);
    assert_eq!(extracted.shared_state_acquisitions.len(), 1);
    assert_eq!(extracted.resolved_shared_state_acquisitions.len(), 1);
    let shared_definition = &extracted.resolved_shared_state_definitions[0];
    assert_eq!(shared_definition.payload.type_key, "fixture.workbench");
    let shared_schema =
        decode_shared_state_schema_abi(&shared_definition.payload.schema_abi).unwrap();
    assert_eq!(
        shared_schema.resolution,
        SharedStateSchemaAbiResolution::Resolved
    );
    assert_eq!(
        shared_schema
            .fields
            .iter()
            .map(|field| (&*field.name, &field.codec))
            .collect::<Vec<_>>(),
        vec![
            ("enabled", &SharedStateSchemaCodec::Bool),
            (
                "labels",
                &SharedStateSchemaCodec::Option(Box::new(SharedStateSchemaCodec::Vec(Box::new(
                    SharedStateSchemaCodec::String
                ))))
            ),
        ]
    );
    let expected_format = if cfg!(target_vendor = "apple") {
        object::BinaryFormat::MachO
    } else if cfg!(target_os = "windows") {
        object::BinaryFormat::Coff
    } else {
        object::BinaryFormat::Elf
    };
    assert!(
        extracted
            .artifacts
            .iter()
            .flat_map(|artifact| artifact.object_formats.iter())
            .all(|format| *format == expected_format),
        "host fixture should exercise {expected_format:?} rlib members"
    );

    let defining_witness = &extracted.definitions[0].witness;
    assert!(matches!(
        defining_witness,
        RelocationIdentity::Symbol(symbol)
            if symbol.contains("__RSJS_COMPONENT_ABI_WITNESS_statusbadge_")
    ));
    for edge in &extracted.resolved_edges {
        assert_eq!(&edge.edge.witness, defining_witness);
        assert_eq!(
            edge.definition.component_record_id,
            extracted.definitions[0].component_record_id
        );
    }
    let component_payload = &extracted.resolved_definitions[0];
    assert_eq!(
        component_payload.definition.component_record_id,
        component_payload.payload.component_record_id
    );
    assert_eq!(
        component_payload.definition.cargo_unit_id,
        component_payload.payload.cargo_unit_id
    );
    assert_eq!(
        component_payload.identity.kind,
        SemanticIdentityKind::Component
    );
    assert_eq!(component_payload.identity.package, "component_lib");
    assert_eq!(component_payload.identity.crate_name, "component_lib");
    assert_eq!(component_payload.identity.module_path, ["ui"]);
    assert_eq!(component_payload.identity.item, "StatusBadge");
    let component_json: serde_json::Value =
        serde_json::from_slice(&component_payload.payload.json).unwrap();
    assert_eq!(component_json["format"], "rsjs.component_transport_payload");
    assert_eq!(component_json["schema_version"], 9);
    assert_eq!(
        component_json["component_descriptor"]["format"],
        "rsjs.component_descriptor"
    );
    assert_eq!(
        component_json["component_descriptor"]["struct_name"],
        "StatusBadge"
    );
    assert_eq!(component_json["component_descriptor"]["schema_version"], 9);
    assert_eq!(
        component_json["transport"]["definition_record_id"],
        hex_id(&component_payload.definition.component_record_id)
    );
    assert!(component_json["transport"]["component_input_fingerprint"]
        .as_str()
        .is_some_and(|fingerprint| fingerprint.starts_with("sha256:") && fingerprint.len() == 71));
    assert_eq!(
        component_json["transport"]["component_field_witnesses"],
        serde_json::json!([])
    );
    assert_eq!(
        component_json["transport"]["client_value_sources"]["payload_handlers"],
        serde_json::json!([])
    );
    assert_eq!(
        component_json["transport"]["direct_component_edges"],
        serde_json::json!([])
    );
    assert_eq!(
        component_json["transport"]["reachable_helper_edges"],
        serde_json::json!([])
    );
    let shared_claims = component_json["transport"]["shared_state_acquisitions"]
        .as_array()
        .expect("SharedState acquisition claims");
    assert_eq!(shared_claims.len(), 1);
    assert_eq!(shared_claims[0]["store_id"], 0);
    assert_eq!(
        shared_claims[0]["source_edge_id"],
        hex_id(&extracted.shared_state_acquisitions[0].source_edge_id)
    );
    assert_eq!(
        extracted.resolved_shared_state_acquisitions[0]
            .definition
            .shared_state_record_id,
        shared_definition.definition.shared_state_record_id,
        "the imported state alias must relocate to its defining derive witness"
    );
    assert_eq!(
        component_json["component_descriptor"]["js_hash"],
        "sha256:e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
        "an unused acquisition must emit transport without activating client JS"
    );
    assert_eq!(
        component_json["transport"]["hydration_constraint"],
        serde_json::json!({ "kind": "unconstrained" })
    );

    let source_ids = extracted
        .resolved_edges
        .iter()
        .map(|edge| edge.edge.source_edge_id)
        .collect::<Vec<_>>();
    assert!(source_ids.contains(&[0x11; 32]));
    assert!(source_ids.contains(&[0x22; 32]));

    let local = extracted
        .resolved_edges
        .iter()
        .find(|edge| edge.edge.source_edge_id == [0x11; 32])
        .expect("same-crate alias edge");
    let cross_rlib = extracted
        .resolved_edges
        .iter()
        .find(|edge| edge.edge.source_edge_id == [0x22; 32])
        .expect("cross-rlib imported alias edge");
    assert_eq!(
        local.edge.location.artifact_path, *component_rlib,
        "same-crate alias should retain its producer artifact"
    );
    assert_eq!(
        cross_rlib.edge.location.artifact_path, *consumer_rlib,
        "imported alias should remain an edge in the consumer rlib"
    );
    assert_eq!(
        local.definition.location, cross_rlib.definition.location,
        "both Rust-resolved aliases must reach the same defining record"
    );

    let defining_helper = &extracted.helper_definitions[0].helper;
    assert!(matches!(
        defining_helper,
        RelocationIdentity::Symbol(symbol) if symbol.contains("pluralize")
    ));
    for edge in &extracted.resolved_helper_edges {
        assert_eq!(&edge.edge.helper, defining_helper);
        assert_eq!(
            edge.definition.helper_record_id,
            extracted.helper_definitions[0].helper_record_id
        );
    }
    let helper_payload = &extracted.resolved_helper_definitions[0];
    assert_eq!(
        helper_payload.definition.helper_record_id,
        helper_payload.payload.helper_record_id
    );
    assert_eq!(
        helper_payload.definition.cargo_unit_id,
        helper_payload.payload.cargo_unit_id
    );
    assert_eq!(helper_payload.identity.kind, SemanticIdentityKind::Helper);
    assert_eq!(helper_payload.identity.package, "helper_lib");
    assert_eq!(helper_payload.identity.crate_name, "helper_lib");
    assert_eq!(helper_payload.identity.module_path, ["labels"]);
    assert_eq!(helper_payload.identity.item, "pluralize");
    let helper_json: serde_json::Value =
        serde_json::from_slice(&helper_payload.payload.json).unwrap();
    assert_eq!(helper_json["format"], "rsjs.standalone_helper_descriptor");
    assert_eq!(helper_json["schema_version"], 1);
    assert_eq!(helper_json["effect_class"], "origin_free");
    assert_eq!(helper_json["signature"]["params"][0]["name"], "count");
    assert_eq!(helper_json["signature"]["params"][0]["rust_type"], "u32");
    assert_eq!(helper_json["signature"]["return_type"], "u32");
    assert_eq!(
        helper_json["body_ir_version"],
        rsjs_ir::abi::CLIENT_IR_SCHEMA_VERSION
    );
    assert!(helper_json["body_ir"].is_object());
    assert!(helper_json["semantic_hash"]
        .as_str()
        .is_some_and(|hash| hash.starts_with("sha256:") && hash.len() == 71));
    let helper_source_ids = extracted
        .resolved_helper_edges
        .iter()
        .map(|edge| edge.edge.source_edge_id)
        .collect::<Vec<_>>();
    assert!(helper_source_ids.contains(&[0x33; 32]));
    assert!(helper_source_ids.contains(&[0x44; 32]));
    let local_helper = extracted
        .resolved_helper_edges
        .iter()
        .find(|edge| edge.edge.source_edge_id == [0x33; 32])
        .expect("same-crate helper alias edge");
    let imported_helper = extracted
        .resolved_helper_edges
        .iter()
        .find(|edge| edge.edge.source_edge_id == [0x44; 32])
        .expect("cross-rlib imported helper alias edge");
    assert_eq!(local_helper.edge.location.artifact_path, *helper_rlib);
    assert_eq!(imported_helper.edge.location.artifact_path, *consumer_rlib);
    assert_eq!(
        local_helper.definition.location, imported_helper.definition.location,
        "same-crate and imported/re-exported helper aliases must reach one definition"
    );
    assert_eq!(
        helper_payload.identity.item, "pluralize",
        "the imported/re-exported spelling must not alter defining helper identity"
    );

    let duplicate = extract_exact_artifacts([helper_rlib, duplicate_helper_rlib]).unwrap_err();
    assert!(
        matches!(duplicate, ExtractionError::AmbiguousHelper { .. }),
        "multiple helper definitions for one Rust function must fail closed: {duplicate}"
    );
}

fn cargo_rlibs(stdout: &[u8]) -> BTreeMap<String, PathBuf> {
    let mut artifacts = BTreeMap::new();
    for line in String::from_utf8_lossy(stdout).lines() {
        let Ok(message) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        if message["reason"] != "compiler-artifact" {
            continue;
        }
        let Some(target_name) = message["target"]["name"].as_str() else {
            continue;
        };
        let Some(filenames) = message["filenames"].as_array() else {
            continue;
        };
        if let Some(filename) = filenames
            .iter()
            .filter_map(serde_json::Value::as_str)
            .find(|filename| filename.ends_with(".rlib"))
        {
            artifacts.insert(target_name.to_string(), PathBuf::from(filename));
        }
    }
    artifacts
}

fn hex_id(id: &[u8; 32]) -> String {
    use std::fmt::Write as _;

    let mut out = String::with_capacity(64);
    for byte in id {
        let _ = write!(out, "{byte:02x}");
    }
    out
}

fn write_fixture(root: &Path, repository: &Path) {
    let component = root.join("component_lib");
    let helper = root.join("helper_lib");
    let consumer = root.join("consumer");
    let duplicate_helper = root.join("duplicate_helper");
    fs::create_dir_all(component.join("src")).unwrap();
    fs::create_dir_all(helper.join("src")).unwrap();
    fs::create_dir_all(consumer.join("src")).unwrap();
    fs::create_dir_all(duplicate_helper.join("src")).unwrap();

    fs::write(
        root.join("Cargo.toml"),
        r#"
[workspace]
members = ["component_lib", "helper_lib", "consumer", "duplicate_helper"]
resolver = "2"

[profile.dev]
codegen-units = 2
lto = false
"#,
    )
    .unwrap();

    fs::write(
        component.join("Cargo.toml"),
        format!(
            r#"
[package]
name = "component_lib"
version = "0.1.0"
edition = "2021"

[dependencies]
proa_core = {{ path = "{root}/proa_core" }}
proa_macros = {{ path = "{root}/proa_macros" }}
rsjs = {{ path = "{root}/rsjs" }}
"#,
            root = repository.display(),
        ),
    )
    .unwrap();
    fs::write(
        component.join("src/lib.rs"),
        r#"
pub mod ui {
    use crate::WorkbenchState as StateAlias;
    use proa_core::{
        DataLoader, WebContext, WebRenderSync, WriteBuf, WriteError,
    };

    pub struct StatusBadge;

    #[rsjs::rsjs(component, client)]
    impl<L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for StatusBadge {
        fn render(
            self,
            cx: &mut WebContext<L, B>,
        ) -> Result<(), WriteError> {
            let _workbench = rsjs::shared::<StateAlias>();
            proa_macros::html_sync! {
                <span data-status-badge>"ready"</span>
            }
            .render(cx)
        }
    }
}

pub use ui::StatusBadge as Badge;

#[derive(rsjs::SharedState)]
#[rsjs(type_key = "fixture.workbench")]
pub struct WorkbenchState {
    pub enabled: bool,
    pub labels: Option<Vec<String>>,
}

rsjs::__private::__rsjs_emit_component_edge!(
    LOCAL_ALIAS_EDGE,
    [0x11; 32],
    Badge
);
"#,
    )
    .unwrap();

    fs::write(
        helper.join("Cargo.toml"),
        format!(
            r#"
[package]
name = "helper_lib"
version = "0.1.0"
edition = "2021"

[dependencies]
rsjs = {{ path = "{root}/rsjs" }}
"#,
            root = repository.display(),
        ),
    )
    .unwrap();
    fs::write(
        helper.join("src/lib.rs"),
        r#"
pub mod labels {
    #[rsjs::client]
    pub fn pluralize(count: u32) -> u32 {
        count
    }
}

pub use labels::pluralize as label_for_count;

rsjs::__private::__rsjs_emit_client_helper_edge!(
    LOCAL_HELPER_ALIAS_EDGE,
    [0x33; 32],
    label_for_count
);
"#,
    )
    .unwrap();

    fs::write(
        consumer.join("Cargo.toml"),
        format!(
            r#"
[package]
name = "consumer"
version = "0.1.0"
edition = "2021"

[dependencies]
component_lib = {{ path = "../component_lib" }}
helper_lib = {{ path = "../helper_lib" }}
rsjs = {{ path = "{root}/rsjs" }}
"#,
            root = repository.display(),
        ),
    )
    .unwrap();
    fs::write(
        consumer.join("src/lib.rs"),
        r#"
use component_lib::Badge as ImportedBadge;
use helper_lib::label_for_count as ImportedLabel;

rsjs::__private::__rsjs_emit_component_edge!(
    IMPORTED_ALIAS_EDGE,
    [0x22; 32],
    ImportedBadge
);

rsjs::__private::__rsjs_emit_client_helper_edge!(
    IMPORTED_HELPER_ALIAS_EDGE,
    [0x44; 32],
    ImportedLabel
);
"#,
    )
    .unwrap();

    fs::write(
        duplicate_helper.join("Cargo.toml"),
        format!(
            r#"
[package]
name = "duplicate_helper"
version = "0.1.0"
edition = "2021"

[dependencies]
helper_lib = {{ path = "../helper_lib" }}
rsjs = {{ path = "{root}/rsjs" }}
"#,
            root = repository.display(),
        ),
    )
    .unwrap();
    fs::write(
        duplicate_helper.join("src/lib.rs"),
        r#"
use helper_lib::label_for_count as DuplicatedHelper;

#[used]
#[cfg_attr(target_vendor = "apple", unsafe(link_section = "__DATA,__rsjs_help"))]
#[cfg_attr(
    any(
        target_os = "linux",
        target_os = "android",
        target_os = "freebsd",
        target_os = "netbsd",
        target_os = "openbsd",
        target_os = "dragonfly",
        target_os = "solaris",
        target_os = "illumos",
        target_os = "haiku",
        target_os = "redox"
    ),
    unsafe(link_section = ".rsjs.helper")
)]
#[cfg_attr(target_os = "windows", unsafe(link_section = ".rsjs$H"))]
static DUPLICATE_HELPER_DEFINITION: rsjs::__private::HelperDefinitionRecord =
    rsjs::__private::HelperDefinitionRecord::new(
        rsjs::__private::UNASSIGNED_CARGO_UNIT_ID,
        [0xdd; 32],
        DuplicatedHelper as *const (),
    );
"#,
    )
    .unwrap();
}
