// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

#[cfg(test)]
mod tests {
    use proa_core::*;
    use std::{
        future::Future,
        sync::{
            atomic::{AtomicUsize, Ordering},
            Arc,
        },
        task::{Context, Waker},
    };

    #[derive(Clone, Copy)]
    enum Finish {
        Pending,
        Ready,
        Error,
        Panic,
    }
    struct InheritedPage(Finish);
    async fn body(out: &mut RenderBuffer, finish: Finish) -> Result<(), WriteError> {
        out.extend(b"partial body")?;
        match finish {
            Finish::Pending => std::future::pending().await,
            Finish::Ready => Ok(()),
            Finish::Error => Err(WriteError::render("original body error")),
            Finish::Panic => panic!("body panic"),
        }
    }
    impl WebRender for InheritedPage {
        const ROLE: ComponentRole = ComponentRole::PAGE;
        fn render(
            self,
            cx: &mut WebContext,
        ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>> {
            RenderOutcome::pending(body(cx.output_mut(), self.0))
        }
    }
    impl WebRenderSend for InheritedPage {
        const ROLE: ComponentRole = ComponentRole::PAGE;
        fn render_send(
            self,
            cx: &mut SendWebContext,
        ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>> + Send> {
            RenderOutcome::pending(body(cx.output_mut(), self.0))
        }
    }

    struct HookedPage(Finish);
    impl WebRender for HookedPage {
        const ROLE: ComponentRole = ComponentRole::PAGE;
        #[allow(refining_impl_trait)]
        async fn render_head(&self, cx: &mut WebContext) -> Result<(), WriteError> {
            cx.head_mut().title("hooked")
        }
        fn render(
            self,
            cx: &mut WebContext,
        ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>> {
            RenderOutcome::pending(body(cx.output_mut(), self.0))
        }
    }
    impl WebRenderSend for HookedPage {
        const ROLE: ComponentRole = ComponentRole::PAGE;
        fn render_head_send(&self, cx: &mut SendWebContext) -> impl SendHeadOperation {
            async move { cx.head_mut().title("hooked") }
        }
        fn render_send(
            self,
            cx: &mut SendWebContext,
        ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>> + Send> {
            RenderOutcome::pending(body(cx.output_mut(), self.0))
        }
    }

    fn publisher(count: &Arc<AtomicUsize>) -> managed_head::Publisher {
        let count = count.clone();
        Arc::new(move |_, _| {
            count.fetch_add(1, Ordering::SeqCst);
            Ok(())
        })
    }

    fn assert_cancelled(result: Result<(), WriteError>) {
        assert!(
            matches!(result, Err(WriteError::HeadLifecycle(HeadError::Cancelled))),
            "{result:?}"
        );
    }

    fn poll_pending(future: impl Future<Output = Result<(), WriteError>>, published: bool) {
        let mut future = std::pin::pin!(future);
        let mut task = Context::from_waker(Waker::noop());
        assert!(future.as_mut().poll(&mut task).is_pending());
        if published {
            // Advance past the publication yield into the pending body.
            assert!(future.as_mut().poll(&mut task).is_pending());
        }
    }

    const MODES: [RenderMode; 4] = [
        RenderMode::Static,
        RenderMode::Agent,
        RenderMode::ChunkedInOrder,
        RenderMode::ChunkedOutOfOrder,
    ];

    #[test]
    fn inherited_page_cancellation_is_sticky_in_local_and_macro_dispatch() {
        for mode in MODES {
            for has_outlet in [false, true] {
                for macro_dispatch in [false, true] {
                    let publications = Arc::new(AtomicUsize::new(0));
                    let mut cx = WebContext::new().with_render_mode(mode);
                    cx.configure_head(HeadConfig::default(), publisher(&publications))
                        .unwrap();
                    if has_outlet {
                        WebRenderSync::render(cx.head(), &mut cx).unwrap();
                    }
                    if macro_dispatch {
                        poll_pending(
                            head_dispatch::begin(InheritedPage(Finish::Pending), &mut cx).resolve(),
                            has_outlet,
                        );
                    } else {
                        poll_pending(
                            render_child(&mut cx, InheritedPage(Finish::Pending)),
                            has_outlet,
                        );
                    }
                    assert_cancelled(cx.head_state().document().unwrap().final_check());
                    assert_cancelled(WebRenderSync::render(cx.head(), &mut cx));
                    assert_eq!(publications.load(Ordering::SeqCst), usize::from(has_outlet));
                }
            }
        }
    }

    #[test]
    fn inherited_page_cancellation_is_sticky_in_send_dispatch() {
        for mode in MODES {
            for has_outlet in [false, true] {
                let publications = Arc::new(AtomicUsize::new(0));
                let mut cx = SendWebContext::new().with_render_mode(mode);
                cx.configure_head(HeadConfig::default(), publisher(&publications))
                    .unwrap();
                if has_outlet {
                    WebRenderSendSync::render_send_sync(cx.head(), &mut cx).unwrap();
                }
                poll_pending(
                    render_send_child(&mut cx, InheritedPage(Finish::Pending)),
                    has_outlet,
                );
                assert_cancelled(cx.head_state().document().unwrap().final_check());
                assert_cancelled(WebRenderSendSync::render_send_sync(cx.head(), &mut cx));
                assert_eq!(publications.load(Ordering::SeqCst), usize::from(has_outlet));
            }
        }
    }

    #[test]
    fn dropping_unpolled_page_outcomes_invalidates_the_sealed_document() {
        let mut local = WebContext::new();
        local
            .configure_head(HeadConfig::default(), Arc::new(|_, _| Ok(())))
            .unwrap();
        drop(head_dispatch::begin(
            InheritedPage(Finish::Ready),
            &mut local,
        ));
        assert_cancelled(local.head_state().document().unwrap().final_check());

        let mut send = SendWebContext::new();
        send.configure_head(HeadConfig::default(), Arc::new(|_, _| Ok(())))
            .unwrap();
        drop(head_dispatch::send_begin(
            InheritedPage(Finish::Ready),
            &mut send,
        ));
        assert_cancelled(send.head_state().document().unwrap().final_check());
    }

    #[test]
    fn dropping_unpolled_hooked_page_outcomes_invalidates_the_document() {
        // Dispatch claims the hook guard before it returns the outcome, so an
        // abandoned hooked Page cannot leave the document open for a later Page.
        let mut local = WebContext::new();
        local
            .configure_head(HeadConfig::default(), Arc::new(|_, _| Ok(())))
            .unwrap();
        drop(head_dispatch::begin(HookedPage(Finish::Ready), &mut local));
        assert_cancelled(local.head_state().document().unwrap().final_check());
        assert_cancelled(futures::executor::block_on(render_child(
            &mut local,
            InheritedPage(Finish::Ready),
        )));

        let mut local = WebContext::new();
        local
            .configure_head(HeadConfig::default(), Arc::new(|_, _| Ok(())))
            .unwrap();
        drop(render_child(&mut local, HookedPage(Finish::Ready)));
        assert_cancelled(local.head_state().document().unwrap().final_check());

        let mut send = SendWebContext::new();
        send.configure_head(HeadConfig::default(), Arc::new(|_, _| Ok(())))
            .unwrap();
        drop(head_dispatch::send_begin(
            HookedPage(Finish::Ready),
            &mut send,
        ));
        assert_cancelled(send.head_state().document().unwrap().final_check());
    }

    #[test]
    fn successfully_completed_inherited_pages_can_publish_later() {
        let publications = Arc::new(AtomicUsize::new(0));
        let mut local = WebContext::new();
        local
            .configure_head(HeadConfig::default(), publisher(&publications))
            .unwrap();
        futures::executor::block_on(render_child(&mut local, InheritedPage(Finish::Ready)))
            .unwrap();
        WebRenderSync::render(local.head(), &mut local).unwrap();
        local
            .head_state()
            .document()
            .unwrap()
            .final_check()
            .unwrap();

        let mut send = SendWebContext::new();
        send.configure_head(HeadConfig::default(), publisher(&publications))
            .unwrap();
        futures::executor::block_on(render_send_child(&mut send, InheritedPage(Finish::Ready)))
            .unwrap();
        WebRenderSendSync::render_send_sync(send.head(), &mut send).unwrap();
        send.head_state().document().unwrap().final_check().unwrap();
        assert_eq!(publications.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn inherited_page_body_error_is_sticky_and_preserves_its_cause() {
        let mut cx = WebContext::new();
        cx.configure_head(HeadConfig::default(), Arc::new(|_, _| Ok(())))
            .unwrap();
        let error =
            futures::executor::block_on(render_child(&mut cx, InheritedPage(Finish::Error)))
                .unwrap_err();
        let WriteError::HeadFailure(failure) = &error else {
            panic!("{error:?}")
        };
        assert_eq!(failure.kind, HeadError::ProviderFailed);
        assert_eq!(failure.cause, WriteError::render("original body error"));
        assert_eq!(
            cx.head_state().document().unwrap().final_check(),
            Err(error)
        );
    }

    struct SyncPage;
    impl WebRenderSync for SyncPage {
        const ROLE: ComponentRole = ComponentRole::PAGE;
        fn render(self, _: &mut WebContext) -> Result<(), WriteError> {
            panic!("sync body panic")
        }
    }
    impl WebRenderSendSync for SyncPage {
        const ROLE: ComponentRole = ComponentRole::PAGE;
        fn render_send_sync(self, _: &mut SendWebContext) -> Result<(), WriteError> {
            panic!("send sync body panic")
        }
    }

    #[test]
    fn inherited_page_unwind_invalidates_sync_and_async_preparation() {
        for synchronous in [false, true] {
            let mut local = WebContext::new();
            local
                .configure_head(HeadConfig::default(), Arc::new(|_, _| Ok(())))
                .unwrap();
            assert!(std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                if synchronous {
                    head_dispatch::sync(SyncPage, &mut local).unwrap();
                } else {
                    futures::executor::block_on(render_child(
                        &mut local,
                        InheritedPage(Finish::Panic),
                    ))
                    .unwrap();
                }
            }))
            .is_err());
            assert_cancelled(local.head_state().document().unwrap().final_check());

            let mut send = SendWebContext::new();
            send.configure_head(HeadConfig::default(), Arc::new(|_, _| Ok(())))
                .unwrap();
            assert!(std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                if synchronous {
                    head_dispatch::send_sync(SyncPage, &mut send).unwrap();
                } else {
                    futures::executor::block_on(render_send_child(
                        &mut send,
                        InheritedPage(Finish::Panic),
                    ))
                    .unwrap();
                }
            }))
            .is_err());
            assert_cancelled(send.head_state().document().unwrap().final_check());
        }
    }
}
