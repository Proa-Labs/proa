// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use rsjs::__private::{SharedStateAbi, SharedStateAbiSchemaStatus};

#[derive(Debug, PartialEq, rsjs::SharedState)]
#[rsjs(type_key = "test.transport.closed")]
struct ClosedState {
    enabled: bool,
    labels: Option<Vec<String>>,
    total: i64,
}

#[derive(Debug, PartialEq, rsjs::ClientProps)]
struct NestedProps {
    label: String,
}

#[derive(Debug, PartialEq, rsjs::SharedState)]
#[rsjs(type_key = "test.transport.nested")]
struct NestedState {
    value: NestedProps,
}

#[test]
fn derive_exposes_the_exact_rust_resolved_definition_witness() {
    let witness = <ClosedState as SharedStateAbi>::WITNESS;
    assert_eq!(
        witness.format_magic,
        rsjs::__private::SHARED_STATE_ABI_WITNESS_MAGIC
    );
    assert_eq!(
        witness.witness_version,
        rsjs::__private::SHARED_STATE_ABI_WITNESS_VERSION
    );
    assert_eq!(
        witness.shared_state_abi_version,
        rsjs::SHARED_STATE_ABI_VERSION
    );
    assert_eq!(witness.schema_status, SharedStateAbiSchemaStatus::Resolved);
    assert_eq!(witness.type_key, "test.transport.closed");
    assert_ne!(witness.schema_abi_hash, [0; 32]);
    assert!(core::ptr::eq(
        witness,
        &__RSJS_SHARED_STATE_ABI_WITNESS_ClosedState
    ));
}

#[test]
fn nested_client_props_remain_runtime_compatible_but_link_fail_closed() {
    let witness = <NestedState as SharedStateAbi>::WITNESS;
    assert_eq!(
        witness.schema_status,
        SharedStateAbiSchemaStatus::UnresolvedNestedClientProps
    );
    assert_eq!(witness.type_key, "test.transport.nested");

    let value = NestedState {
        value: NestedProps {
            label: "still supported by the existing runtime codec".to_string(),
        },
    };
    let encoded = rsjs::ClientEncode::encode(&value);
    let decoded = <NestedState as rsjs::ClientDecode>::decode(&encoded).unwrap();
    assert_eq!(decoded, value);
}
