// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Hidden Rust witness for `#[derive(rsjs::SharedState)]` definitions.
//!
//! SharedState protocol identity is the explicit `type_key` plus canonical
//! schema ABI bytes. The witness in this module is deliberately narrower: it
//! gives rustc one nominal, relocation-bearing target for the defining Rust
//! type so acquisition edges can resolve imports, aliases, and re-exports
//! without reconstructing identity from source path tokens.

/// Magic bytes at the front of every target-side SharedState ABI witness.
#[doc(hidden)]
pub const SHARED_STATE_ABI_WITNESS_MAGIC: [u8; 8] = *b"RSJSSABI";

/// Version of [`SharedStateAbiWitness`].
#[doc(hidden)]
pub const SHARED_STATE_ABI_WITNESS_VERSION: u32 = 1;

/// Whether the defining derive emitted a complete canonical schema.
#[doc(hidden)]
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SharedStateAbiSchemaStatus {
    /// Every field uses the closed primitive/`Option`/`Vec` codec vocabulary.
    Resolved = 1,
    /// At least one field reaches another derived client codec. The inline
    /// payload carries a typed unresolved marker and the canonical linker must
    /// reject it until recursive codec witnesses are available.
    UnresolvedNestedClientProps = 2,
}

/// Const-friendly metadata retained beside one SharedState definition.
///
/// Object extraction follows relocations to this value but never decodes its
/// target-native Rust layout. The authoritative public identity and schema are
/// the inline SharedState payload joined to the definition record.
#[doc(hidden)]
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct SharedStateAbiWitness {
    /// [`SHARED_STATE_ABI_WITNESS_MAGIC`].
    pub format_magic: [u8; 8],
    /// [`SHARED_STATE_ABI_WITNESS_VERSION`].
    pub witness_version: u32,
    /// Browser/runtime SharedState protocol version.
    pub shared_state_abi_version: u32,
    /// Whether the inline schema payload is complete.
    pub schema_status: SharedStateAbiSchemaStatus,
    /// Explicit public protocol key.
    pub type_key: &'static str,
    /// Hash of the exact inner canonical schema ABI bytes.
    pub schema_abi_hash: [u8; 32],
    /// Logical label for diagnostics only.
    pub defining_symbol: &'static str,
}

impl SharedStateAbiWitness {
    /// Return the address that acts as this build's nominal Rust type witness.
    #[doc(hidden)]
    pub fn defining_address(&'static self) -> *const Self {
        core::ptr::from_ref(self)
    }
}

/// Hidden supertrait implemented by `#[derive(rsjs::SharedState)]`.
///
/// The public authoring surface remains `SharedState`; generated acquisition
/// records use this associated reference to let rustc select the exact defining
/// witness through normal name resolution.
#[doc(hidden)]
#[diagnostic::on_unimplemented(
    message = "`{Self}` does not carry an RSJS SharedState ABI witness",
    label = "this type is not a derived SharedState schema",
    note = "add `#[derive(rsjs::SharedState)]` and an explicit namespaced `#[rsjs(type_key = \"...\")]` to the state struct"
)]
pub trait SharedStateAbi {
    /// Rust-resolved reference to the defining witness static.
    const WITNESS: &'static SharedStateAbiWitness;
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Probe;

    static PROBE_WITNESS: SharedStateAbiWitness = SharedStateAbiWitness {
        format_magic: SHARED_STATE_ABI_WITNESS_MAGIC,
        witness_version: SHARED_STATE_ABI_WITNESS_VERSION,
        shared_state_abi_version: rsjs_transport::SHARED_STATE_RUNTIME_ABI_VERSION,
        schema_status: SharedStateAbiSchemaStatus::Resolved,
        type_key: "test.probe",
        schema_abi_hash: [0x42; 32],
        defining_symbol: "probe",
    };

    impl SharedStateAbi for Probe {
        const WITNESS: &'static SharedStateAbiWitness = &PROBE_WITNESS;
    }

    #[test]
    fn associated_reference_preserves_the_defining_static_identity() {
        assert!(core::ptr::eq(
            <Probe as SharedStateAbi>::WITNESS,
            &PROBE_WITNESS
        ));
        assert_eq!(
            <Probe as SharedStateAbi>::WITNESS.defining_address(),
            core::ptr::from_ref(&PROBE_WITNESS)
        );
    }
}
