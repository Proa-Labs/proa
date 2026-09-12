// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Defining-side coverage for the hidden Rust-resolved component ABI.

use proa_core::{DataLoader, RenderOutcome, WebContext, WebRender, WriteError};
use rsjs::__private::{
    ComponentAbi, ComponentAbiWitness, ComponentOriginPermission, ComponentRenderKind,
    COMPONENT_ABI_FORMAT_MAGIC, COMPONENT_ABI_VERSION, COMPONENT_DESCRIPTOR_SCHEMA_VERSION,
    COMPONENT_SOURCE_KEY_VERSION,
};

struct AsyncOrigin;

mod participant_definition {
    use proa_core::{DataLoader, WebContext, WebRenderSync, WriteError};

    pub(super) struct Participant;

    #[rsjs::rsjs(component)]
    impl<L: DataLoader> WebRenderSync<L> for Participant {
        fn render(self, cx: &mut WebContext<L>) -> Result<(), WriteError> {
            proa_macros::html_sync! {
                <span data-participant>"participant"</span>
            }
            .render(cx)
        }
    }
}

use participant_definition::Participant;

#[rsjs::rsjs(component, client)]
impl<L: DataLoader> WebRender<L> for AsyncOrigin {
    fn render(
        self,
        cx: &mut WebContext<L>,
    ) -> RenderOutcome<impl std::future::Future<Output = Result<(), WriteError>>> {
        RenderOutcome::pending(async move {
            let count = rsjs::signal(1_i64);
            proa_macros::html! {
                <button on_click={|_| count.update(|value| value + 1_i64)}>"Increment"</button>
            }
            .render(cx)
            .resolve()
            .await
        })
    }
}

fn witness<T: ComponentAbi>() -> &'static ComponentAbiWitness {
    T::WITNESS
}

#[test]
fn participant_sync_component_implements_the_defining_witness() {
    let witness = witness::<Participant>();

    assert_eq!(witness.format_magic, COMPONENT_ABI_FORMAT_MAGIC);
    assert_eq!(witness.abi_version, COMPONENT_ABI_VERSION);
    assert_eq!(
        witness.descriptor_schema_version,
        COMPONENT_DESCRIPTOR_SCHEMA_VERSION
    );
    assert_eq!(witness.source_key_version, COMPONENT_SOURCE_KEY_VERSION);
    assert_eq!(witness.render_kind, ComponentRenderKind::Sync);
    assert_eq!(
        witness.origin_permission,
        ComponentOriginPermission::ParticipantOnly
    );
    assert_eq!(
        witness.transport_definition_abi().flags(),
        0,
        "the transport projection must come from the typed sync/participant witness"
    );
    assert!(witness.asset_id.starts_with("participant_"));
    assert!(witness.provisional_source_key.contains("Participant"));
    assert!(witness
        .defining_symbol
        .contains("__RSJS_COMPONENT_ABI_WITNESS_participant_"));
    assert_eq!(witness.defining_address(), core::ptr::from_ref(witness));
}

#[test]
fn origin_capable_async_component_has_a_distinct_defining_witness() {
    let participant = witness::<Participant>();
    let asynchronous = witness::<AsyncOrigin>();

    assert_eq!(asynchronous.render_kind, ComponentRenderKind::Async);
    assert_eq!(
        asynchronous.origin_permission,
        ComponentOriginPermission::OriginCapable
    );
    assert_eq!(
        asynchronous.transport_definition_abi().flags(),
        rsjs_transport::COMPONENT_DEFINITION_KNOWN_FLAGS,
        "the transport projection must carry both async and origin-capable bits"
    );
    assert!(asynchronous.asset_id.starts_with("asyncorigin_"));
    assert!(asynchronous.provisional_source_key.contains("AsyncOrigin"));
    assert_ne!(
        participant.defining_address(),
        asynchronous.defining_address()
    );
}
