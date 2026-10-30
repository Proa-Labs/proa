//! Managed readiness is independent of body byte availability.
use crate::{FrameworkError, FrameworkLocalStream, FrameworkSendStream, FrameworkStreamFactory};
use bytes::Bytes;
use futures::{Stream, StreamExt};
use proa_core::ResponseContext;
use std::{
    pin::Pin,
    sync::{Arc, OnceLock},
};

pub enum ManagedEvent {
    Ready {
        response: ResponseContext,
        prefix: Vec<Bytes>,
    },
    Chunk(Bytes),
}
pub type LocalEvents = Pin<Box<dyn Stream<Item = Result<ManagedEvent, FrameworkError>>>>;
pub type SendEvents = Pin<Box<dyn Stream<Item = Result<ManagedEvent, FrameworkError>> + Send>>;
pub enum ManagedFactory {
    Local(Box<dyn FnOnce() -> LocalEvents + Send>),
    Send(Box<dyn FnOnce() -> SendEvents + Send>),
}
impl std::fmt::Debug for ManagedFactory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ManagedFactory")
    }
}
pub type PreparedHead = Arc<OnceLock<ResponseContext>>;
fn bytes<S>(events: S, head: PreparedHead) -> impl Stream<Item = Result<Bytes, FrameworkError>>
where
    S: Stream<Item = Result<ManagedEvent, FrameworkError>>,
{
    async_stream::try_stream! {
        futures::pin_mut!(events);
        let first=events.next().await.ok_or(FrameworkError::Invariant("missing managed Ready event"))??;
        let ManagedEvent::Ready{response,prefix}=first else {
            Err(FrameworkError::Invariant("managed bytes before Ready"))?;unreachable!()
        };
        head.set(response).map_err(|_|FrameworkError::Invariant("duplicate prepared head"))?;
        if prefix.iter().all(Bytes::is_empty) {
            // Expose readiness to the adapter without inventing a body frame or
            // polling a body that may wait forever. Only the publication edge
            // suspends once; native responses do not use this adapter.
            let mut suspended = false;
            futures::future::poll_fn(move |cx| {
                if suspended { std::task::Poll::Ready(()) } else {
                    suspended = true;
                    cx.waker().wake_by_ref();
                    std::task::Poll::Pending
                }
            }).await;
        }
        // Yield real pages before polling any subsequent render work. No
        // synthetic bytes are used to carry status or control signals.
        for bytes in prefix {if !bytes.is_empty(){yield bytes;}}
        while let Some(event)=events.next().await{
            match event? {
                ManagedEvent::Chunk(bytes)=>yield bytes,
                ManagedEvent::Ready{..}=>Err(FrameworkError::Invariant("duplicate managed Ready event"))?,
            }
        }
    }
}
pub fn adapt(factory: ManagedFactory) -> (FrameworkStreamFactory, PreparedHead) {
    let head = Arc::new(OnceLock::new());
    let output = head.clone();
    let factory = match factory {
        ManagedFactory::Local(make) => FrameworkStreamFactory::Local(Box::new(move || {
            Box::pin(bytes(make(), output)) as FrameworkLocalStream
        })),
        ManagedFactory::Send(make) => FrameworkStreamFactory::Send(Box::new(move || {
            Box::pin(bytes(make(), output)) as FrameworkSendStream
        })),
    };
    (factory, head)
}

/// Wait for the first payload or explicit managed readiness. `None` can mean
/// an empty Ready prefix, so callers must retain and continue polling the tail.
pub async fn first_or_ready<S>(
    stream: &mut S,
    head: Option<&PreparedHead>,
) -> Option<Result<Bytes, FrameworkError>>
where
    S: Stream<Item = Result<Bytes, FrameworkError>> + Unpin + ?Sized,
{
    futures::future::poll_fn(|cx| match Pin::new(&mut *stream).poll_next(cx) {
        std::task::Poll::Pending if head.is_some_and(|h| h.get().is_some()) => {
            std::task::Poll::Ready(None)
        }
        other => other,
    })
    .await
}

/// Per-response managed configuration. A publisher is installed only by the
/// response driver; application code supplies ordinary defaults/configuration.
#[derive(Clone)]
pub struct ManagedRequest {
    pub config: proa_core::HeadConfig,
    publisher: Option<proa_core::managed_head::Publisher>,
}
impl std::fmt::Debug for ManagedRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ManagedRequest")
            .field("config", &self.config)
            .finish()
    }
}
impl From<proa_core::HeadConfig> for ManagedRequest {
    fn from(config: proa_core::HeadConfig) -> Self {
        Self {
            config,
            publisher: None,
        }
    }
}

// One pump for both native delivery drivers. The existing OOO driver still
// controls boundary/tail ordering; its root can now publish while it is pending.
fn pump<S>(
    source: S,
    mut ready: futures::channel::mpsc::UnboundedReceiver<Result<ManagedEvent, FrameworkError>>,
) -> impl Stream<Item = Result<ManagedEvent, FrameworkError>>
where
    S: Stream<Item = Result<Bytes, FrameworkError>> + Unpin,
{
    let mut source = source;
    let mut deferred = None;
    futures::stream::poll_fn(move |cx| {
        use std::task::Poll;
        if let Poll::Ready(Some(event)) = Pin::new(&mut ready).poll_next(cx) {
            return Poll::Ready(Some(event));
        }
        if let Some(item) = deferred.take() {
            return Poll::Ready(item);
        }
        let item = match Pin::new(&mut source).poll_next(cx) {
            Poll::Pending => {
                return match Pin::new(&mut ready).poll_next(cx) {
                    Poll::Ready(Some(event)) => Poll::Ready(Some(event)),
                    _ => Poll::Pending,
                }
            }
            Poll::Ready(item) => item.map(|r| r.map(ManagedEvent::Chunk)),
        };
        // A single source poll may publish Ready and then fail. Deliver Ready
        // first so a later error terminates the committed body, not its status.
        if let Poll::Ready(Some(event)) = Pin::new(&mut ready).poll_next(cx) {
            deferred = Some(item);
            Poll::Ready(Some(event))
        } else {
            Poll::Ready(item)
        }
    })
}

pub(crate) fn stream_factory(
    mut request: crate::FrameworkRequestContext,
    make: impl FnOnce(crate::FrameworkRequestContext) -> crate::FrameworkStreamFactory + Send + 'static,
) -> ManagedFactory {
    let (tx, rx) = futures::channel::mpsc::unbounded();
    request
        .head
        .as_mut()
        .expect("managed configuration")
        .publisher = Some(Arc::new(move |response, prefix| {
        tx.unbounded_send(Ok(ManagedEvent::Ready { response, prefix }))
            .map_err(|_| proa_core::WriteError::StreamClosed)
    }));
    match make(request) {
        crate::FrameworkStreamFactory::Local(make) => {
            ManagedFactory::Local(Box::new(move || Box::pin(pump(make(), rx))))
        }
        crate::FrameworkStreamFactory::Send(make) => {
            ManagedFactory::Send(Box::new(move || Box::pin(pump(make(), rx))))
        }
    }
}

#[derive(Default)]
pub(crate) struct BufferedHead(Arc<std::sync::Mutex<Vec<Bytes>>>);
impl BufferedHead {
    pub fn finish(self, tail: proa_core::RenderBuffer) -> Result<Vec<u8>, proa_core::WriteError> {
        use proa_core::WriteBuf;
        let mut prefix = self.0.lock().unwrap();
        if prefix.is_empty() {
            return Ok(tail.into_vec());
        }
        // Buffered finalizers already require one contiguous output. Copy each
        // retained page once, directly into that output, without rebuilding a
        // second RenderBuffer that would immediately be flattened again.
        let capacity = prefix.iter().try_fold(tail.len(), |len, page| {
            len.checked_add(page.len())
                .ok_or(proa_core::WriteError::OutOfCapacity)
        })?;
        let mut out = Vec::with_capacity(capacity);
        for page in prefix.drain(..).chain(tail.into_pages()) {
            out.extend_from_slice(&page);
        }
        Ok(out)
    }
}
macro_rules! setup {
    ($name:ident, $context:ident $(, $send:ident)?) => {
        pub(crate) fn $name<L: proa_core::DataLoader, B: proa_core::WriteBuf $(+ $send)?>(
            cx: &mut proa_core::$context<L, B>, request: &crate::FrameworkRequestContext,
        ) -> Result<Option<BufferedHead>, proa_core::WriteError> {
            let Some(head) = &request.head else { return Ok(None) };
            if let Some(publisher) = &head.publisher {
                cx.configure_head(head.config.clone(), publisher.clone())?;
                Ok(None)
            } else {
                let held = BufferedHead::default();
                let queue = held.0.clone();
                cx.configure_head(head.config.clone(), Arc::new(move |_, prefix| { *queue.lock().unwrap() = prefix; Ok(()) }))?;
                Ok(Some(held))
            }
        }
    }
}
setup!(setup_local, WebContext);
setup!(setup_send, SendWebContext, Send);
