// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

#![deny(warnings)]

#[cfg(test)]
mod tests {
    use futures::executor::block_on;
    use proa_core::*;
    use std::{
        future::{Future, Ready},
        sync::{
            atomic::{AtomicUsize, Ordering},
            Arc, Mutex,
        },
    };

    #[derive(Clone)]
    struct Page {
        title: &'static str,
        calls: Arc<AtomicUsize>,
    }
    impl WebRender for Page {
        #[allow(refining_impl_trait)]
        async fn render_head(&self, cx: &mut WebContext) -> Result<(), WriteError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            let title = std::future::ready(self.title).await;
            cx.head_mut().title(title)?;
            Ok(())
        }
        fn render(
            self,
            _: &mut WebContext,
        ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>> {
            RenderOutcome::<Ready<_>>::Done(Ok(()))
        }
    }
    impl WebRenderSend for Page {
        #[allow(refining_impl_trait)]
        async fn render_head_send(&self, cx: &mut SendWebContext) -> Result<(), WriteError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            let title = std::future::ready(self.title).await;
            cx.head_mut().title(title)?;
            Ok(())
        }
        fn render_send(
            self,
            _: &mut SendWebContext,
        ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>> + Send> {
            RenderOutcome::<Ready<_>>::Done(Ok(()))
        }
    }
    impl WebRenderSync for Page {
        fn render_head(&self, cx: &mut WebContext) -> impl SyncHeadOperation {
            self.calls.fetch_add(1, Ordering::SeqCst);
            cx.head_mut().title(self.title)?;
            Ok(())
        }
        fn render(self, _: &mut WebContext) -> Result<(), WriteError> {
            Ok(())
        }
    }
    impl WebRenderSendSync for Page {
        fn render_head_send_sync(&self, cx: &mut SendWebContext) -> impl SyncHeadOperation {
            self.calls.fetch_add(1, Ordering::SeqCst);
            cx.head_mut().title(self.title)?;
            Ok(())
        }
        fn render_send_sync(self, _: &mut SendWebContext) -> Result<(), WriteError> {
            Ok(())
        }
    }

    fn publisher(output: &Arc<Mutex<Vec<u8>>>) -> managed_head::Publisher {
        let output = output.clone();
        Arc::new(move |_, parts| {
            for part in parts {
                output.lock().unwrap().extend_from_slice(&part);
            }
            Ok(())
        })
    }

    #[test]
    fn head_hook_alone_selects_the_page_in_all_render_surfaces() {
        for surface in 0..6 {
            let calls = Arc::new(AtomicUsize::new(0));
            let page = Page {
                title: "A & B",
                calls: calls.clone(),
            };
            let output = Arc::new(Mutex::new(Vec::new()));
            let config = HeadConfig::default()
                .with_defaults(|head| {
                    head.title("Default")?;
                    head.description("Shared description")
                })
                .unwrap();
            if surface < 3 {
                let mut cx = WebContext::new();
                cx.configure_head(config, publisher(&output)).unwrap();
                WebRenderSync::render(cx.head(), &mut cx).unwrap();
                match surface {
                    0 => block_on(render_child(&mut cx, page)).unwrap(),
                    1 => head_dispatch::sync(page, &mut cx).unwrap(),
                    _ => block_on(render_child(&mut cx, to_async(page))).unwrap(),
                }
                cx.head_state().document().unwrap().final_check().unwrap();
            } else {
                let mut cx = SendWebContext::new();
                cx.configure_head(config, publisher(&output)).unwrap();
                WebRenderSendSync::render_send_sync(cx.head(), &mut cx).unwrap();
                match surface {
                    3 => block_on(render_send_child(&mut cx, page)).unwrap(),
                    4 => head_dispatch::send_sync(page, &mut cx).unwrap(),
                    _ => block_on(render_send_child(&mut cx, to_send(page))).unwrap(),
                }
                cx.head_state().document().unwrap().final_check().unwrap();
            }
            assert_eq!(calls.load(Ordering::SeqCst), 1, "surface={surface}");
            let output = output.lock().unwrap();
            let html = std::str::from_utf8(&output).unwrap();
            assert!(html.contains("<title>A &amp; B</title>"), "{html}");
            assert!(html.contains("content=\"Shared description\""), "{html}");
        }
    }

    #[test]
    fn page_detection_preserves_adapter_identity_without_running_hooks() {
        fn is_page<C: WebRender>(_: &C) -> bool {
            head_dispatch::descriptor::<NoLoader, RenderBuffer, C>().is_page
        }
        let calls = Arc::new(AtomicUsize::new(0));
        let page = Page {
            title: "Untouched",
            calls: calls.clone(),
        };
        assert!(is_page(&page));
        assert!(is_page(&to_async(page)));
        assert!(!is_page(&()));
        assert!(!is_page(&to_async("ordinary child")));
        assert_eq!(calls.load(Ordering::SeqCst), 0);
    }

    struct Outer(Page);
    impl WebRender for Outer {
        #[allow(refining_impl_trait)]
        async fn render_head(&self, _: &mut WebContext) -> Result<(), WriteError> {
            Ok(())
        }
        fn render(
            self,
            cx: &mut WebContext,
        ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>> {
            head_dispatch::begin(self.0, cx)
        }
    }

    #[test]
    fn nested_page_is_rejected_before_its_hook_runs() {
        let calls = Arc::new(AtomicUsize::new(0));
        let mut cx = WebContext::new();
        cx.configure_head(HeadConfig::default(), Arc::new(|_, _| Ok(())))
            .unwrap();
        let result = block_on(render_child(
            &mut cx,
            Outer(Page {
                title: "Too late",
                calls: calls.clone(),
            }),
        ));
        assert_eq!(result, Err(WriteError::HeadLifecycle(HeadError::Closed)));
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        assert_eq!(cx.head_state().document().unwrap().final_check(), result);
    }

    struct CountBody(Arc<AtomicUsize>);
    impl WebRenderSync for CountBody {
        fn render(self, _: &mut WebContext) -> Result<(), WriteError> {
            self.0.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
    }

    #[test]
    fn inferred_page_fallback_is_rejected_before_the_protected_child_runs() {
        for synchronous in [false, true] {
            let calls = Arc::new(AtomicUsize::new(0));
            let mut cx = WebContext::new();
            cx.configure_head(HeadConfig::default(), Arc::new(|_, _| Ok(())))
                .unwrap();
            let fallback = Page {
                title: "Forbidden fallback",
                calls: calls.clone(),
            };
            let result = if synchronous {
                head_dispatch::sync(
                    ErrorBoundary::new(CountBody(calls.clone()), fallback),
                    &mut cx,
                )
            } else {
                block_on(render_child(
                    &mut cx,
                    ErrorBoundary::new(to_async(CountBody(calls.clone())), fallback),
                ))
            };
            assert!(
                matches!(result, Err(WriteError::HeadFailure(_))),
                "{result:?}"
            );
            assert_eq!(calls.load(Ordering::SeqCst), 0);
            assert_eq!(cx.head_state().document().unwrap().final_check(), result);
        }
    }
}
