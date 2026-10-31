// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Default head hooks obey the same role contract on every rendering surface.
use proa_core::{
    head_dispatch, managed_head::ExcludedGuard, to_async, to_send, ComponentRole, HeadConfig,
    HeadError, RenderOutcome, SendWebContext, WebContext, WebRender, WebRenderSend,
    WebRenderSendSync, WebRenderSync, WriteBuf, WriteError,
};
use std::{
    future::{Future, Ready},
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
};

struct Page;
impl WebRenderSync for Page {
    const ROLE: ComponentRole = ComponentRole::PAGE;
    fn render(self, cx: &mut WebContext) -> Result<(), WriteError> {
        WebRenderSync::render(cx.head(), cx)
    }
}
impl WebRenderSendSync for Page {
    const ROLE: ComponentRole = ComponentRole::PAGE;
    fn render_send_sync(self, cx: &mut SendWebContext) -> Result<(), WriteError> {
        WebRenderSendSync::render_send_sync(cx.head(), cx)
    }
}

struct SyncNode<const LAYOUT: bool> {
    calls: Arc<AtomicUsize>,
    with_page: bool,
}
impl<const LAYOUT: bool> WebRenderSync for SyncNode<LAYOUT> {
    const ROLE: ComponentRole = if LAYOUT {
        ComponentRole::LAYOUT
    } else {
        ComponentRole::COMPONENT
    };
    fn render(self, cx: &mut WebContext) -> Result<(), WriteError> {
        self.calls.fetch_add(1, Ordering::Relaxed);
        if self.with_page {
            head_dispatch::sync(Page, cx)
        } else {
            Ok(())
        }
    }
}
impl<const LAYOUT: bool> WebRenderSendSync for SyncNode<LAYOUT> {
    const ROLE: ComponentRole = if LAYOUT {
        ComponentRole::LAYOUT
    } else {
        ComponentRole::COMPONENT
    };
    fn render_send_sync(self, cx: &mut SendWebContext) -> Result<(), WriteError> {
        self.calls.fetch_add(1, Ordering::Relaxed);
        if self.with_page {
            head_dispatch::send_sync(Page, cx)
        } else {
            Ok(())
        }
    }
}
struct AsyncNode<const LAYOUT: bool>(SyncNode<LAYOUT>);
impl<const LAYOUT: bool> WebRender for AsyncNode<LAYOUT> {
    const ROLE: ComponentRole = <SyncNode<LAYOUT> as WebRenderSync>::ROLE;
    fn render(
        self,
        cx: &mut WebContext,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>> {
        RenderOutcome::<Ready<_>>::Done(WebRenderSync::render(self.0, cx))
    }
}
impl<const LAYOUT: bool> WebRenderSend for AsyncNode<LAYOUT> {
    const ROLE: ComponentRole = <SyncNode<LAYOUT> as WebRenderSendSync>::ROLE;
    fn render_send(
        self,
        cx: &mut SendWebContext,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>> + Send> {
        RenderOutcome::<Ready<_>>::Done(WebRenderSendSync::render_send_sync(self.0, cx))
    }
}

#[derive(Clone, Copy, Debug)]
enum State {
    Manual,
    Open,
    Sealed,
    Excluded,
}
impl State {
    fn error(self) -> Option<HeadError> {
        match self {
            Self::Manual => Some(HeadError::ManualMode),
            Self::Open => None,
            Self::Sealed => Some(HeadError::Closed),
            Self::Excluded => Some(HeadError::Excluded),
        }
    }
}
macro_rules! setup {
    ($cx:expr, $state:expr) => {{
        let cx = &mut $cx;
        if !matches!($state, State::Manual) {
            cx.configure_head(HeadConfig::default(), Arc::new(|_, _| Ok(())))
                .unwrap();
        }
        if matches!($state, State::Sealed) {
            cx.managed_seal().unwrap();
        }
        if matches!($state, State::Excluded) {
            Some(cx.require_managed_head().unwrap().exclude())
        } else {
            None
        }
    }};
}
async fn check<const LAYOUT: bool>() {
    for state in [State::Manual, State::Open, State::Sealed, State::Excluded] {
        for surface in 0..6 {
            let calls = Arc::new(AtomicUsize::new(0));
            let node = SyncNode::<LAYOUT> {
                calls: calls.clone(),
                with_page: LAYOUT && matches!(state, State::Open),
            };
            let mut local = WebContext::new();
            let mut send = SendWebContext::new();
            let _exclusion: Option<ExcludedGuard> = if surface < 3 {
                setup!(local, state)
            } else {
                setup!(send, state)
            };
            let result = match surface {
                0 => head_dispatch::sync(node, &mut local),
                1 => {
                    head_dispatch::begin(to_async(node), &mut local)
                        .resolve()
                        .await
                }
                2 => {
                    head_dispatch::begin(AsyncNode(node), &mut local)
                        .resolve()
                        .await
                }
                3 => head_dispatch::send_sync(node, &mut send),
                4 => {
                    head_dispatch::send_begin(to_send(node), &mut send)
                        .resolve()
                        .await
                }
                5 => {
                    head_dispatch::send_begin(AsyncNode(node), &mut send)
                        .resolve()
                        .await
                }
                _ => unreachable!(),
            };
            if let Some(expected) = state.error().filter(|_| LAYOUT) {
                assert!(
                    matches!(result, Err(WriteError::HeadLifecycle(actual)) if actual == expected),
                    "{state:?}, surface {surface}: {result:?}"
                );
                assert_eq!(
                    calls.load(Ordering::Relaxed),
                    0,
                    "rejected layout body must not run"
                );
            } else {
                result.unwrap();
                assert_eq!(calls.load(Ordering::Relaxed), 1);
                if LAYOUT {
                    let document = if surface < 3 {
                        local.require_managed_head()
                    } else {
                        send.require_managed_head()
                    }
                    .unwrap();
                    document.final_check().unwrap();
                    assert!(document.is_published());
                }
            }
        }
    }
}
#[tokio::test]
async fn hookless_layout_contract_matches_across_six_surfaces() {
    check::<true>().await;
}
#[tokio::test]
async fn ordinary_components_remain_body_only_on_every_surface() {
    check::<false>().await;
}

struct StatelessLayout;
impl<L: proa_core::DataLoader, B: WriteBuf> WebRenderSync<L, B> for StatelessLayout {
    const ROLE: ComponentRole = ComponentRole::LAYOUT;
    fn render(self, _: &mut WebContext<L, B>) -> Result<(), WriteError> {
        Ok(())
    }
}
#[test]
fn stateless_adapter_cannot_erase_a_hookless_layout_role() {
    let mut cx = SendWebContext::new();
    assert!(WebRenderSendSync::render_send_sync(
        proa_core::to_send_static(StatelessLayout),
        &mut cx
    )
    .is_err());
}
