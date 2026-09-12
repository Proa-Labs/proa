// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use std::{future::Future, sync::Arc};

use proa_actions::ActionDataStatic;
use proa_core::{
    DataLoader, LoadError, LoadRequest, LoadValue, RenderBuffer, WebContext, WebRender,
    WebRenderSync, WriteBuf, WriteError,
};
use proa_macros::{html, ProaComponent};

#[derive(ProaComponent)]
#[component(tag = "section", async_render, children_builder)]
struct Card<C = ()> {
    #[component(attr, attr_name = "id")]
    id: Option<&'static str>,
    #[component(class)]
    class: Option<&'static str>,
    #[component(children)]
    children: Option<C>,
}

#[derive(ProaComponent)]
#[component(tag = "article", children_builder)]
struct GenericShell<'a, Metadata, const N: usize, C = ()> {
    #[component(attr, attr_name = "id")]
    id: Option<&'a str>,
    metadata: Metadata,
    bytes: [u8; N],
    #[component(children)]
    children: Option<C>,
}

impl<Metadata, const N: usize, C> GenericShell<'_, Metadata, N, C> {
    fn classes(&self) -> ([&'static str; 1], usize) {
        (["shell"], 1)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AuthoringTone {
    Quiet,
}

#[derive(ProaComponent)]
#[component(tag = "aside", constructor, children_builder)]
struct AuthoredShell<'a, Metadata, const N: usize, C>
where
    Metadata: Copy,
{
    label: &'a str,
    metadata: Metadata,
    #[component(optional_builder = "with_extra")]
    extra: Option<Metadata>,
    bytes: [u8; N],
    #[component(default = AuthoringTone::Quiet)]
    tone: AuthoringTone,
    #[component(default)]
    hidden: bool,
    #[component(attr, attr_name = "id", optional_builder = "with_id")]
    id: Option<&'a str>,
    #[component(class, optional_builder = "with_class")]
    class: Option<&'static str>,
    #[component(children)]
    children: Option<C>,
}

impl<Metadata: Copy, const N: usize, C> AuthoredShell<'_, Metadata, N, C> {
    fn classes(&self) -> ([&'static str; 1], usize) {
        (["quiet"], 1)
    }
}

#[derive(ProaComponent)]
#[component(tag = "mark", constructor)]
struct MappedText<'a> {
    #[component(children, map = "as_deref", optional_builder = "with_text")]
    text: core::option::Option<&'a str>,
}

impl MappedText<'_> {
    fn classes(&self) -> ([&'static str; 0], usize) {
        ([], 0)
    }
}

#[derive(ProaComponent)]
#[component(tag = "generic-default", constructor)]
struct GenericDefault<State> {
    #[component(default)]
    state: State,
}

impl<State> GenericDefault<State> {
    fn classes(&self) -> ([&'static str; 0], usize) {
        ([], 0)
    }
}

impl<C> Card<C> {
    fn classes(&self) -> ([&'static str; 1], usize) {
        (["card"], 1)
    }
}

#[derive(ProaComponent)]
#[component(tag = "span")]
#[component(async_render)]
struct SplitAttributeBridge {}

impl SplitAttributeBridge {
    fn classes(&self) -> ([&'static str; 0], usize) {
        ([], 0)
    }
}

#[derive(ProaComponent)]
#[component(tag = "write-shape")]
struct WriteShapeProbe {
    #[component(attr, attr_name = "data-value")]
    value: Option<&'static str>,
    #[component(bool_attr, attr_name = "disabled")]
    disabled: bool,
    #[component(action, attr_name = "data-action")]
    action: Option<ActionDataStatic>,
}

impl WriteShapeProbe {
    fn classes(&self) -> ([&'static str; 0], usize) {
        ([], 0)
    }
}

#[derive(Default)]
struct RecordingBuf {
    bytes: Vec<u8>,
    writes: Vec<Vec<u8>>,
}

impl WriteBuf for RecordingBuf {
    fn capacity(&self) -> usize {
        usize::MAX
    }

    fn len(&self) -> usize {
        self.bytes.len()
    }

    fn extend(&mut self, bytes: &[u8]) -> Result<(), WriteError> {
        self.writes.push(bytes.to_vec());
        self.bytes.extend_from_slice(bytes);
        Ok(())
    }

    fn as_slice(&self) -> &[u8] {
        &self.bytes
    }

    fn to_vec(&self) -> Vec<u8> {
        self.bytes.clone()
    }

    fn clear(&mut self) {
        self.bytes.clear();
        self.writes.clear();
    }
}

#[derive(Debug)]
struct TestLoader;

impl DataLoader for TestLoader {
    fn load(
        &self,
        _request: &LoadRequest,
    ) -> impl Future<Output = Result<LoadValue, LoadError>> + Send {
        std::future::ready(Err(LoadError::NoLoader))
    }
}

fn assert_sync_component<B, L, T>()
where
    B: proa_core::WriteBuf,
    L: DataLoader,
    T: WebRenderSync<L, B>,
{
}

fn assert_async_component<B, L, T>()
where
    B: proa_core::WriteBuf,
    L: DataLoader,
    T: WebRender<L, B>,
{
}

#[test]
fn derive_hides_buffer_and_loader_generics_for_generic_children() {
    assert_sync_component::<Vec<u8>, TestLoader, Card<&'static str>>();
    assert_sync_component::<RenderBuffer, TestLoader, Card<&'static str>>();
    assert_async_component::<Vec<u8>, TestLoader, Card<&'static str>>();
    assert_async_component::<Vec<u8>, TestLoader, SplitAttributeBridge>();

    let mut out = Vec::new();
    let mut cx = WebContext::with_loader_buffer(&mut out, Arc::new(TestLoader));
    WebRenderSync::render(
        Card {
            id: Some("profile"),
            class: Some("featured"),
            children: Some("Ada & Lin"),
        },
        &mut cx,
    )
    .unwrap();

    assert_eq!(
        String::from_utf8(out).unwrap(),
        r#"<section class="card featured" id="profile">Ada &amp; Lin</section>"#
    );
}

#[test]
fn children_builder_infers_child_type_and_preserves_fields_and_generics() {
    let component = GenericShell::<_, 2, ()> {
        id: Some("profile"),
        metadata: 17_u16,
        bytes: [3, 5],
        children: None,
    }
    .with_children("Ada & Lin");

    fn assert_type(_: &GenericShell<'_, u16, 2, &'static str>) {}
    assert_type(&component);
    assert_eq!(component.metadata, 17);
    assert_eq!(component.bytes, [3, 5]);

    let mut out = Vec::new();
    let mut cx = WebContext::with_loader_buffer(&mut out, Arc::new(TestLoader));
    WebRenderSync::render(component, &mut cx).unwrap();
    assert_eq!(
        String::from_utf8(out).unwrap(),
        r#"<article class="shell" id="profile">Ada &amp; Lin</article>"#
    );
}

#[test]
fn constructor_and_optional_builders_preserve_static_types_and_defaults() {
    let initial = AuthoredShell::new("launch", 17_u16, [3, 5]);
    let initial_layout = (
        std::mem::size_of_val(&initial),
        std::mem::align_of_val(&initial),
    );
    let configured = initial
        .with_extra(23_u16)
        .with_id("profile")
        .with_class("featured");
    assert_eq!(
        (
            std::mem::size_of_val(&configured),
            std::mem::align_of_val(&configured),
        ),
        initial_layout
    );
    let component = configured.with_children("Ada & Lin");

    fn assert_type(_: &AuthoredShell<'_, u16, 2, &'static str>) {}
    assert_type(&component);
    assert_eq!(component.label, "launch");
    assert_eq!(component.metadata, 17);
    assert_eq!(component.extra, Some(23));
    assert_eq!(component.bytes, [3, 5]);
    assert_eq!(component.tone, AuthoringTone::Quiet);
    assert!(!component.hidden);
    assert_eq!(component.id, Some("profile"));
    assert_eq!(component.class, Some("featured"));

    let mut out = Vec::new();
    let mut cx = WebContext::with_loader_buffer(&mut out, Arc::new(TestLoader));
    WebRenderSync::render(component, &mut cx).unwrap();
    assert_eq!(
        String::from_utf8(out).unwrap(),
        r#"<aside class="quiet featured" id="profile">Ada &amp; Lin</aside>"#
    );
}

#[test]
fn optional_builder_supports_a_mapped_concrete_child() {
    let component = MappedText::new().with_text("Mapped & safe");
    assert_eq!(component.text, Some("Mapped & safe"));

    let mut out = Vec::new();
    let mut cx = WebContext::with_buffer(&mut out);
    WebRenderSync::render(component, &mut cx).unwrap();
    assert_eq!(
        String::from_utf8(out).unwrap(),
        "<mark>Mapped &amp; safe</mark>"
    );
}

#[test]
fn default_uses_a_method_local_bound_for_generic_fields() {
    let component = GenericDefault::<String>::new();
    assert!(component.state.is_empty());
}

#[test]
fn derive_consolidates_compile_time_tag_and_attribute_fragments() {
    let component = WriteShapeProbe {
        value: Some("A&B"),
        disabled: true,
        action: Some(ActionDataStatic::new("save&go")),
    };
    let mut out = RecordingBuf::default();
    let mut cx = WebContext::with_buffer(&mut out);

    component.render_opening(&mut cx).unwrap();
    component.render_closing(&mut cx).unwrap();
    drop(cx);

    assert_eq!(
        std::str::from_utf8(out.as_slice()).unwrap(),
        concat!(
            r#"<write-shape data-value="A&amp;B" disabled "#,
            r#"data-action="save&amp;go"></write-shape>"#
        )
    );

    let writes = out.writes.iter().map(Vec::as_slice).collect::<Vec<_>>();
    for consolidated in [
        b"<write-shape".as_slice(),
        br#" data-value=""#,
        b" disabled",
        br#" data-action=""#,
        b"</write-shape>",
    ] {
        assert!(
            writes.contains(&consolidated),
            "missing consolidated write {:?} in {:?}",
            String::from_utf8_lossy(consolidated),
            writes
                .iter()
                .map(|write| String::from_utf8_lossy(write))
                .collect::<Vec<_>>()
        );
    }
}

#[tokio::test]
async fn async_render_opt_in_provides_an_immediate_web_render_bridge() {
    let rendered = proa_core::render_to_string_async(html! {
        <main>
            {Card::<() > { id: None, class: None, children: None, }.with_children("Hello")}
        </main>
    })
    .await;
    assert_eq!(
        rendered,
        r#"<main><section class="card">Hello</section></main>"#
    );
}
