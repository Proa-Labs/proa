// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Hidden, versioned Rust witness for compiler-authored RSJS components.
//!
//! This module is deliberately not part of the ordinary authoring surface.
//! `#[rsjs(component...)]` expansions implement [`ComponentAbi`] and point its
//! associated witness at one retained defining static. A later descriptor
//! linker can follow that rustc-resolved address instead of reconstructing a
//! component identity from source path tokens.

/// Magic bytes at the front of every component ABI witness.
#[doc(hidden)]
pub const COMPONENT_ABI_FORMAT_MAGIC: [u8; 8] = *b"RSJSCABI";

/// Version of the defining-witness representation in this module.
#[doc(hidden)]
pub const COMPONENT_ABI_VERSION: u32 = 1;

/// Version of the provisional source-key construction used by the proc macro.
///
/// This key is diagnostic/build metadata only. It is not the final normalized
/// package/module semantic key described by the linker RFC.
#[doc(hidden)]
pub const COMPONENT_SOURCE_KEY_VERSION: u32 = 1;

/// Descriptor schema written by the current `rsjs_macros` producer.
///
/// The macro crate cannot depend on `rsjs` without a dependency cycle, so its
/// focused integration test checks that the emitted value remains equal to
/// this runtime constant.
#[doc(hidden)]
pub const COMPONENT_DESCRIPTOR_SCHEMA_VERSION: u64 = 9;

/// Server render surface implemented by an analyzed component.
#[doc(hidden)]
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ComponentRenderKind {
    /// The component implements `WebRenderSync`.
    Sync = 0,
    /// The component implements `WebRender`.
    Async = 1,
}

/// Whether a component may originate browser-owned behavior.
#[doc(hidden)]
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ComponentOriginPermission {
    /// The component may participate in a parent graph but cannot create a
    /// writable signal, handler, query, or another client origin locally.
    ParticipantOnly = 0,
    /// The component explicitly opted into local browser origins with
    /// `#[rsjs(component, client)]`.
    OriginCapable = 1,
}

/// Const-friendly metadata retained beside one analyzed component impl.
///
/// `ComponentAbi::WITNESS` points directly at the generated defining static.
/// That reference is the build-local address/symbol identity; `defining_symbol`
/// is a stable logical record label for diagnostics. `provisional_source_key`
/// is intentionally token-derived and must not be treated as the final linker
/// semantic key.
#[doc(hidden)]
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct ComponentAbiWitness {
    /// [`COMPONENT_ABI_FORMAT_MAGIC`].
    pub format_magic: [u8; 8],
    /// [`COMPONENT_ABI_VERSION`].
    pub abi_version: u32,
    /// Descriptor schema encoded in the component's sidecar.
    pub descriptor_schema_version: u64,
    /// [`COMPONENT_SOURCE_KEY_VERSION`].
    pub source_key_version: u32,
    /// Sync or async server render surface.
    pub render_kind: ComponentRenderKind,
    /// Participant-only or origin-capable authoring permission.
    pub origin_permission: ComponentOriginPermission,
    /// Existing runtime/descriptor asset identity.
    pub asset_id: &'static str,
    /// Package, expansion module, and authored self-type tokens.
    ///
    /// This value is provisional source identity only. Rust-resolved edge
    /// relocations remain authoritative once linker extraction exists.
    pub provisional_source_key: &'static str,
    /// Logical identifier of the generated defining static.
    pub defining_symbol: &'static str,
}

impl ComponentAbiWitness {
    /// Return the address that acts as this build's defining witness identity.
    #[doc(hidden)]
    pub fn defining_address(&'static self) -> *const Self {
        core::ptr::from_ref(self)
    }

    /// Project the Rust witness facts into the target-independent definition
    /// record that carries this witness's relocation.
    ///
    /// Keeping this conversion on the witness prevents the object producer
    /// from independently reconstructing render/origin facts from JSON.
    #[doc(hidden)]
    pub const fn transport_definition_abi(&self) -> rsjs_transport::ComponentDefinitionAbi {
        let render_kind = match self.render_kind {
            ComponentRenderKind::Sync => rsjs_transport::TransportComponentRenderKind::Sync,
            ComponentRenderKind::Async => rsjs_transport::TransportComponentRenderKind::Async,
        };
        let origin_permission = match self.origin_permission {
            ComponentOriginPermission::ParticipantOnly => {
                rsjs_transport::TransportComponentOriginPermission::ParticipantOnly
            }
            ComponentOriginPermission::OriginCapable => {
                rsjs_transport::TransportComponentOriginPermission::OriginCapable
            }
        };
        rsjs_transport::ComponentDefinitionAbi::new(render_kind, origin_permission)
    }
}

/// Implemented exactly once for each type carrying `#[rsjs(component...)]`.
///
/// The associated reference must point directly at the generated `#[used]`
/// defining static so a future artifact extractor can follow its relocation.
#[doc(hidden)]
#[diagnostic::on_unimplemented(
    message = "`{Self}` does not carry an RSJS component ABI witness",
    label = "this type is not an analyzed RSJS component",
    note = "add `#[rsjs(component)]` to exactly one real `WebRenderSync` or `WebRender` impl; add `client` only when that component originates browser behavior"
)]
pub trait ComponentAbi {
    /// Rust-resolved reference to this component's defining witness static.
    const WITNESS: &'static ComponentAbiWitness;
}

/// One child component site selected by a linked parent render plan.
///
/// The child site is a value-level token instead of a reference to a static in
/// the child's crate. This keeps separately compiled component crates from
/// requiring cross-crate references to plan-specific statics. The child type's
/// generated lookup resolves the token during the final, plan-specialized
/// compilation.
#[doc(hidden)]
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct LinkedChildSite {
    /// Parent-local component edge ID.
    pub edge_id: u32,
    /// Stable logical render-root scope identity.
    pub scope_id: [u8; 32],
    /// Stable linked component-site coordinate.
    pub candidate_id: u64,
}

impl LinkedChildSite {
    /// Construct one plan-specific child-site token.
    #[doc(hidden)]
    pub const fn new(edge_id: u32, scope_id: [u8; 32], candidate_id: u64) -> Self {
        Self {
            edge_id,
            scope_id,
            candidate_id,
        }
    }
}

/// Final time semantics for one inbound live prop use.
///
/// This is plan metadata only. Browser lowering consumes it in a later
/// compiler/runtime slice; retaining it on the exact component occurrence
/// prevents a later stage from reconstructing temporal behavior from source
/// syntax or descriptor defaults.
#[doc(hidden)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum LinkedTemporalPropMode {
    /// Read the current parent-owned value whenever the child use evaluates.
    LiveRead,
    /// Sample the parent-owned value once when the child-local state cell is
    /// initialized.
    SeedOnce {
        /// Child-local prop-use site that owns the one-time seed.
        seed_site_id: u32,
    },
    /// Re-run the child effect when the parent-owned value changes.
    EffectDependency,
    /// Rebind the child query observer when the parent-owned key changes.
    QueryKeyDependency,
}

/// Final dispatch semantics for a compiler-owned callback prop.
#[doc(hidden)]
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum LinkedCallbackDispatch {
    /// Resolve the callback currently connected to the live parent edge at
    /// invocation time.
    ResolveCurrent = 0,
}

/// One exact inbound live prop use attached to a linked component occurrence.
#[doc(hidden)]
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct LinkedLiveProp {
    /// Concrete parent occurrence.
    pub parent_candidate_id: u64,
    /// Parent-local component edge.
    pub edge_id: u32,
    /// Concrete child occurrence carrying this record.
    pub child_candidate_id: u64,
    /// Stable child field ID.
    pub field_id: u32,
    /// Rust-validated client crossing hook.
    pub crossing_hook_id: u32,
    /// Parent graph value connected to the field.
    pub parent_value_id: u32,
    /// Child graph value produced by the field.
    pub child_value_id: u32,
    /// Exact child prop-use site.
    pub prop_site_id: u32,
    /// Time semantics selected by the semantic linker.
    pub temporal: LinkedTemporalPropMode,
}

impl LinkedLiveProp {
    /// Construct one plan-specific inbound live prop record.
    #[doc(hidden)]
    #[allow(clippy::too_many_arguments)]
    pub const fn new(
        parent_candidate_id: u64,
        edge_id: u32,
        child_candidate_id: u64,
        field_id: u32,
        crossing_hook_id: u32,
        parent_value_id: u32,
        child_value_id: u32,
        prop_site_id: u32,
        temporal: LinkedTemporalPropMode,
    ) -> Self {
        Self {
            parent_candidate_id,
            edge_id,
            child_candidate_id,
            field_id,
            crossing_hook_id,
            parent_value_id,
            child_value_id,
            prop_site_id,
            temporal,
        }
    }
}

/// One exact compiler-owned callback attached to a linked child occurrence.
#[doc(hidden)]
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct LinkedLiveCallback {
    /// Concrete parent occurrence.
    pub parent_candidate_id: u64,
    /// Parent-local component edge.
    pub edge_id: u32,
    /// Concrete child occurrence carrying this record.
    pub child_candidate_id: u64,
    /// Stable child callback field ID.
    pub field_id: u32,
    /// Rust-validated callback crossing hook.
    pub crossing_hook_id: u32,
    /// Parent graph callback value connected to the field.
    pub parent_value_id: u32,
    /// Child graph callback value receiving the live edge.
    pub child_value_id: u32,
    /// Invocation-time dispatch selected by the semantic linker.
    pub dispatch: LinkedCallbackDispatch,
}

impl LinkedLiveCallback {
    /// Construct one plan-specific live callback record.
    #[doc(hidden)]
    pub const fn new(
        parent_candidate_id: u64,
        edge_id: u32,
        child_candidate_id: u64,
        field_id: u32,
        crossing_hook_id: u32,
        parent_value_id: u32,
        child_value_id: u32,
        dispatch: LinkedCallbackDispatch,
    ) -> Self {
        Self {
            parent_candidate_id,
            edge_id,
            child_candidate_id,
            field_id,
            crossing_hook_id,
            parent_value_id,
            child_value_id,
            dispatch,
        }
    }
}

/// Exact lexical SharedState provider selected for one component-local
/// acquisition.
#[doc(hidden)]
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct LinkedSharedStateRequirement {
    /// Consumer-local canonical SharedState reference ID.
    pub reference_id: u32,
    /// Exact final-plan provider occurrence. This is deliberately stronger
    /// than a SharedState type key.
    pub provider: proa_core::RsjsLinkedProviderOccurrenceKey,
}

impl LinkedSharedStateRequirement {
    /// Construct one exact provider requirement.
    #[doc(hidden)]
    pub const fn new(
        reference_id: u32,
        scope_id: [u8; 32],
        provider_host_candidate_id: u64,
        provider_site_id: u32,
    ) -> Self {
        Self {
            reference_id,
            provider: proa_core::RsjsLinkedProviderOccurrenceKey::new(
                scope_id,
                provider_host_candidate_id,
                provider_site_id,
            ),
        }
    }
}

/// Effective activation strategy selected by final owner partitioning.
///
/// This is occurrence-specific: the same component definition can be fused
/// into owners with different final strategies at different call sites.
#[doc(hidden)]
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LinkedOwnerActivationStrategy {
    Eager,
    Visible,
    Idle,
    Interaction { event: Option<&'static str> },
    Media { query: &'static str },
}

/// Static render decision for one concrete occurrence of a component.
///
/// `owner_id` and `owner_root_candidate_id` are `None` for pure server
/// occurrences. An active owner's root occurrence has
/// `owner_root_candidate_id == Some(candidate_id)`; fused members name that
/// same root candidate. Child entries are sorted by `edge_id`. Live input
/// records are attached to the concrete child occurrence rather than the
/// component definition, because one definition can participate in several
/// parents with different graph values and temporal uses.
#[doc(hidden)]
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LinkedRenderSitePlan {
    /// Stable logical render-root scope identity.
    pub scope_id: [u8; 32],
    /// Stable linked component-site coordinate.
    pub candidate_id: u64,
    /// Final owner partition, if this occurrence participates in browser work.
    pub owner_id: Option<u64>,
    /// Candidate at which that owner is rooted.
    pub owner_root_candidate_id: Option<u64>,
    /// Effective activation strategy of the selected owner.
    pub owner_strategy: Option<LinkedOwnerActivationStrategy>,
    /// Plan-specific owner module asset. Fused members name the enclosing
    /// owner but do not publish the asset themselves.
    pub owner_asset_id: Option<&'static str>,
    /// Response seed key for this exact occurrence.
    pub seed_key: Option<&'static str>,
    /// Definition-level reusable fragment mounted at this occurrence.
    pub fragment_asset_id: &'static str,
    /// Direct linked children selected for this component occurrence.
    pub children: &'static [LinkedChildSite],
    /// Inbound live prop uses for this exact occurrence.
    pub live_props: &'static [LinkedLiveProp],
    /// Inbound compiler-owned callbacks for this exact occurrence.
    pub live_callbacks: &'static [LinkedLiveCallback],
    /// Exact lexical provider for each reachable SharedState acquisition.
    pub shared_state_requirements: &'static [LinkedSharedStateRequirement],
}

impl LinkedRenderSitePlan {
    /// Construct one plan-specific component occurrence.
    #[doc(hidden)]
    pub const fn new(
        scope_id: [u8; 32],
        candidate_id: u64,
        owner_id: Option<u64>,
        owner_root_candidate_id: Option<u64>,
        owner_strategy: Option<LinkedOwnerActivationStrategy>,
        owner_asset_id: Option<&'static str>,
        seed_key: Option<&'static str>,
        fragment_asset_id: &'static str,
        children: &'static [LinkedChildSite],
        live_props: &'static [LinkedLiveProp],
        live_callbacks: &'static [LinkedLiveCallback],
    ) -> Self {
        Self {
            scope_id,
            candidate_id,
            owner_id,
            owner_root_candidate_id,
            owner_strategy,
            owner_asset_id,
            seed_key,
            fragment_asset_id,
            children,
            live_props,
            live_callbacks,
            shared_state_requirements: &[],
        }
    }

    /// Construct one plan-specific component occurrence with exact lexical
    /// SharedState provider requirements.
    #[doc(hidden)]
    #[allow(clippy::too_many_arguments)]
    pub const fn new_with_shared_state(
        scope_id: [u8; 32],
        candidate_id: u64,
        owner_id: Option<u64>,
        owner_root_candidate_id: Option<u64>,
        owner_strategy: Option<LinkedOwnerActivationStrategy>,
        owner_asset_id: Option<&'static str>,
        seed_key: Option<&'static str>,
        fragment_asset_id: &'static str,
        children: &'static [LinkedChildSite],
        live_props: &'static [LinkedLiveProp],
        live_callbacks: &'static [LinkedLiveCallback],
        shared_state_requirements: &'static [LinkedSharedStateRequirement],
    ) -> Self {
        Self {
            scope_id,
            candidate_id,
            owner_id,
            owner_root_candidate_id,
            owner_strategy,
            owner_asset_id,
            seed_key,
            fragment_asset_id,
            children,
            live_props,
            live_callbacks,
            shared_state_requirements,
        }
    }

    /// Whether this occurrence publishes and activates the final owner asset.
    #[doc(hidden)]
    pub const fn is_owner_root(&self) -> bool {
        matches!(
            (self.owner_id, self.owner_root_candidate_id),
            (Some(_), Some(root)) if root == self.candidate_id
        )
    }

    /// Resolve a parent-local edge to its exact child-site token.
    ///
    /// Final-plan validation guarantees uniqueness. Missing or duplicate
    /// entries indicate stale/malformed generated code and therefore fail
    /// closed rather than silently falling back to a standalone plan.
    #[doc(hidden)]
    pub fn child(&'static self, edge_id: u32) -> LinkedChildSite {
        let mut found = None;
        for child in self.children {
            if child.edge_id == edge_id {
                assert!(
                    found.is_none(),
                    "RSJS linked render plan contains duplicate child edge {edge_id} for candidate {}",
                    self.candidate_id
                );
                found = Some(*child);
            }
        }
        found.unwrap_or_else(|| {
            panic!(
                "RSJS linked render plan has no child edge {edge_id} for candidate {}",
                self.candidate_id
            )
        })
    }

    /// Resolve one component-local SharedState acquisition to the exact
    /// lexical provider selected by semantic linking.
    #[doc(hidden)]
    pub fn shared_state_provider(
        &'static self,
        reference_id: u32,
    ) -> proa_core::RsjsLinkedProviderOccurrenceKey {
        let mut found = None;
        for requirement in self.shared_state_requirements {
            if requirement.reference_id == reference_id {
                assert!(
                    found.is_none(),
                    "RSJS linked render plan contains duplicate SharedState reference {reference_id} for candidate {}",
                    self.candidate_id
                );
                found = Some(requirement.provider);
            }
        }
        found.unwrap_or_else(|| {
            panic!(
                "RSJS linked render plan has no SharedState provider for reference {reference_id} on candidate {}",
                self.candidate_id
            )
        })
    }

    fn assert_identity(&self, scope_id: [u8; 32], candidate_id: u64) {
        assert!(
            self.scope_id == scope_id && self.candidate_id == candidate_id,
            "RSJS linked render-site lookup returned the wrong plan or candidate"
        );
    }
}

/// Compiler-only synchronous entry for rendering a component at a linked site.
///
/// Implementations are generated beside the authored `WebRenderSync` impl.
/// They factor the authored body once, select an explicit static plan, and do
/// not consult ambient render mode.
#[doc(hidden)]
pub trait ComponentSiteRenderSync<
    L: proa_core::DataLoader = proa_core::NoLoader,
    B: proa_core::WriteBuf = proa_core::RenderBuffer,
>: Sized
{
    /// Resolve a plan-specific candidate to a static render-site plan.
    #[doc(hidden)]
    fn linked_site(candidate_id: u64, expected_scope_id: [u8; 32])
        -> &'static LinkedRenderSitePlan;

    /// Render the authored component body under an explicit site plan.
    #[doc(hidden)]
    fn render_at(
        self,
        cx: &mut proa_core::WebContext<L, B>,
        site: &'static LinkedRenderSitePlan,
    ) -> Result<(), proa_core::WriteError>;
}

/// Compiler-only asynchronous entry for rendering a component at a linked
/// site.
///
/// The RPITIT return is delegated directly by [`LinkedComponent`]. No boxed
/// future, wrapper future, or eager poll is introduced at the site boundary.
#[doc(hidden)]
pub trait ComponentSiteRender<
    L: proa_core::DataLoader = proa_core::NoLoader,
    B: proa_core::WriteBuf = proa_core::RenderBuffer,
>: Sized
{
    /// Resolve a plan-specific candidate to a static render-site plan.
    #[doc(hidden)]
    fn linked_site(candidate_id: u64, expected_scope_id: [u8; 32])
        -> &'static LinkedRenderSitePlan;

    /// Render the authored component body under an explicit site plan.
    #[doc(hidden)]
    fn render_at(
        self,
        cx: &mut proa_core::WebContext<L, B>,
        site: &'static LinkedRenderSitePlan,
    ) -> proa_core::RenderOutcome<
        impl core::future::Future<Output = Result<(), proa_core::WriteError>>,
    >;
}

/// Allocation-free adapter that renders a component through one linked child
/// site rather than through its standalone render-trait entry.
#[doc(hidden)]
#[derive(Clone, Copy, Debug)]
pub struct LinkedComponent<T> {
    component: T,
    site: LinkedChildSite,
}

impl<T> LinkedComponent<T> {
    /// Pair a component value with a plan-specific child-site token.
    #[doc(hidden)]
    pub const fn new(component: T, site: LinkedChildSite) -> Self {
        Self { component, site }
    }
}

impl<T, L, B> proa_core::WebRenderSync<L, B> for LinkedComponent<T>
where
    T: ComponentSiteRenderSync<L, B>,
    B: proa_core::WriteBuf,
    L: proa_core::DataLoader,
{
    #[inline]
    fn render(self, cx: &mut proa_core::WebContext<L, B>) -> Result<(), proa_core::WriteError> {
        let plan = T::linked_site(self.site.candidate_id, self.site.scope_id);
        plan.assert_identity(self.site.scope_id, self.site.candidate_id);
        T::render_at(self.component, cx, plan)
    }
}

impl<T, L, B> proa_core::WebRender<L, B> for LinkedComponent<T>
where
    T: ComponentSiteRender<L, B>,
    B: proa_core::WriteBuf,
    L: proa_core::DataLoader,
{
    #[inline]
    fn render(
        self,
        cx: &mut proa_core::WebContext<L, B>,
    ) -> proa_core::RenderOutcome<
        impl core::future::Future<Output = Result<(), proa_core::WriteError>>,
    > {
        let plan = T::linked_site(self.site.candidate_id, self.site.scope_id);
        plan.assert_identity(self.site.scope_id, self.site.candidate_id);
        T::render_at(self.component, cx, plan)
    }
}

/// Emit one descriptor-pass component edge through rustc-resolved type
/// identity.
///
/// This is a compiler-internal prototype surface. The eventual RSJS template
/// linker calls it for direct component syntax after assigning the source edge
/// ID. The record's relocation comes from
/// `<$component_ty as ComponentAbi>::WITNESS`; the type's source spelling is
/// never encoded as identity.
#[doc(hidden)]
#[macro_export]
macro_rules! __rsjs_emit_component_edge {
    (
        $record_ident:ident,
        $cargo_unit_id:expr,
        $source_edge_id:expr,
        $component_ty:ty
    ) => {
        #[doc(hidden)]
        #[allow(non_upper_case_globals)]
        const _: () = {
            #[used]
            #[cfg_attr(target_vendor = "apple", unsafe(link_section = "__DATA,__rsjs_edge"))]
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
                unsafe(link_section = ".rsjs.edge")
            )]
            #[cfg_attr(target_os = "windows", unsafe(link_section = ".rsjs$E"))]
            static $record_ident: $crate::__private::ComponentEdgeRecord<
                $crate::__private::ComponentAbiWitness,
            > = $crate::__private::ComponentEdgeRecord::new(
                $cargo_unit_id,
                $source_edge_id,
                <$component_ty as $crate::__private::ComponentAbi>::WITNESS,
            );

            #[cfg(not(any(
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
            )))]
            compile_error!(
                "RSJS descriptor relocation transport does not support this target object format"
            );
        };
    };
    ($record_ident:ident, $source_edge_id:expr, $component_ty:ty) => {
        $crate::__rsjs_emit_component_edge!(
            $record_ident,
            $crate::__private::UNASSIGNED_CARGO_UNIT_ID,
            $source_edge_id,
            $component_ty
        );
    };
}

/// Emit one rustc-checked inbound component-field witness.
///
/// The record relocates to the defining component witness itself. Field
/// existence and callback shape are checked by a separate generated,
/// never-called Rust validation method; the transport deliberately does not
/// manufacture a nominal type hash.
#[doc(hidden)]
#[macro_export]
macro_rules! __rsjs_emit_component_field {
    (
        $record_ident:ident,
        $cargo_unit_id:expr,
        $field_record_id:expr,
        $component_witness:expr
    ) => {
        #[doc(hidden)]
        #[allow(non_upper_case_globals)]
        const _: () = {
            #[used]
            #[cfg_attr(target_vendor = "apple", unsafe(link_section = "__DATA,__rsjs_field"))]
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
                unsafe(link_section = ".rsjs.component_field")
            )]
            #[cfg_attr(target_os = "windows", unsafe(link_section = ".rsjs$F"))]
            static $record_ident: $crate::__private::ComponentFieldRecord<
                $crate::__private::ComponentAbiWitness,
            > = $crate::__private::ComponentFieldRecord::new(
                $cargo_unit_id,
                $field_record_id,
                $component_witness,
            );

            #[cfg(not(any(
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
            )))]
            compile_error!(
                "RSJS descriptor relocation transport does not support this target object format"
            );
        };
    };
}

#[cfg(test)]
mod tests {
    use super::*;
    use proa_core::{DataLoader, RenderOutcome, WebContext, WriteBuf, WriteError};

    struct Probe;

    static PROBE_WITNESS: ComponentAbiWitness = ComponentAbiWitness {
        format_magic: COMPONENT_ABI_FORMAT_MAGIC,
        abi_version: COMPONENT_ABI_VERSION,
        descriptor_schema_version: COMPONENT_DESCRIPTOR_SCHEMA_VERSION,
        source_key_version: COMPONENT_SOURCE_KEY_VERSION,
        render_kind: ComponentRenderKind::Sync,
        origin_permission: ComponentOriginPermission::ParticipantOnly,
        asset_id: "probe",
        provisional_source_key: "rsjs::component_abi::tests::Probe",
        defining_symbol: "PROBE_WITNESS",
    };

    impl ComponentAbi for Probe {
        const WITNESS: &'static ComponentAbiWitness = &PROBE_WITNESS;
    }

    #[test]
    fn associated_witness_preserves_the_defining_static_address() {
        assert_eq!(Probe::WITNESS.format_magic, *b"RSJSCABI");
        assert!(core::ptr::eq(
            Probe::WITNESS,
            &PROBE_WITNESS as &'static ComponentAbiWitness
        ));
        assert_eq!(
            Probe::WITNESS.defining_address(),
            core::ptr::from_ref(&PROBE_WITNESS)
        );
    }

    #[test]
    fn transport_projection_comes_from_the_typed_component_witness() {
        let mut witness = PROBE_WITNESS;
        witness.render_kind = ComponentRenderKind::Async;
        witness.origin_permission = ComponentOriginPermission::OriginCapable;

        assert_eq!(
            witness.transport_definition_abi(),
            rsjs_transport::ComponentDefinitionAbi::new(
                rsjs_transport::TransportComponentRenderKind::Async,
                rsjs_transport::TransportComponentOriginPermission::OriginCapable,
            )
        );
    }

    const SCOPE_ID: [u8; 32] = [7; 32];
    static CHILDREN: [LinkedChildSite; 1] = [LinkedChildSite::new(9, SCOPE_ID, 4)];
    static LIVE_PROPS: [LinkedLiveProp; 4] = [
        LinkedLiveProp::new(
            2,
            9,
            4,
            10,
            20,
            30,
            40,
            50,
            LinkedTemporalPropMode::LiveRead,
        ),
        LinkedLiveProp::new(
            2,
            9,
            4,
            11,
            21,
            31,
            41,
            51,
            LinkedTemporalPropMode::SeedOnce { seed_site_id: 51 },
        ),
        LinkedLiveProp::new(
            2,
            9,
            4,
            12,
            22,
            32,
            42,
            52,
            LinkedTemporalPropMode::EffectDependency,
        ),
        LinkedLiveProp::new(
            2,
            9,
            4,
            13,
            23,
            33,
            43,
            53,
            LinkedTemporalPropMode::QueryKeyDependency,
        ),
    ];
    static LIVE_CALLBACKS: [LinkedLiveCallback; 1] = [LinkedLiveCallback::new(
        2,
        9,
        4,
        60,
        61,
        62,
        63,
        LinkedCallbackDispatch::ResolveCurrent,
    )];
    static SITE: LinkedRenderSitePlan = LinkedRenderSitePlan::new(
        SCOPE_ID,
        4,
        Some(2),
        Some(4),
        Some(LinkedOwnerActivationStrategy::Visible),
        Some("rsjs_owner_test_2"),
        Some("rsjs_owner_test_2"),
        "sync_probe_fragment",
        &CHILDREN,
        &LIVE_PROPS,
        &LIVE_CALLBACKS,
    );

    struct SyncProbe(&'static str);

    impl<L: DataLoader, B: WriteBuf> ComponentSiteRenderSync<L, B> for SyncProbe {
        fn linked_site(candidate_id: u64, scope_id: [u8; 32]) -> &'static LinkedRenderSitePlan {
            assert_eq!((scope_id, candidate_id), (SCOPE_ID, 4));
            &SITE
        }

        fn render_at(
            self,
            cx: &mut WebContext<L, B>,
            plan: &'static LinkedRenderSitePlan,
        ) -> Result<(), WriteError> {
            assert!(core::ptr::eq(plan, &SITE));
            cx.out.extend(self.0.as_bytes())
        }
    }

    #[test]
    fn linked_sync_component_delegates_through_the_exact_site() {
        let mut output = Vec::new();
        let mut cx = WebContext::with_buffer(&mut output);
        let component = LinkedComponent::new(SyncProbe("linked"), CHILDREN[0]);
        proa_core::WebRenderSync::render(component, &mut cx).unwrap();
        assert_eq!(output, b"linked");
        assert_eq!(SITE.child(9), CHILDREN[0]);
        assert_eq!(SITE.live_props, &LIVE_PROPS);
        assert_eq!(SITE.live_callbacks, &LIVE_CALLBACKS);
        assert_eq!(
            SITE.owner_strategy,
            Some(LinkedOwnerActivationStrategy::Visible)
        );
        assert!(SITE.is_owner_root());
        assert_eq!(SITE.owner_asset_id, Some("rsjs_owner_test_2"));
        assert_eq!(SITE.seed_key, Some("rsjs_owner_test_2"));
        assert_eq!(SITE.fragment_asset_id, "sync_probe_fragment");
        assert!(matches!(
            SITE.live_props[1].temporal,
            LinkedTemporalPropMode::SeedOnce { seed_site_id: 51 }
        ));
        assert_eq!(
            SITE.live_callbacks[0].dispatch,
            LinkedCallbackDispatch::ResolveCurrent
        );
        assert_eq!(SITE.live_callbacks[0].child_value_id, 63);
    }

    struct AsyncProbe;

    impl<L: DataLoader, B: WriteBuf> ComponentSiteRender<L, B> for AsyncProbe {
        fn linked_site(candidate_id: u64, scope_id: [u8; 32]) -> &'static LinkedRenderSitePlan {
            assert_eq!((scope_id, candidate_id), (SCOPE_ID, 4));
            &SITE
        }

        fn render_at(
            self,
            _cx: &mut WebContext<L, B>,
            plan: &'static LinkedRenderSitePlan,
        ) -> RenderOutcome<impl core::future::Future<Output = Result<(), WriteError>>> {
            assert!(core::ptr::eq(plan, &SITE));
            RenderOutcome::<core::future::Ready<Result<(), WriteError>>>::Done(Ok(()))
        }
    }

    #[test]
    fn linked_async_component_directly_delegates_the_rpitit_outcome() {
        let mut output = Vec::new();
        let mut cx = WebContext::with_buffer(&mut output);
        let component = LinkedComponent::new(AsyncProbe, CHILDREN[0]);
        let outcome = proa_core::WebRender::render(component, &mut cx);
        assert!(matches!(outcome, RenderOutcome::Done(Ok(()))));
    }

    struct WrongLookup;

    impl<L: DataLoader, B: WriteBuf> ComponentSiteRenderSync<L, B> for WrongLookup {
        fn linked_site(_candidate_id: u64, _scope_id: [u8; 32]) -> &'static LinkedRenderSitePlan {
            static WRONG: LinkedRenderSitePlan = LinkedRenderSitePlan::new(
                [8; 32],
                5,
                None,
                None,
                None,
                None,
                None,
                "wrong_fragment",
                &[],
                &[],
                &[],
            );
            &WRONG
        }

        fn render_at(
            self,
            _cx: &mut WebContext<L, B>,
            _plan: &'static LinkedRenderSitePlan,
        ) -> Result<(), WriteError> {
            Ok(())
        }
    }

    #[test]
    #[should_panic(expected = "wrong plan or candidate")]
    fn linked_component_fails_closed_when_lookup_returns_the_wrong_site() {
        let mut output = Vec::new();
        let mut cx = WebContext::with_buffer(&mut output);
        let component = LinkedComponent::new(WrongLookup, CHILDREN[0]);
        let _ = proa_core::WebRenderSync::render(component, &mut cx);
    }
}
