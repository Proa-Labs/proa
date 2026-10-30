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
        const ROLE: ComponentRole = ComponentRole::PAGE;

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
        const ROLE: ComponentRole = ComponentRole::PAGE;

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
        const ROLE: ComponentRole = ComponentRole::PAGE;
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
        const ROLE: ComponentRole = ComponentRole::PAGE;
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
    fn explicit_page_hooks_work_in_all_render_surfaces() {
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

    #[derive(Clone)]
    struct Layout<C> {
        title: &'static str,
        calls: Arc<AtomicUsize>,
        child: C,
    }
    impl<C: WebRender + 'static> WebRender for Layout<C> {
        const ROLE: proa_core::ComponentRole = proa_core::ComponentRole::LAYOUT;

        async fn render_head(&self, cx: &mut WebContext) -> Result<(), WriteError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            cx.head_mut().title(self.title)?;
            cx.head_mut().description(self.title)?;
            Ok(())
        }
        fn render(
            self,
            cx: &mut WebContext,
        ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>> {
            head_dispatch::begin(self.child, cx)
        }
    }
    impl<C: WebRenderSend + 'static> WebRenderSend for Layout<C> {
        const ROLE: proa_core::ComponentRole = proa_core::ComponentRole::LAYOUT;
        fn render_head_send(
            &self,
            cx: &mut SendWebContext,
        ) -> impl Future<Output = Result<(), WriteError>> + Send {
            // Capture independent values so the layout's child need not be Sync.
            let title = self.title;
            let calls = self.calls.clone();
            async move {
                calls.fetch_add(1, Ordering::SeqCst);
                cx.head_mut().title(title)?;
                cx.head_mut().description(title)?;
                Ok(())
            }
        }
        fn render_send(
            self,
            cx: &mut SendWebContext,
        ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>> + Send> {
            head_dispatch::send_begin(self.child, cx)
        }
    }
    impl<C: WebRenderSync> WebRenderSync for Layout<C> {
        const ROLE: proa_core::ComponentRole = proa_core::ComponentRole::LAYOUT;
        fn render_head(&self, cx: &mut WebContext) -> impl SyncHeadOperation {
            self.calls.fetch_add(1, Ordering::SeqCst);
            cx.head_mut().title(self.title)?;
            cx.head_mut().description(self.title)?;
            Ok(())
        }
        fn render(self, cx: &mut WebContext) -> Result<(), WriteError> {
            head_dispatch::sync(self.child, cx)
        }
    }
    impl<C: WebRenderSendSync> WebRenderSendSync for Layout<C> {
        const ROLE: proa_core::ComponentRole = proa_core::ComponentRole::LAYOUT;
        fn render_head_send_sync(&self, cx: &mut SendWebContext) -> impl SyncHeadOperation {
            self.calls.fetch_add(1, Ordering::SeqCst);
            cx.head_mut().title(self.title)?;
            cx.head_mut().description(self.title)?;
            Ok(())
        }
        fn render_send_sync(self, cx: &mut SendWebContext) -> Result<(), WriteError> {
            head_dispatch::send_sync(self.child, cx)
        }
    }

    #[test]
    fn ancestor_layout_hooks_remain_open_until_the_selected_page() {
        for surface in 0..6 {
            let calls = Arc::new(AtomicUsize::new(0));
            let root = Layout {
                title: "Root defaults",
                calls: calls.clone(),
                child: Layout {
                    title: "Shop defaults",
                    calls: calls.clone(),
                    child: Page {
                        title: "Product & title",
                        calls: calls.clone(),
                    },
                },
            };
            let descriptor =
                head_dispatch::descriptor::<NoLoader, RenderBuffer, Layout<Layout<Page>>>();
            assert!(descriptor.runs_head);
            assert!(!descriptor.is_page);
            assert!(descriptor.is_layout);
            let output = Arc::new(Mutex::new(Vec::new()));
            if surface < 3 {
                let mut cx = WebContext::new();
                cx.configure_head(HeadConfig::default(), publisher(&output))
                    .unwrap();
                WebRenderSync::render(cx.head(), &mut cx).unwrap();
                match surface {
                    0 => block_on(render_child(&mut cx, root)).unwrap(),
                    1 => head_dispatch::sync(root, &mut cx).unwrap(),
                    _ => block_on(render_child(&mut cx, to_async(root))).unwrap(),
                }
                cx.head_state().document().unwrap().final_check().unwrap();
            } else {
                let mut cx = SendWebContext::new();
                cx.configure_head(HeadConfig::default(), publisher(&output))
                    .unwrap();
                WebRenderSendSync::render_send_sync(cx.head(), &mut cx).unwrap();
                match surface {
                    3 => block_on(render_send_child(&mut cx, root)).unwrap(),
                    4 => head_dispatch::send_sync(root, &mut cx).unwrap(),
                    _ => block_on(render_send_child(&mut cx, to_send(root))).unwrap(),
                }
                cx.head_state().document().unwrap().final_check().unwrap();
            }
            assert_eq!(calls.load(Ordering::SeqCst), 3, "surface={surface}");
            let output = output.lock().unwrap();
            let html = std::str::from_utf8(&output).unwrap();
            assert!(
                html.contains("<title>Product &amp; title</title>"),
                "{html}"
            );
            assert!(html.contains("content=\"Shop defaults\""), "{html}");
        }
    }

    struct Outer<C>(C);
    impl<C: WebRender + 'static> WebRender for Outer<C> {
        const ROLE: ComponentRole = ComponentRole::PAGE;

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

    #[test]
    fn layout_hook_below_the_page_is_rejected_before_it_runs() {
        let calls = Arc::new(AtomicUsize::new(0));
        let mut cx = WebContext::new();
        cx.configure_head(HeadConfig::default(), Arc::new(|_, _| Ok(())))
            .unwrap();
        let child = Layout {
            title: "Forbidden body metadata",
            calls: calls.clone(),
            child: (),
        };
        let result = block_on(render_child(&mut cx, Outer(child)));
        assert_eq!(result, Err(WriteError::HeadLifecycle(HeadError::Closed)));
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        assert_eq!(cx.head_state().document().unwrap().final_check(), result);
    }

    #[test]
    fn dropping_unpolled_page_or_layout_cancels_preparation_without_running_hook() {
        for send in [false, true] {
            for layout in [false, true] {
                let calls = Arc::new(AtomicUsize::new(0));
                let page = Page {
                    title: "Unpolled",
                    calls: calls.clone(),
                };
                let config = HeadConfig::default();
                let sink: managed_head::Publisher = Arc::new(|_, _| Ok(()));
                let document = if send {
                    let mut cx = SendWebContext::new();
                    cx.configure_head(config, sink).unwrap();
                    let document = cx.head_state().document().unwrap().clone();
                    if layout {
                        drop(head_dispatch::send_begin(
                            Layout {
                                title: "Layout",
                                calls: calls.clone(),
                                child: page,
                            },
                            &mut cx,
                        ));
                    } else {
                        drop(head_dispatch::send_begin(page, &mut cx));
                    }
                    document
                } else {
                    let mut cx = WebContext::new();
                    cx.configure_head(config, sink).unwrap();
                    let document = cx.head_state().document().unwrap().clone();
                    if layout {
                        drop(head_dispatch::begin(
                            Layout {
                                title: "Layout",
                                calls: calls.clone(),
                                child: page,
                            },
                            &mut cx,
                        ));
                    } else {
                        drop(head_dispatch::begin(page, &mut cx));
                    }
                    document
                };
                assert_eq!(calls.load(Ordering::SeqCst), 0);
                assert_eq!(
                    document.final_check(),
                    Err(WriteError::HeadLifecycle(HeadError::Cancelled))
                );
            }
        }
    }

    struct CountBody(Arc<AtomicUsize>);
    impl WebRenderSync for CountBody {
        fn render(self, _: &mut WebContext) -> Result<(), WriteError> {
            self.0.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
    }

    #[test]
    fn direct_render_child_keeps_body_work_lazy_until_polled() {
        let calls = Arc::new(AtomicUsize::new(0));
        let mut cx = WebContext::new();
        let future = render_child(&mut cx, to_async(CountBody(calls.clone())));
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        drop(future);
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        block_on(render_child(&mut cx, to_async(CountBody(calls.clone())))).unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn page_fallback_is_rejected_before_the_protected_child_runs() {
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
