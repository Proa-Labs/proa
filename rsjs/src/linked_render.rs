// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Render-time boundary for one component occurrence selected by the linker.
//!
//! The same authored component body can appear as pure SSR, as the root of a
//! browser owner, or as a fused member of another owner. Keeping that decision
//! in one renderable wrapper lets the proc macro preserve a single anonymous
//! `html_sync!` / `html!` value type instead of cloning the authored body into
//! three branches.

use core::future::Future;

use proa_core::{
    shared::attr_value::AttrValue, CspNonce, DataLoader, RenderOutcome,
    RsjsLinkedProviderOccurrenceKey, WebContext, WebRender, WebRenderSync, WriteBuf, WriteError,
};

use crate::component_abi::LinkedRenderSitePlan;

/// Occurrence-aware hydration-marker attribute.
///
/// A reusable component definition may participate in a browser owner at one
/// linked site and remain pure SSR at another. The server program is shared
/// between those occurrences, so marker emission must consult the exact linked
/// site instead of being fixed at definition compile time.
#[doc(hidden)]
pub struct LinkedMarkerAttr<T> {
    value: Option<T>,
}

impl<T> LinkedMarkerAttr<T> {
    /// Retain `value` only when this concrete occurrence belongs to an owner.
    #[doc(hidden)]
    pub fn new(site: &LinkedRenderSitePlan, value: T) -> Self {
        Self {
            value: if site.owner_id.is_some() {
                Some(value)
            } else {
                None
            },
        }
    }

    /// Construct `value` only when this concrete occurrence belongs to an owner.
    #[doc(hidden)]
    pub fn from_fn<F>(site: &LinkedRenderSitePlan, value: F) -> Self
    where
        F: FnOnce() -> T,
    {
        Self {
            value: site.owner_id.is_some().then(value),
        }
    }
}

impl<T: AttrValue> AttrValue for LinkedMarkerAttr<T> {
    #[inline(always)]
    fn should_render(&self) -> bool {
        self.value.as_ref().is_some_and(AttrValue::should_render)
    }

    #[inline(always)]
    fn is_boolean_attr(&self) -> bool {
        self.value.as_ref().is_some_and(AttrValue::is_boolean_attr)
    }

    #[inline(always)]
    fn url_str(&self) -> Option<&str> {
        self.value.as_ref().and_then(AttrValue::url_str)
    }

    #[inline(always)]
    fn render_attr_value<B: WriteBuf, L: DataLoader>(
        &self,
        cx: &mut WebContext<L, B>,
    ) -> Result<(), WriteError> {
        match self.value.as_ref() {
            Some(value) => value.render_attr_value(cx),
            None => Ok(()),
        }
    }
}

/// Build one browser-owner-only value without evaluating its factory for a
/// pure-SSR occurrence.
///
/// The returned [`Option`] is inline storage. This helper adds no allocation;
/// any allocation performed by `value` happens only for an active owner.
#[doc(hidden)]
pub fn linked_owner_value<T, F>(
    site: &LinkedRenderSitePlan,
    value: F,
) -> Result<Option<T>, WriteError>
where
    F: FnOnce() -> Result<T, WriteError>,
{
    if linked_site_is_active(site)? {
        value().map(Some)
    } else {
        Ok(None)
    }
}

/// Occurrence-aware child carrier for a compiler-recognized
/// `SharedStateProvider`.
///
/// The provider itself enters its response-local scope before rendering this
/// value. This wrapper tags that just-entered binding with the exact final-plan
/// provider occurrence, renders the child without adding DOM, and removes the
/// tag on success, error, or unwind. The owned guard remains live across async
/// suspension.
#[doc(hidden)]
pub struct LinkedSharedStateProviderChildren<C> {
    child: C,
    occurrence: RsjsLinkedProviderOccurrenceKey,
}

impl<C> LinkedSharedStateProviderChildren<C> {
    /// Wrap provider children with their exact final-plan occurrence.
    #[doc(hidden)]
    pub const fn new(
        child: C,
        plan_id: [u8; 32],
        host_candidate_id: u32,
        provider_site_id: u32,
    ) -> Self {
        Self {
            child,
            occurrence: RsjsLinkedProviderOccurrenceKey::new(
                plan_id,
                host_candidate_id,
                provider_site_id,
            ),
        }
    }
}

impl<C, L, B> WebRenderSync<L, B> for LinkedSharedStateProviderChildren<C>
where
    C: WebRenderSync<L, B>,
    B: WriteBuf,
    L: DataLoader,
{
    #[inline]
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let _linked_provider = cx.rsjs_shared_state_tag_linked_provider(self.occurrence)?;
        self.child.render(cx)
    }
}

impl<C, L, B> WebRender<L, B> for LinkedSharedStateProviderChildren<C>
where
    C: WebRender<L, B>,
    B: WriteBuf,
    L: DataLoader,
{
    #[inline]
    fn render(
        self,
        cx: &mut WebContext<L, B>,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>> {
        RenderOutcome::pending(async move {
            let _linked_provider = cx.rsjs_shared_state_tag_linked_provider(self.occurrence)?;
            self.child.render(cx).resolve().await
        })
    }
}

/// Occurrence-aware carrier for a standalone reactive text slot.
///
/// Hydrated owners need the `<span data-rsjs-bind="...">` lookup host. A
/// pure-SSR occurrence renders the exact value directly, without introducing a
/// compiler-only element into the authored DOM.
#[doc(hidden)]
pub struct LinkedTextBinding<T> {
    child: T,
    site: &'static LinkedRenderSitePlan,
    marker: &'static str,
}

impl<T> LinkedTextBinding<T> {
    /// Wrap one text-binding value with its exact final-plan occurrence.
    #[doc(hidden)]
    pub const fn new(child: T, site: &'static LinkedRenderSitePlan, marker: &'static str) -> Self {
        Self {
            child,
            site,
            marker,
        }
    }
}

impl<T, L, B> WebRenderSync<L, B> for LinkedTextBinding<T>
where
    T: WebRenderSync<L, B>,
    B: WriteBuf,
    L: DataLoader,
{
    #[inline]
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let Self {
            child,
            site,
            marker,
        } = self;
        if !linked_site_is_active(site)? {
            return child.render(cx);
        }

        write_text_binding_prefix(marker, cx)?;
        child.render(cx)?;
        cx.out.extend_static(b"</span>")
    }
}

impl<T, L, B> WebRender<L, B> for LinkedTextBinding<T>
where
    T: WebRender<L, B>,
    B: WriteBuf,
    L: DataLoader,
{
    #[inline]
    fn render(
        self,
        cx: &mut WebContext<L, B>,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>> {
        RenderOutcome::pending(async move {
            let Self {
                child,
                site,
                marker,
            } = self;
            if !linked_site_is_active(site)? {
                return child.render(cx).resolve().await;
            }

            write_text_binding_prefix(marker, cx)?;
            child.render(cx).resolve().await?;
            cx.out.extend_static(b"</span>")
        })
    }
}

/// One compiler-selected render occurrence around an ordinary Proa child.
///
/// Pure-server sites render `child` byte-for-byte. Active sites emit one
/// inert wrapper and adjacent state records. Only the final owner root
/// publishes the owner asset; fused members merely expose their DOM and seeds
/// to that root module.
#[doc(hidden)]
pub struct LinkedRenderBoundary<'state, C> {
    child: C,
    site: &'static LinkedRenderSitePlan,
    signal_state: Option<&'state str>,
    query_state: Option<&'state str>,
    mutation_state: Option<&'state str>,
}

impl<'state, C> LinkedRenderBoundary<'state, C> {
    /// Wrap one authored render value with its exact final-plan occurrence.
    #[doc(hidden)]
    pub const fn new(
        child: C,
        site: &'static LinkedRenderSitePlan,
        signal_state: Option<&'state str>,
        query_state: Option<&'state str>,
        mutation_state: Option<&'state str>,
    ) -> Self {
        Self {
            child,
            site,
            signal_state,
            query_state,
            mutation_state,
        }
    }
}

impl<C, L, B> WebRenderSync<L, B> for LinkedRenderBoundary<'_, C>
where
    C: WebRenderSync<L, B>,
    B: WriteBuf,
    L: DataLoader,
{
    #[inline]
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let Self {
            child,
            site,
            signal_state,
            query_state,
            mutation_state,
        } = self;
        if !linked_site_is_active(site)? {
            return child.render(cx);
        }

        let signal_state = require_active_signal_state(signal_state)?;
        let nonce = write_prefix(site, cx)?;
        child.render(cx)?;
        write_suffix(
            site,
            signal_state,
            query_state,
            mutation_state,
            cx,
            nonce.as_ref(),
        )
    }
}

impl<C, L, B> WebRender<L, B> for LinkedRenderBoundary<'_, C>
where
    C: WebRender<L, B>,
    B: WriteBuf,
    L: DataLoader,
{
    #[inline]
    fn render(
        self,
        cx: &mut WebContext<L, B>,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>> {
        RenderOutcome::pending(async move {
            let Self {
                child,
                site,
                signal_state,
                query_state,
                mutation_state,
            } = self;
            if !linked_site_is_active(site)? {
                return child.render(cx).resolve().await;
            }

            let signal_state = require_active_signal_state(signal_state)?;
            let nonce = write_prefix(site, cx)?;
            child.render(cx).resolve().await?;
            write_suffix(
                site,
                signal_state,
                query_state,
                mutation_state,
                cx,
                nonce.as_ref(),
            )
        })
    }
}

#[inline]
#[doc(hidden)]
pub fn linked_site_is_active(site: &LinkedRenderSitePlan) -> Result<bool, WriteError> {
    let active = site.owner_id.is_some();
    let coherent = if active {
        site.owner_root_candidate_id.is_some()
            && site.owner_strategy.is_some()
            && site.owner_asset_id.is_some()
            && site.seed_key.is_some()
    } else {
        site.owner_root_candidate_id.is_none()
            && site.owner_strategy.is_none()
            && site.owner_asset_id.is_none()
            && site.seed_key.is_none()
    };
    if coherent {
        Ok(active)
    } else {
        Err(WriteError::InvalidAttribute(
            "incoherent RSJS linked render-site owner metadata",
        ))
    }
}

#[inline]
fn write_prefix<B: WriteBuf, L: DataLoader>(
    site: &'static LinkedRenderSitePlan,
    cx: &mut WebContext<L, B>,
) -> Result<Option<CspNonce>, WriteError> {
    let owner_asset_id = site.owner_asset_id.ok_or(WriteError::InvalidAttribute(
        "active RSJS linked site is missing its owner asset",
    ))?;
    if site.is_owner_root() {
        cx.rsjs_island_used(owner_asset_id);
    }

    let seed_key = site.seed_key.ok_or(WriteError::InvalidAttribute(
        "active RSJS linked site is missing its seed key",
    ))?;
    cx.out
        .extend_static(b"<rsjs-island data-rsjs-island-root=\"")?;
    cx.out.extend(seed_key.as_bytes())?;
    cx.out.extend_static(b"\" data-rsjs-linked-candidate=\"")?;
    write_u32(&mut cx.out, site.candidate_id)?;
    cx.out.extend_static(b"\">")?;
    Ok(cx.csp_nonce_cloned())
}

#[inline]
fn write_suffix<B: WriteBuf, L: DataLoader>(
    site: &'static LinkedRenderSitePlan,
    signal_state: &str,
    query_state: Option<&str>,
    mutation_state: Option<&str>,
    cx: &mut WebContext<L, B>,
    nonce: Option<&CspNonce>,
) -> Result<(), WriteError> {
    let seed_key = site.seed_key.ok_or(WriteError::InvalidAttribute(
        "active RSJS linked site is missing its seed key",
    ))?;
    cx.out
        .extend_static(b"</rsjs-island><script type=\"application/json\" data-rsjs-island=\"")?;
    cx.out.extend(seed_key.as_bytes())?;
    cx.out.extend_static(b"\"")?;
    write_nonce(cx, nonce)?;
    cx.out.extend_static(b">")?;
    cx.out.extend(signal_state.as_bytes())?;
    cx.out.extend_static(b"</script>")?;

    if let Some(query_state) = query_state {
        write_state_script(
            cx,
            b" data-rsjs-island-queries=\"",
            site.fragment_asset_id,
            query_state,
            nonce,
        )?;
    }
    if let Some(mutation_state) = mutation_state {
        write_state_script(
            cx,
            b" data-rsjs-island-mutations=\"",
            site.fragment_asset_id,
            mutation_state,
            nonce,
        )?;
    }
    Ok(())
}

#[inline]
fn require_active_signal_state(signal_state: Option<&str>) -> Result<&str, WriteError> {
    signal_state.ok_or(WriteError::InvalidAttribute(
        "active RSJS linked site is missing its signal seed",
    ))
}

#[inline]
fn write_text_binding_prefix<B: WriteBuf, L: DataLoader>(
    marker: &str,
    cx: &mut WebContext<L, B>,
) -> Result<(), WriteError> {
    cx.out.extend_static(b"<span data-rsjs-bind=\"")?;
    cx.out.extend(marker.as_bytes())?;
    cx.out.extend_static(b"\">")
}

#[inline]
fn write_nonce<B: WriteBuf, L: DataLoader>(
    cx: &mut WebContext<L, B>,
    nonce: Option<&CspNonce>,
) -> Result<(), WriteError> {
    if let Some(nonce) = nonce {
        nonce.write_html_attr(&mut cx.out)?;
    }
    Ok(())
}

#[inline]
fn write_state_script<B: WriteBuf, L: DataLoader>(
    cx: &mut WebContext<L, B>,
    attribute: &'static [u8],
    fragment_asset_id: &'static str,
    state: &str,
    nonce: Option<&CspNonce>,
) -> Result<(), WriteError> {
    cx.out.extend_static(b"<script type=\"application/json\"")?;
    cx.out.extend_static(attribute)?;
    cx.out.extend(fragment_asset_id.as_bytes())?;
    cx.out.extend_static(b"\"")?;
    write_nonce(cx, nonce)?;
    cx.out.extend_static(b">")?;
    cx.out.extend(state.as_bytes())?;
    cx.out.extend_static(b"</script>")
}

#[inline]
fn write_u32<B: WriteBuf>(out: &mut B, mut value: u32) -> Result<(), WriteError> {
    let mut digits = [0_u8; 10];
    let mut cursor = digits.len();
    loop {
        cursor -= 1;
        digits[cursor] = b'0' + (value % 10) as u8;
        value /= 10;
        if value == 0 {
            return out.extend(&digits[cursor..]);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    static SERVER_SITE: LinkedRenderSitePlan = LinkedRenderSitePlan::new(
        [1; 32],
        0,
        None,
        None,
        None,
        None,
        None,
        "server_fragment",
        &[],
        &[],
        &[],
    );
    static ROOT_SITE: LinkedRenderSitePlan = LinkedRenderSitePlan::new(
        [2; 32],
        7,
        Some(3),
        Some(7),
        Some(crate::component_abi::LinkedOwnerActivationStrategy::Visible),
        Some("rsjs_owner_test_3"),
        Some("rsjs_owner_test_3"),
        "root_fragment",
        &[],
        &[],
        &[],
    );
    static MEMBER_SITE: LinkedRenderSitePlan = LinkedRenderSitePlan::new(
        [2; 32],
        12,
        Some(3),
        Some(7),
        Some(crate::component_abi::LinkedOwnerActivationStrategy::Visible),
        Some("rsjs_owner_test_3"),
        Some("rsjs_member_test_12"),
        "member_fragment",
        &[],
        &[],
        &[],
    );

    struct Bytes(&'static [u8]);

    impl<L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for Bytes {
        fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
            cx.out.extend(self.0)
        }
    }

    const LINKED_PROVIDER_PLAN_ID: [u8; 32] = [0x55; 32];
    const LINKED_PROVIDER_HOST: u32 = 9;
    const LINKED_PROVIDER_SITE: u32 = 3;

    fn linked_provider_occurrence() -> RsjsLinkedProviderOccurrenceKey {
        RsjsLinkedProviderOccurrenceKey::new(
            LINKED_PROVIDER_PLAN_ID,
            LINKED_PROVIDER_HOST,
            LINKED_PROVIDER_SITE,
        )
    }

    struct LinkedProviderSyncProbe;

    impl<L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for LinkedProviderSyncProbe {
        fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
            let (value, _) = cx.rsjs_shared_state_acquire_linked::<u8>(
                linked_provider_occurrence(),
                "test.linked-wrapper",
                "sha256:linked-wrapper",
                "LinkedWrapperState",
                "LinkedProviderSyncProbe",
            )?;
            cx.out.extend(&[*value])
        }
    }

    struct LinkedProviderAsyncProbe;

    impl<L: DataLoader, B: WriteBuf> WebRender<L, B> for LinkedProviderAsyncProbe {
        fn render(
            self,
            cx: &mut WebContext<L, B>,
        ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>> {
            RenderOutcome::pending(async move {
                tokio::task::yield_now().await;
                let (value, _) = cx.rsjs_shared_state_acquire_linked::<u8>(
                    linked_provider_occurrence(),
                    "test.linked-wrapper-async",
                    "sha256:linked-wrapper-async",
                    "LinkedWrapperAsyncState",
                    "LinkedProviderAsyncProbe",
                )?;
                cx.out.extend(&[*value])
            })
        }
    }

    #[test]
    fn linked_provider_children_tag_only_the_child_render_extent() {
        let mut output = Vec::new();
        let mut cx = WebContext::with_buffer(&mut output);
        let _scope = cx
            .rsjs_shared_state_enter(
                "test.linked-wrapper",
                "sha256:linked-wrapper",
                "LinkedWrapperState",
                &[],
                std::rc::Rc::new(61u8),
                "{\"value\":61}".to_owned(),
            )
            .unwrap();

        WebRenderSync::render(
            LinkedSharedStateProviderChildren::new(
                LinkedProviderSyncProbe,
                LINKED_PROVIDER_PLAN_ID,
                LINKED_PROVIDER_HOST,
                LINKED_PROVIDER_SITE,
            ),
            &mut cx,
        )
        .unwrap();
        assert!(
            cx.rsjs_shared_state_acquire_linked::<u8>(
                linked_provider_occurrence(),
                "test.linked-wrapper",
                "sha256:linked-wrapper",
                "LinkedWrapperState",
                "AfterLinkedProviderChildren",
            )
            .is_err(),
            "the linked occurrence tag must be removed after the child finishes"
        );
        drop(cx);
        assert_eq!(output, [61]);
    }

    #[tokio::test]
    async fn linked_provider_children_keep_the_tag_across_async_suspension() {
        let mut output = Vec::new();
        let mut cx = WebContext::with_buffer(&mut output);
        let _scope = cx
            .rsjs_shared_state_enter(
                "test.linked-wrapper-async",
                "sha256:linked-wrapper-async",
                "LinkedWrapperAsyncState",
                &[],
                std::rc::Rc::new(62u8),
                "{\"value\":62}".to_owned(),
            )
            .unwrap();

        WebRender::render(
            LinkedSharedStateProviderChildren::new(
                LinkedProviderAsyncProbe,
                LINKED_PROVIDER_PLAN_ID,
                LINKED_PROVIDER_HOST,
                LINKED_PROVIDER_SITE,
            ),
            &mut cx,
        )
        .resolve()
        .await
        .unwrap();
        drop(cx);
        assert_eq!(output, [62]);
    }

    #[test]
    fn server_site_is_byte_for_byte_and_publishes_no_asset() {
        let mut output = Vec::new();
        let mut cx = WebContext::with_buffer(&mut output);
        LinkedRenderBoundary::new(Bytes(b"<p>server</p>"), &SERVER_SITE, None, None, None)
            .render(&mut cx)
            .unwrap();
        assert!(cx.rsjs_islands_used().is_empty());
        drop(cx);
        assert_eq!(output, b"<p>server</p>");
    }

    #[test]
    fn marker_attributes_follow_the_exact_occurrence_owner() {
        let calls = core::cell::Cell::new(0_u32);
        let mut server_output = Vec::new();
        let mut server_cx = WebContext::with_buffer(&mut server_output);
        let server_attr = LinkedMarkerAttr::from_fn(&SERVER_SITE, || {
            calls.set(calls.get() + 1);
            "4"
        });
        assert!(
            !server_attr.is_boolean_attr(),
            "a disabled marker must not report boolean-attribute semantics"
        );
        server_attr
            .render_attr(
                b" data-rsjs-handler=\"",
                b" data-rsjs-handler",
                &mut server_cx,
            )
            .unwrap();
        server_attr.render_attr_value(&mut server_cx).unwrap();
        drop(server_cx);
        assert!(
            server_output.is_empty(),
            "a pure-SSR occurrence must not retain inert hydration lookup bytes"
        );
        assert_eq!(
            calls.get(),
            0,
            "a pure-SSR occurrence must not evaluate a marker payload factory"
        );

        let mut owner_output = Vec::new();
        let mut owner_cx = WebContext::with_buffer(&mut owner_output);
        LinkedMarkerAttr::from_fn(&ROOT_SITE, || {
            calls.set(calls.get() + 1);
            "4"
        })
        .render_attr(
            b" data-rsjs-handler=\"",
            b" data-rsjs-handler",
            &mut owner_cx,
        )
        .unwrap();
        drop(owner_cx);
        assert_eq!(owner_output, b" data-rsjs-handler=\"4\"");
        assert_eq!(calls.get(), 1);
    }

    #[test]
    fn owner_values_do_not_run_their_factory_for_pure_ssr() {
        let calls = core::cell::Cell::new(0_u32);
        let server_value = linked_owner_value(&SERVER_SITE, || {
            calls.set(calls.get() + 1);
            Ok::<_, WriteError>(String::from("server seed"))
        })
        .unwrap();
        assert!(server_value.is_none());
        assert_eq!(calls.get(), 0);

        let owner_value = linked_owner_value(&ROOT_SITE, || {
            calls.set(calls.get() + 1);
            Ok::<_, WriteError>(String::from("owner seed"))
        })
        .unwrap();
        assert_eq!(owner_value.as_deref(), Some("owner seed"));
        assert_eq!(calls.get(), 1);
    }

    #[test]
    fn text_binding_carrier_exists_only_for_owned_occurrences() {
        let mut server_output = Vec::new();
        let mut server_cx = WebContext::with_buffer(&mut server_output);
        WebRenderSync::render(
            LinkedTextBinding::new(proa_core::text::text("<server>"), &SERVER_SITE, "4"),
            &mut server_cx,
        )
        .unwrap();
        drop(server_cx);
        assert_eq!(
            server_output, b"&lt;server&gt;",
            "the inactive path must preserve ordinary text escaping without adding a carrier"
        );

        let mut owner_output = Vec::new();
        let mut owner_cx = WebContext::with_buffer(&mut owner_output);
        WebRenderSync::render(
            LinkedTextBinding::new(proa_core::text::text("<owner>"), &ROOT_SITE, "4"),
            &mut owner_cx,
        )
        .unwrap();
        drop(owner_cx);
        assert_eq!(
            owner_output,
            b"<span data-rsjs-bind=\"4\">&lt;owner&gt;</span>"
        );
    }

    #[test]
    fn owner_root_emits_seed_records_and_publishes_one_owner_asset() {
        let mut output = Vec::new();
        let mut cx = WebContext::with_buffer(&mut output);
        LinkedRenderBoundary::new(
            Bytes(b"<button>go</button>"),
            &ROOT_SITE,
            Some("{\"signals\":[1]}"),
            Some("{\"0\":{\"data\":\"ok\"}}"),
            Some("{}"),
        )
        .render(&mut cx)
        .unwrap();
        let assets = cx.rsjs_islands_used().iter().copied().collect::<Vec<_>>();
        drop(cx);
        assert_eq!(
            String::from_utf8(output).unwrap(),
            concat!(
                "<rsjs-island data-rsjs-island-root=\"rsjs_owner_test_3\" ",
                "data-rsjs-linked-candidate=\"7\"><button>go</button></rsjs-island>",
                "<script type=\"application/json\" data-rsjs-island=\"rsjs_owner_test_3\">",
                "{\"signals\":[1]}</script>",
                "<script type=\"application/json\" data-rsjs-island-queries=\"root_fragment\">",
                "{\"0\":{\"data\":\"ok\"}}</script>",
                "<script type=\"application/json\" data-rsjs-island-mutations=\"root_fragment\">",
                "{}</script>",
            )
        );
        assert_eq!(assets, vec!["rsjs_owner_test_3"]);
    }

    #[test]
    fn active_owner_requires_a_signal_seed() {
        let mut output = Vec::new();
        let mut cx = WebContext::with_buffer(&mut output);
        let error =
            LinkedRenderBoundary::new(Bytes(b"<button>go</button>"), &ROOT_SITE, None, None, None)
                .render(&mut cx)
                .expect_err("an active owner cannot publish without a signal seed");
        assert!(matches!(
            error,
            WriteError::InvalidAttribute("active RSJS linked site is missing its signal seed")
        ));
        drop(cx);
        assert!(
            output.is_empty(),
            "missing owner seed metadata must fail before wrapper or child bytes are committed"
        );
    }

    #[test]
    fn fused_member_emits_its_own_seed_without_republishing_owner_asset() {
        let mut output = Vec::new();
        let mut cx = WebContext::with_buffer(&mut output);
        LinkedRenderBoundary::new(
            Bytes(b"<span>member</span>"),
            &MEMBER_SITE,
            Some("{\"signals\":[]}"),
            None,
            None,
        )
        .render(&mut cx)
        .unwrap();
        assert!(cx.rsjs_islands_used().is_empty());
        drop(cx);
        assert_eq!(
            String::from_utf8(output).unwrap(),
            concat!(
                "<rsjs-island data-rsjs-island-root=\"rsjs_member_test_12\" ",
                "data-rsjs-linked-candidate=\"12\"><span>member</span></rsjs-island>",
                "<script type=\"application/json\" data-rsjs-island=\"rsjs_member_test_12\">",
                "{\"signals\":[]}</script>",
            )
        );
    }
}
