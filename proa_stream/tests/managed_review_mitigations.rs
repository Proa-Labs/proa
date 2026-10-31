// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use proa_core::*;
use std::{
    future::{Future, Ready},
    sync::Arc,
};

struct Failure {
    hook: bool,
}
impl WebRender for Failure {
    const ROLE: proa_core::ComponentRole = proa_core::ComponentRole::LAYOUT;
    async fn render_head(&self, cx: &mut WebContext) -> Result<(), WriteError> {
        cx.head_mut().title("default")?;
        if self.hook {
            return Err(WriteError::render("database connection refused"));
        }
        Ok(())
    }
    fn render(
        self,
        _: &mut WebContext,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>> {
        RenderOutcome::<Ready<_>>::Done(Err(WriteError::render("database connection refused")))
    }
}
#[test]
fn original_failure_survives_hook_body_and_finalization() {
    use std::error::Error;
    for hook in [true, false] {
        let mut cx = WebContext::new();
        cx.configure_head(HeadConfig::default(), Arc::new(|_, _| Ok(())))
            .unwrap();
        let err = futures::executor::block_on(render_child(&mut cx, Failure { hook })).unwrap_err();
        assert!(!err.is_boundary_catchable());
        let WriteError::HeadFailure(failure) = &err else {
            panic!("{err:?}")
        };
        assert!(failure.component.ends_with("Failure"));
        assert_eq!(failure.phase, if hook { "render_head" } else { "render" });
        assert_eq!(
            failure.cause,
            WriteError::render("database connection refused")
        );
        assert!(err
            .source()
            .unwrap()
            .source()
            .unwrap()
            .to_string()
            .contains("database connection refused"));
        assert_eq!(
            cx.head_state()
                .document()
                .unwrap()
                .final_check()
                .unwrap_err(),
            err
        );
    }
}
#[test]
fn detached_local_and_send_responses_cannot_stage_ignored_decisions() {
    let mut cx = WebContext::new();
    assert!(cx.response_mut().set_status(http::StatusCode::CREATED));
    let mut child = WebContext::with_loader_buffer_and_context_parts(
        RenderBuffer::new(),
        Arc::new(NoLoader),
        cx.context_parts(),
    );
    assert!(!child.response_mut().set_status(http::StatusCode::NOT_FOUND));
    assert!(!child.response_mut().insert_header(
        http::header::CACHE_CONTROL,
        http::HeaderValue::from_static("no-store")
    ));
    assert_eq!(cx.response().status(), Some(http::StatusCode::CREATED));
    let mut cx = SendWebContext::new();
    assert!(cx.response_mut().set_status(http::StatusCode::CREATED));
    let mut child = SendWebContext::with_loader_buffer_and_context_parts(
        RenderBuffer::new(),
        Arc::new(NoLoader),
        cx.context_parts(),
    );
    assert!(!child.response_mut().set_status(http::StatusCode::NOT_FOUND));
    assert!(!child.response_mut().insert_header(
        http::header::CACHE_CONTROL,
        http::HeaderValue::from_static("no-store")
    ));
}

#[test]
fn publisher_failure_preserves_its_source_without_reopening() {
    let mut cx = WebContext::new();
    cx.configure_head(
        HeadConfig::default(),
        Arc::new(|_, _| Err(WriteError::StreamClosed)),
    )
    .unwrap();
    WebRenderSync::render(cx.head(), &mut cx).unwrap();
    let error = cx.managed_seal().unwrap_err();
    let WriteError::HeadFailure(failure) = &error else {
        panic!("{error:?}")
    };
    assert_eq!(failure.kind, HeadError::PublisherFailed);
    assert_eq!(failure.cause, WriteError::StreamClosed);
    assert_eq!(
        cx.head_state()
            .document()
            .unwrap()
            .final_check()
            .unwrap_err(),
        error
    );
}
