// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Request-local document preparation and ordered native-buffer publication.
use crate::{ResponseContext, WriteError};
use bytes::Bytes;
use std::sync::{Arc, Mutex};

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum HeadError {
    #[error("managed head requires HeadConfig before rendering")]
    ManualMode,
    #[error("head mutations belong in render_head, not render")]
    OutsideHook,
    #[error("the Page has sealed the document head")]
    Closed,
    #[error("head work is excluded inside Suspense, captures and fallbacks")]
    Excluded,
    #[error("head hooks cannot recursively dispatch another head hook")]
    NestedHook,
    #[error("document preparation was cancelled or left incomplete")]
    Cancelled,
    #[error("head preparation failed")]
    HookFailed,
    #[error("a head provider failed while rendering its body")]
    ProviderFailed,
    #[error("a successful head provider must contain the selected Page")]
    OrphanProvider,
    #[error("managed document did not select a Page")]
    MissingPage,
    #[error("managed document did not render cx.head()")]
    MissingOutlet,
    #[error("managed document rendered more than one head outlet")]
    DuplicateOutlet,
    #[error("head outlet belongs to a different document")]
    ForeignOutlet,
    #[error("error recovery cannot rewind across head preparation or publication")]
    RecoveryCrossedLifecycle,
    #[error("managed head requires a writer with owned page transfer")]
    UnsupportedWriter,
    #[error("managed head preparation exceeded its configured limit")]
    RetainedLimit,
    #[error("managed head insertion encountered invalid retained output")]
    InvalidOutput,
    #[error("managed head publication failed")]
    PublisherFailed,
    #[error("managed document state cannot be replaced after initialization")]
    StateReplaced,
    #[error("use checked head setters in a managed document")]
    LegacyMutation,
    #[error("the CSP nonce must be installed before managed head preparation begins")]
    NonceAfterInitialization,
}
fn error(e: HeadError) -> WriteError {
    WriteError::HeadLifecycle(e)
}

/// Give the transport one poll to observe a newly published head before
/// continuing synchronous body work in the same async render.
pub(crate) async fn yield_after_publication() {
    let mut yielded = false;
    std::future::poll_fn(move |cx| {
        if yielded {
            std::task::Poll::Ready(())
        } else {
            yielded = true;
            cx.waker().wake_by_ref();
            std::task::Poll::Pending
        }
    })
    .await
}

pub(crate) fn streaming(mode: crate::RenderMode) -> bool {
    matches!(
        mode,
        crate::RenderMode::ChunkedInOrder | crate::RenderMode::ChunkedOutOfOrder
    )
}

/// Application configuration shared by every request that enables managed head rendering.
#[derive(Clone, Debug)]
pub struct HeadConfig {
    pub preparation_limit: usize,
    pub urls: Option<Arc<crate::HeadUrlPolicy>>,
    pub defaults: HeadDefaults,
}
impl Default for HeadConfig {
    fn default() -> Self {
        Self {
            preparation_limit: 1024 * 1024,
            urls: None,
            defaults: HeadDefaults::default(),
        }
    }
}
/// Reusable immutable defaults. Strings and resource keys share storage across requests.
#[derive(Clone, Default)]
pub struct HeadDefaults(Arc<crate::head_fields::HeadFields>);
impl std::fmt::Debug for HeadDefaults {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("HeadDefaults")
    }
}
impl HeadConfig {
    /// Configure the trusted origin and mount. Call this before
    /// [`HeadConfig::with_defaults`]: defaults resolve their URLs when they are
    /// configured. Requests share the policy; it is not cloned per request.
    pub fn with_urls(mut self, urls: crate::HeadUrlPolicy) -> Self {
        self.urls = Some(Arc::new(urls));
        self
    }
    pub fn with_preparation_limit(mut self, bytes: usize) -> Self {
        self.preparation_limit = bytes;
        self
    }
    /// Prepare immutable app defaults once, using the same checked setters as hooks.
    ///
    /// Resource and canonical defaults are resolved against the policy from
    /// [`HeadConfig::with_urls`] at this call, so configure URLs first; without
    /// a policy those setters fail with `missing trusted URL policy`.
    pub fn with_defaults(
        mut self,
        configure: impl FnOnce(&mut crate::web::HeadWriter<'_>) -> Result<(), WriteError>,
    ) -> Result<Self, WriteError> {
        let document = ManagedHead::new(self.preparation_limit, Arc::new(|_, _| Ok(())));
        {
            let mut state = document.0.lock().unwrap();
            state.fields = self.defaults.0.clone();
            state.url_policy = self.urls.clone();
        }
        let guard = document.begin_hook_for("application defaults")?;
        let mut head = crate::HeadContext::new();
        head.manage(document.clone());
        let scope = guard.finish(configure(&mut crate::web::HeadWriter::new(&mut head)))?;
        document.seal()?;
        scope.finish(Ok(()))?;
        self.defaults = HeadDefaults(document.0.lock().unwrap().fields.clone());
        Ok(self)
    }
}
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{component} {phase} failed: {cause}")]
pub struct HeadFailure {
    pub component: &'static str,
    pub phase: &'static str,
    pub kind: HeadError,
    #[source]
    pub cause: WriteError,
}
pub type Publisher =
    Arc<dyn Fn(ResponseContext, Vec<Bytes>) -> Result<(), WriteError> + Send + Sync>;
struct State {
    failed: Option<WriteError>,
    hook: bool,
    sealed: bool,
    published: bool,
    excluded: usize,
    outlet: Option<usize>,
    fields: Arc<crate::head_fields::HeadFields>,
    url_policy: Option<Arc<crate::head_fields::HeadUrlPolicy>>,
    limit: usize,
    publisher: Publisher,
    revision: usize,
    recovery: std::collections::BTreeMap<u64, usize>,
}
#[derive(Clone)]
pub struct ManagedHead(Arc<Mutex<State>>, bool);
impl std::fmt::Debug for ManagedHead {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ManagedHead")
    }
}
impl State {
    fn check(&self) -> Result<(), WriteError> {
        self.failed.clone().map_or(Ok(()), Err)
    }
    fn fail_cause<T>(
        &mut self,
        component: &'static str,
        phase: &'static str,
        cause: WriteError,
    ) -> Result<T, WriteError> {
        let first = self.failed.get_or_insert_with(|| {
            // proa-perf-ok: diagnostics allocate only on a failed managed render
            WriteError::HeadFailure(Arc::new(HeadFailure {
                component,
                phase,
                kind: match phase {
                    "render_head" => HeadError::HookFailed,
                    "render" => HeadError::ProviderFailed,
                    "publication" => HeadError::PublisherFailed,
                    _ => HeadError::RecoveryCrossedLifecycle,
                },
                cause,
            }))
        });
        Err(first.clone())
    }
    fn fail<T>(&mut self, e: HeadError) -> Result<T, WriteError> {
        let first = self.failed.get_or_insert_with(|| error(e));
        Err(first.clone())
    }
}
impl ManagedHead {
    pub fn new(limit: usize, publisher: Publisher) -> Self {
        Self(
            Arc::new(Mutex::new(State {
                failed: None,
                hook: false,
                sealed: false,
                published: false,
                excluded: 0,
                outlet: None,
                fields: Default::default(),
                url_policy: None,
                limit,
                publisher,
                revision: 0,
                recovery: Default::default(),
            })),
            false,
        )
    }
    pub fn reject_component<T>(
        &self,
        component: &'static str,
        phase: &'static str,
        kind: HeadError,
    ) -> Result<T, WriteError> {
        let mut state = self.0.lock().unwrap();
        let failure = state.failed.get_or_insert_with(|| {
            WriteError::HeadFailure(Arc::new(HeadFailure {
                component,
                phase,
                kind,
                cause: error(kind),
            }))
        });
        Err(failure.clone())
    }
    pub fn reject_replacement(&self) {
        let _ = self.fail::<()>(HeadError::StateReplaced);
    }
    pub fn revision(&self) -> usize {
        self.0.lock().unwrap().revision
    }
    pub fn detached(&self) -> Self {
        Self(self.0.clone(), true)
    }
    pub fn begin_hook(&self) -> Result<HookGuard, WriteError> {
        self.begin_hook_for("head provider")
    }
    /// A Page without a head hook still owns preparation until its body completes.
    /// Create the guard before sealing or constructing the body, so dropping an
    /// unpolled continuation or unwinding cannot leave a reusable document.
    /// Hooked providers get the same property from the `HookGuard` that
    /// dispatch claims before it returns a pending outcome.
    pub(crate) fn track_page(&self, component: &'static str) -> ProviderGuard {
        ProviderGuard {
            head: self.clone(),
            component,
            done: false,
        }
    }
    pub fn begin_hook_for(&self, component: &'static str) -> Result<HookGuard, WriteError> {
        let mut s = self.0.lock().unwrap();
        s.check()?;
        if self.1 || s.excluded != 0 {
            return s.fail(HeadError::Excluded);
        }
        if s.sealed {
            return s.fail(HeadError::Closed);
        }
        if s.hook {
            return s.fail(HeadError::NestedHook);
        }
        s.revision += 1;
        s.hook = true;
        Arc::make_mut(&mut s.fields).begin_hook();
        Ok(HookGuard {
            head: self.clone(),
            component,
            done: false,
        })
    }
    pub fn title(&self, title: std::borrow::Cow<'static, str>) -> Result<(), WriteError> {
        let mut s = self.0.lock().unwrap();
        s.check()?;
        if self.1 || s.excluded != 0 {
            return s.fail(HeadError::Excluded);
        }
        if s.sealed {
            return s.fail(HeadError::Closed);
        }
        if !s.hook {
            return s.fail(HeadError::OutsideHook);
        }
        Arc::make_mut(&mut s.fields).set_title(Some(title), false)
    }
    pub fn check_initialization(&self) -> Result<(), WriteError> {
        let mut s = self.0.lock().unwrap();
        s.check()?;
        if s.revision != 0 || self.1 || s.excluded != 0 {
            return s.fail(HeadError::StateReplaced);
        }
        Ok(())
    }
    /// The nonce is read at publication, so it is fixed once preparation begins.
    pub fn reject_late_nonce(&self) -> Result<(), WriteError> {
        let mut s = self.0.lock().unwrap();
        s.check()?;
        if s.revision != 0 || self.1 || s.excluded != 0 {
            return s.fail(HeadError::NonceAfterInitialization);
        }
        Ok(())
    }
    pub fn configure_urls(
        &self,
        policy: impl Into<Arc<crate::head_fields::HeadUrlPolicy>>,
    ) -> Result<(), WriteError> {
        let mut s = self.0.lock().unwrap();
        s.check()?;
        if s.revision != 0 || self.1 || s.excluded != 0 {
            return s.fail(HeadError::StateReplaced);
        }
        s.url_policy = Some(policy.into());
        Ok(())
    }
    fn mutate(
        &self,
        operation: impl FnOnce(&mut State) -> Result<(), WriteError>,
    ) -> Result<(), WriteError> {
        let mut s = self.0.lock().unwrap();
        s.check()?;
        if self.1 || s.excluded != 0 {
            return s.fail(HeadError::Excluded);
        }
        if s.sealed {
            return s.fail(HeadError::Closed);
        }
        if !s.hook {
            return s.fail(HeadError::OutsideHook);
        }
        operation(&mut s)
    }
    pub fn meta(
        &self,
        name: &str,
        value: Option<std::borrow::Cow<'static, str>>,
    ) -> Result<(), WriteError> {
        self.mutate(|s| Arc::make_mut(&mut s.fields).meta(name, value))
    }
    pub fn canonical(&self, value: &str) -> Result<(), WriteError> {
        self.mutate(|s| {
            let policy = s
                .url_policy
                .as_ref()
                .ok_or_else(|| WriteError::render("missing trusted URL policy"))?;
            Arc::make_mut(&mut s.fields).canonical(policy, value)
        })
    }
    pub fn resource<K: Eq + Send + Sync + 'static>(
        &self,
        family: crate::head_fields::ResourceFamily,
        key: K,
        value: Option<&str>,
    ) -> Result<(), WriteError> {
        self.mutate(|s| {
            let policy = s
                .url_policy
                .as_ref()
                .ok_or_else(|| WriteError::render("missing trusted URL policy"))?;
            Arc::make_mut(&mut s.fields).resource(policy, family, key, value)
        })
    }
    pub fn absolute_title(&self, value: std::borrow::Cow<'static, str>) -> Result<(), WriteError> {
        self.mutate(|s| Arc::make_mut(&mut s.fields).set_title(Some(value), true))
    }
    pub fn clear_title(&self) -> Result<(), WriteError> {
        self.mutate(|s| Arc::make_mut(&mut s.fields).set_title(None, false))
    }
    pub fn title_template(
        &self,
        value: Option<std::borrow::Cow<'static, str>>,
    ) -> Result<(), WriteError> {
        self.mutate(|s| Arc::make_mut(&mut s.fields).set_template(value))
    }
    pub fn clear_canonical(&self) -> Result<(), WriteError> {
        self.mutate(|s| {
            Arc::make_mut(&mut s.fields).clear_canonical();
            Ok(())
        })
    }
    pub fn open_graph(&self, patch: crate::head_fields::OpenGraph) -> Result<(), WriteError> {
        self.mutate(|s| Arc::make_mut(&mut s.fields).graph(patch, s.url_policy.as_deref()))
    }
    pub fn seal(&self) -> Result<(), WriteError> {
        let mut s = self.0.lock().unwrap();
        s.check()?;
        if self.1 || s.excluded != 0 {
            return s.fail(HeadError::Excluded);
        }
        if s.sealed {
            return s.fail(HeadError::Closed);
        }
        if s.hook {
            return s.fail(HeadError::NestedHook);
        }
        s.revision += 1;
        s.sealed = true;
        Ok(())
    }
    pub fn outlet(&self) -> HeadOutlet {
        HeadOutlet(Some(self.clone()))
    }
    pub fn reserve(&self, outlet: HeadOutlet, position: usize) -> Result<(), WriteError> {
        let mut s = self.0.lock().unwrap();
        s.check()?;
        if !Arc::ptr_eq(&self.0, &outlet.head_state()?.0) {
            return s.fail(HeadError::ForeignOutlet);
        }
        if self.1 || s.excluded != 0 || s.hook {
            return s.fail(HeadError::Excluded);
        }
        if s.outlet.is_some() {
            return s.fail(HeadError::DuplicateOutlet);
        }
        s.revision += 1;
        s.outlet = Some(position);
        Ok(())
    }
    pub fn should_publish(&self) -> Result<bool, WriteError> {
        let s = self.0.lock().unwrap();
        s.check()?;
        Ok(s.sealed && s.outlet.is_some() && !s.published)
    }
    pub fn fail<T>(&self, e: HeadError) -> Result<T, WriteError> {
        self.0.lock().unwrap().fail(e)
    }
    pub fn is_published(&self) -> bool {
        self.0.lock().unwrap().published
    }
    /// Read the final title for client-navigation payloads after publication.
    pub fn resolved_title(&self) -> Option<String> {
        self.0.lock().unwrap().fields.resolved_title()
    }
    /// Check sticky failure after deferred work, without requiring root completion.
    pub fn check_terminal(&self) -> Result<(), WriteError> {
        self.0.lock().unwrap().check()
    }
    pub fn final_check(&self) -> Result<(), WriteError> {
        let mut s = self.0.lock().unwrap();
        s.check()?;
        if s.excluded != 0 || !s.recovery.is_empty() {
            return s.fail(HeadError::Cancelled);
        }
        if !s.sealed {
            return s.fail(HeadError::MissingPage);
        }
        if s.outlet.is_none() {
            return s.fail(HeadError::MissingOutlet);
        }
        Ok(())
    }
    /// Checkpoint bookkeeping lives only in managed state, not native futures.
    pub fn begin_recovery(&self, id: u64) -> Result<(), WriteError> {
        if self.1 {
            return self.0.lock().unwrap().check();
        }
        let mut s = self.0.lock().unwrap();
        s.check()?;
        let revision = s.revision;
        if s.recovery.len() >= 1024 || s.recovery.insert(id, revision).is_some() {
            return s.fail(HeadError::RetainedLimit);
        }
        Ok(())
    }
    pub fn finish_recovery(
        &self,
        id: u64,
        result: &Result<(), WriteError>,
    ) -> Result<(), WriteError> {
        if self.1 {
            return self.0.lock().unwrap().check();
        }
        let mut s = self.0.lock().unwrap();
        let revision = s.recovery.remove(&id);
        s.check()?;
        if let Err(cause) = result {
            if revision != Some(s.revision) {
                return s.fail_cause(
                    "ErrorBoundary",
                    "recovery across head lifecycle",
                    cause.clone(),
                );
            }
        }
        Ok(())
    }
    /// In-place exclusion for an async boundary. Cancellation deliberately
    /// leaves the document excluded; finalization rejects that abandoned work.
    /// This uses no extra guard storage in the native boundary future.
    pub fn enter_boundary(&self) {
        if !self.1 {
            self.0.lock().unwrap().excluded += 1;
        }
    }
    pub fn leave_boundary(&self) {
        if !self.1 {
            self.0.lock().unwrap().excluded -= 1;
        }
    }
    pub fn exclude(&self) -> ExcludedGuard {
        self.enter_boundary();
        ExcludedGuard(self.clone())
    }
    pub fn limit_check(&self, len: usize) -> Result<(), WriteError> {
        let mut s = self.0.lock().unwrap();
        s.check()?;
        if !s.published && len > s.limit {
            return s.fail(HeadError::RetainedLimit);
        }
        Ok(())
    }
    pub fn publish(
        &self,
        pages: Vec<Bytes>,
        response: ResponseContext,
        nonce: Option<&crate::CspNonce>,
    ) -> Result<(), WriteError> {
        let (offset, fields, publisher, limit) = {
            let mut s = self.0.lock().unwrap();
            s.check()?;
            if !s.sealed || s.outlet.is_none() || s.published {
                return s.fail(HeadError::Closed);
            }
            let len = pages.iter().map(Bytes::len).sum::<usize>();
            if len > s.limit {
                return s.fail(HeadError::RetainedLimit);
            }
            // Latch before serialization: failures never reopen this document.
            s.published = true;
            (
                s.outlet.unwrap(),
                s.fields.clone(),
                s.publisher.clone(),
                s.limit,
            )
        };
        let mut encoded = crate::RenderBuffer::new();
        let meter = crate::OutputMeter::new(crate::head_fields::HEAD_LIMIT as u64);
        let mut bounded = crate::MeteredWriteBuf::new(&mut encoded, meter.borrow());
        if fields.write(&mut bounded, nonce).is_err() {
            return self.fail(HeadError::RetainedLimit);
        }
        let mut head = Some(encoded.into_pages().collect::<Vec<_>>());
        let mut chunks = Vec::new();
        let mut left = offset;
        for mut page in pages {
            if head.is_some() && left <= page.len() {
                if left > 0 {
                    chunks.push(page.split_to(left));
                }
                chunks.extend(head.take().unwrap());
                left = 0;
                if !page.is_empty() {
                    chunks.push(page);
                }
            } else {
                left = left.saturating_sub(page.len());
                chunks.push(page);
            }
        }
        if left != 0 {
            return self.fail(HeadError::InvalidOutput);
        }
        if let Some(head) = head {
            chunks.extend(head)
        }
        if chunks.iter().map(Bytes::len).sum::<usize>() > limit {
            return self.fail(HeadError::RetainedLimit);
        }
        let mut utf8 = Utf8::default();
        for part in &chunks {
            if utf8.push(part).is_err() {
                return self.fail(HeadError::InvalidOutput);
            }
        }
        if utf8.finish().is_err() {
            return self.fail(HeadError::InvalidOutput);
        }
        if let Err(cause) = publisher(response, chunks) {
            return self
                .0
                .lock()
                .unwrap()
                .fail_cause("document publisher", "publication", cause);
        }
        Ok(())
    }
}
// The handle is owned, contains no cx borrow, and does not record a position
// until it is rendered. Cloning is deliberately not provided.
pub struct HeadOutlet(Option<ManagedHead>);
impl HeadOutlet {
    pub(crate) fn head_state(&self) -> Result<ManagedHead, WriteError> {
        self.0.clone().ok_or(error(HeadError::ManualMode))
    }
}
pub struct HookGuard {
    head: ManagedHead,
    component: &'static str,
    done: bool,
}
impl HookGuard {
    pub fn finish(mut self, result: Result<(), WriteError>) -> Result<ProviderGuard, WriteError> {
        self.done = true;
        let mut s = self.head.0.lock().unwrap();
        s.hook = false;
        if let Err(cause) = result {
            return s.fail_cause(self.component, "render_head", cause);
        }
        s.check()?;
        Ok(ProviderGuard {
            head: self.head.clone(),
            component: self.component,
            done: false,
        })
    }
}
impl Drop for HookGuard {
    fn drop(&mut self) {
        if !self.done {
            let mut s = self.head.0.lock().unwrap();
            s.hook = false;
            s.failed.get_or_insert_with(|| error(HeadError::Cancelled));
        }
    }
}
pub struct ProviderGuard {
    head: ManagedHead,
    component: &'static str,
    done: bool,
}
impl ProviderGuard {
    pub fn finish(mut self, result: Result<(), WriteError>) -> Result<(), WriteError> {
        self.done = true;
        let mut s = self.head.0.lock().unwrap();
        s.check()?;
        if let Err(cause) = result {
            return s.fail_cause(self.component, "render", cause);
        }
        if !s.sealed {
            return s.fail(HeadError::OrphanProvider);
        }
        Ok(())
    }
}
impl Drop for ProviderGuard {
    fn drop(&mut self) {
        if !self.done {
            self.head
                .0
                .lock()
                .unwrap()
                .failed
                .get_or_insert_with(|| error(HeadError::Cancelled));
        }
    }
}
pub struct ExcludedGuard(ManagedHead);
impl Drop for ExcludedGuard {
    fn drop(&mut self) {
        self.0.leave_boundary();
    }
}

macro_rules! context_lifecycle {
    ($context:ident $(,$send:ident)?)=>{
        impl<L:crate::DataLoader,B:crate::WriteBuf $(+$send)?> crate::$context<L,B>{
            /// Create an owned insertion command. Only rendering it reserves its position.
            pub fn head(&self) -> HeadOutlet { HeadOutlet(self.head_state().document()) }
            #[doc(hidden)]
            pub fn require_managed_head(&self) -> Result<ManagedHead, WriteError> {
                self.head_state().document().ok_or(error(HeadError::ManualMode))
            }
            /// Enable managed document rendering before invoking any components.
            pub fn configure_head(&mut self, config: HeadConfig, publisher: Publisher) -> Result<(), WriteError> {
                if !B::__head_supports_pages() { return Err(error(HeadError::UnsupportedWriter)); }
                let doc = ManagedHead::new(config.preparation_limit, publisher);
                doc.0.lock().unwrap().fields = config.defaults.0;
                if let Some(policy) = config.urls { doc.configure_urls(policy)?; }
                self.head_mut().manage(doc);
                self.require_managed_head()?.check_initialization()
            }

            pub fn managed_seal(&mut self)->Result<(),WriteError>{
                if let Some(d)=self.head_state().document(){d.seal()?;}
                self.managed_publish()
            }
            pub fn managed_publish(&mut self)->Result<(),WriteError>{
                let Some(d)=self.head_state().document() else{return Ok(())};
                d.limit_check(self.output().len())?;
                if d.should_publish()?{
                    let pages=match self.output_mut().__head_take_pages(){
                        Ok(p)=>p,Err(_)=>return d.fail(HeadError::UnsupportedWriter),
                    };
                    let response=self.response().clone();
                    self.response_mut().mark_stream_chunk_flushed();
                    d.publish(pages,response,self.csp_nonce())?;
                }Ok(())
            }
        }
    }
}
context_lifecycle!(WebContext);
context_lifecycle!(SendWebContext, Send);
impl<L: crate::DataLoader, B: crate::WriteBuf> crate::WebRenderSync<L, B> for HeadOutlet {
    fn render(self, cx: &mut crate::WebContext<L, B>) -> Result<(), WriteError> {
        let own = self.head_state()?;
        let Some(actual) = cx.head_state().document() else {
            return own.fail(HeadError::ForeignOutlet);
        };
        actual.reserve(self, cx.output().len())?;
        cx.managed_publish()
    }
}
impl<L: crate::DataLoader, B: crate::WriteBuf> crate::WebRender<L, B> for HeadOutlet {
    fn render(
        self,
        cx: &mut crate::WebContext<L, B>,
    ) -> crate::RenderOutcome<impl std::future::Future<Output = Result<(), WriteError>>> {
        let was_published = cx.head_state().document().is_some_and(|d| d.is_published());
        let result = crate::WebRenderSync::render(self, cx);
        let newly_published = !was_published
            && result.is_ok()
            && streaming(cx.render_mode())
            && cx.head_state().document().is_some_and(|d| d.is_published());
        if newly_published {
            crate::RenderOutcome::pending(async {
                yield_after_publication().await;
                Ok(())
            })
        } else {
            crate::RenderOutcome::Done(result)
        }
    }
}
impl<L: crate::DataLoader, B: crate::WriteBuf + Send> crate::WebRenderSendSync<L, B>
    for HeadOutlet
{
    fn render_send_sync(self, cx: &mut crate::SendWebContext<L, B>) -> Result<(), WriteError> {
        let own = self.head_state()?;
        let Some(actual) = cx.head_state().document() else {
            return own.fail(HeadError::ForeignOutlet);
        };
        actual.reserve(self, cx.output().len())?;
        cx.managed_publish()
    }
}
impl<L: crate::DataLoader, B: crate::WriteBuf + Send> crate::WebRenderSend<L, B> for HeadOutlet {
    fn render_send(
        self,
        cx: &mut crate::SendWebContext<L, B>,
    ) -> crate::RenderOutcome<impl std::future::Future<Output = Result<(), WriteError>> + Send>
    {
        let was_published = cx.head_state().document().is_some_and(|d| d.is_published());
        let result = crate::WebRenderSendSync::render_send_sync(self, cx);
        let newly_published = !was_published
            && result.is_ok()
            && streaming(cx.render_mode())
            && cx.head_state().document().is_some_and(|d| d.is_published());
        if newly_published {
            crate::RenderOutcome::pending(async {
                yield_after_publication().await;
                Ok(())
            })
        } else {
            crate::RenderOutcome::Done(result)
        }
    }
}

#[derive(Default)]
struct Utf8 {
    pending: [u8; 4],
    len: usize,
}
impl Utf8 {
    fn push(&mut self, mut bytes: &[u8]) -> Result<(), HeadError> {
        while self.len > 0 && !bytes.is_empty() {
            if self.len == 4 {
                return Err(HeadError::InvalidOutput);
            }
            self.pending[self.len] = bytes[0];
            self.len += 1;
            bytes = &bytes[1..];
            match std::str::from_utf8(&self.pending[..self.len]) {
                Ok(_) => self.len = 0,
                Err(e) if e.error_len().is_none() => {}
                Err(_) => return Err(HeadError::InvalidOutput),
            }
        }
        if self.len > 0 {
            return Ok(());
        }
        match std::str::from_utf8(bytes) {
            Ok(_) => Ok(()),
            Err(e) if e.error_len().is_none() => {
                let tail = &bytes[e.valid_up_to()..];
                self.pending[..tail.len()].copy_from_slice(tail);
                self.len = tail.len();
                Ok(())
            }
            Err(_) => Err(HeadError::InvalidOutput),
        }
    }
    fn finish(self) -> Result<(), HeadError> {
        if self.len == 0 {
            Ok(())
        } else {
            Err(HeadError::InvalidOutput)
        }
    }
}
