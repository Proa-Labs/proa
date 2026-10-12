// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use super::*;

const IMPORTS:&str = "use proa_core::{WebRenderSync, WebRender, WebRenderSend, MdRenderSync, MdRender, to_async, to_md_async, text}; use proa_macros::{html_sync, html, md_sync, md, content_part, omit};";
fn fixture(html: &str, markdown: &str) -> String {
    format!("{IMPORTS}\nstruct Product {{title: String, data: Data, stock:u32, items: Vec<Data>, image: String}}\nstruct Data; struct RestockNotice; struct Impact;\nimpl WebRenderSync for Product {{fn render(self, cx: &mut Cx)->Result {{ {html} }} }}\nimpl MdRenderSync for Product {{fn render_md(self, cx: &mut Cx)->Result {{ {markdown} }} }}")
}
fn lint(source: &str) -> Vec<Diagnostic> {
    let path = PathBuf::from("/coverage/src/lib.rs");
    lint_sources(
        &[(path.clone(), source.into())],
        &BTreeSet::from([path]),
        false,
        &ContentOptions::default(),
    )
}
fn code(diags: &[Diagnostic], code: &str) -> usize {
    diags.iter().filter(|d| d.code == code).count()
}
fn missing(source: &str) -> Vec<Diagnostic> {
    lint(source)
        .into_iter()
        .filter(|d| d.code == "render/content-coverage")
        .collect()
}

#[test]
fn deletion_catches_child_even_when_inputs_are_equal() {
    let original=fixture("html_sync! { {RestockNotice::new(self.data)} {Impact::new(self.data)} }.render(cx)","let restock=RestockNotice::new(self.data); let impact=Impact::new(self.data); md_sync! { r###\"{restock} {impact}\"### }.render_md(cx)");
    assert_eq!(code(&lint(&original), "render/content-coverage"), 0);
    let mutated = original.replace("{restock} {impact}", "{restock}");
    let diags = missing(&mutated);
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].severity, Severity::Error);
    assert!(diags[0].message.contains("Impact"));
    assert!(diags[0].help.as_ref().unwrap().contains("{impact}"));
    assert_eq!(diags[0].labels.len(), 2);
}

#[test]
fn markdown_layout_state_does_not_hide_missing_content() {
    let src = fixture(
        "html_sync! {{self.title}}.render(cx)",
        "let previous=cx.md_set_inline(true); md_sync! {r###\"{self.title}\"###}.render_md(cx)?; cx.md_set_inline(previous); cx.md_blank_line()?; Ok(())",
    ).replace("&mut Cx", "&mut proa_core::WebContext");
    assert!(lint(&src).is_empty());
    let missing_title = src.replace("{self.title}\"###", "\"###");
    assert_eq!(missing(&missing_title).len(), 1);
}

#[test]
fn markdown_code_content_is_a_render_capability_not_a_cfg_variant() {
    let source = r#"
use proa_core::{WebRenderSync, MdRenderSync, MdCodeContent};
use proa_macros::{html_sync, md_sync};
struct Badge<C> { children: C }
impl<C: WebRenderSync> WebRenderSync for Badge<C> {
    fn render(self, cx: &mut Cx) -> Result { html_sync! {{self.children}}.render(cx) }
}
impl<C: MdCodeContent> MdRenderSync for Badge<C> {
    fn render_md(self, cx: &mut Cx) -> Result { md_sync! {{self.children}}.render_md(cx) }
}
"#;
    assert!(lint(source).is_empty());
    let changed = source.replace("md_sync! {{self.children}}", "md_sync! {}");
    assert_eq!(missing(&changed).len(), 1);
}
#[test]
fn unused_template_and_uncalled_closure_never_count() {
    for extra in [
        "let unused=md_sync! {r###\"{self.title}\"###};",
        "let closure=|| md_sync! {r###\"{self.title}\"###};",
    ] {
        let src = fixture(
            "html_sync! {{self.title}}.render(cx)",
            &format!("{extra} md_sync! {{r###\"\"###}}.render_md(cx)"),
        );
        assert_eq!(missing(&src).len(), 1);
    }
}
#[test]
fn alias_uses_definition_time_binding_with_shadowing() {
    let src = fixture(
        "html_sync! {{self.title}}.render(cx)",
        "let x=self.title;let alias=x;let x=self.image;md_sync! {r###\"{alias}\"###}.render_md(cx)",
    );
    assert!(missing(&src).is_empty());
    let bad = src.replace("{alias}", "{x}");
    assert_eq!(missing(&bad).len(), 1);
}
#[test]
fn conditions_do_not_count_as_output_and_wrong_guard_is_unchecked() {
    let src=fixture("html_sync! { {if self.stock == 0 {html_sync! {{RestockNotice::new(self.data)}}}} }.render(cx)","md_sync! { r###\"{if self.stock > 0 {{RestockNotice::new(self.data)}}}\"###}.render_md(cx)");
    let diags = lint(&src);
    assert_eq!(code(&diags, "render/content-coverage-unchecked"), 1);
    assert_eq!(code(&diags, "render/content-coverage"), 0);
    assert_eq!(diags[0].severity, Severity::Warning);
    assert!(diags[0]
        .notes
        .iter()
        .any(|n| n.contains("same input states")));
    let tested = fixture(
        "html_sync! {{self.stock}}.render(cx)",
        "if self.stock>0 {} md_sync! {r###\"\"###}.render_md(cx)",
    );
    assert_eq!(missing(&tested).len(), 1);
}
#[test]
fn opaque_helpers_are_not_confirmed_absence() {
    let src = fixture(
        "html_sync! {{RestockNotice::new(self.data)}}.render(cx)",
        "summary(self.data).render_md(cx)",
    );
    let diags = lint(&src);
    assert_eq!(code(&diags, "render/content-coverage"), 0);
    assert_eq!(code(&diags, "render/content-coverage-unchecked"), 1);
    assert!(diags[0].notes.iter().any(|n| n.contains("helper")));
}

#[test]
fn unresolved_render_trait_discovery_is_a_warning() {
    let source = fixture(
        "html_sync! {{self.title}}.render(cx)",
        "md_sync! {r###\"{self.title}\"###}.render_md(cx)",
    )
    .replace(
        IMPORTS,
        "use proa_core::*; use proa_macros::{html_sync, md_sync};",
    );
    let diags = lint(&source);
    assert!(!diags.is_empty());
    assert!(diags.iter().all(|d| {
        d.code == "render/content-coverage-unchecked" && d.severity == Severity::Warning
    }));
}
#[test]
fn context_and_loader_preparation_do_not_create_content_obligations() {
    for preparation in [
        "let loader = cx.loader(); let unused = loader.load(query).await?;",
        "let unused = cx.load_typed::<Data>(query).await?;",
        "let mode = cx.render_mode(); cx.set_render_mode(mode);",
        "let request = cx.request(); let headers = request.headers(); let _ = headers;",
        "let request = cx.request_mut(); let headers = request.headers_mut(); headers.insert(name, value);",
        "cx.head_mut().add_asset(asset); cx.telemetry_mut().record(event);",
        "let locals = cx.locals_mut(); locals.insert(42u32);",
        "let output = &mut *cx; let unused = output.load_bytes(query).await?;",
    ] {
        let src = fixture(
            "html_sync! {{self.title}}.render(cx)",
            &format!("{preparation} md_sync! {{r###\"{{self.title}}\"###}}.render_md(cx)"),
        )
        .replace("&mut Cx", "&mut proa_core::WebContext");
        assert!(lint(&src).is_empty(), "{preparation}: {:?}", lint(&src));
    }
    let src = fixture(
        "html_sync! {{self.title}}.render(cx)",
        "let unused = cx.load_typed::<Data>(query).await?; md_sync! {r###\"\"###}.render_md(cx)",
    )
    .replace("&mut Cx", "&mut RenderCx");
    let src = format!("use proa_core::WebContext as RenderCx; {src}");
    assert_eq!(
        missing(&src).len(),
        1,
        "unused loader data cannot cover a field"
    );
    assert_eq!(code(&lint(&src), "render/content-coverage-unchecked"), 0);
}
#[test]
fn rendered_loader_and_context_data_need_explicit_correspondence() {
    let src = fixture(
        "let stock = cx.load_typed::<Data>(html_query).await?; html_sync! {{content_part!(\"stock\", badge(stock))}}.render(cx)",
        "let stock = cx.load_typed::<Data>(md_query).await?; md_sync! {r###\"{content_part!(\"stock\", summary(stock))}\"###}.render_md(cx)",
    ).replace("&mut Cx", "&mut proa_core::WebContext");
    assert!(lint(&src).is_empty(), "{:?}", lint(&src));
    for output in [
        "let stock = cx.load_typed::<Data>(query).await?; md_sync! {r###\"{stock}\"###}.render_md(cx)",
        "let response = cx.response(); md_sync! {r###\"{response}\"###}.render_md(cx)",
    ] {
        let src = fixture("html_sync! {{self.title}}.render(cx)", output)
            .replace("&mut Cx", "&mut proa_core::WebContext");
        assert_eq!(code(&lint(&src), "render/content-coverage-unchecked"), 1, "{output}");
        assert!(missing(&src).is_empty());
    }
}
#[test]
fn infrastructure_exemptions_preserve_actual_and_opaque_output() {
    for preparation in [
        "cx.write_md_text(value)?;",
        "cx.output_mut().write(value)?;",
        "write_summary(self.data, cx)?;",
        "cx.set_csp_nonce({ write_summary(self.data, cx)?; None });",
        "cx.locals_mut().insert(|| write_summary(self.data, cx));",
        "cx.response_mut().custom_write(cx);",
        "let loader = cx.loader().clone(); loader.load({ write_summary(self.data, cx)?; query }).await?;",
    ] {
        let src = fixture(
            "html_sync! {{self.title}}.render(cx)",
            &format!("{preparation} md_sync! {{r###\"\"###}}.render_md(cx)"),
        )
        .replace("&mut Cx", "&mut proa_core::WebContext");
        assert_eq!(
            code(&lint(&src), "render/content-coverage-unchecked"),
            1,
            "{preparation}"
        );
        assert!(missing(&src).is_empty(), "{preparation}");
    }
}
#[test]
fn borrowed_loader_arguments_preserve_opaque_output_writes() {
    let source = fixture(
        "html_sync! {{self.title}}.render(cx)",
        "let loader = cx.loader().clone(); let _unused = loader.load(&{ write_summary(self.title, cx)?; request }).await?; md_sync! {r###\"\"###}.render_md(cx)",
    )
    .replace("&mut Cx", "&mut proa_core::WebContext");
    let diagnostics = lint(&source);
    assert_eq!(code(&diagnostics, "render/content-coverage-unchecked"), 1);
    assert_eq!(code(&diagnostics, "render/content-coverage"), 0);
}
#[test]
fn borrowed_callbacks_remain_unchecked_instead_of_confirmed_missing() {
    for call in [
        "invoke(&mut || write_summary(self.title, cx))",
        "handler.invoke(&mut || write_summary(self.title, cx))",
        "let mut callback = || write_summary(self.title, cx); invoke(&mut callback)",
        "let mut callback = || write_summary(self.title, cx); callback()",
    ] {
        let source = fixture("html_sync! {{self.title}}.render(cx)", call)
            .replace("&mut Cx", "&mut proa_core::WebContext");
        let diagnostics = lint(&source);
        assert_eq!(
            code(&diagnostics, "render/content-coverage-unchecked"),
            1,
            "{call}"
        );
        assert_eq!(code(&diagnostics, "render/content-coverage"), 0, "{call}");
    }
}
#[test]
fn familiar_method_names_do_not_exempt_custom_or_shadowed_contexts() {
    let src = fixture(
        "html_sync! {{self.title}}.render(cx)",
        "cx.load(query); md_sync! {r###\"\"###}.render_md(cx)",
    );
    assert_eq!(code(&lint(&src), "render/content-coverage-unchecked"), 1);
    let local = format!("mod proa_core {{ pub struct WebContext; }} {src}")
        .replace("impl WebRenderSync", "impl ::proa_core::WebRenderSync")
        .replace("impl MdRenderSync", "impl ::proa_core::MdRenderSync")
        .replace("&mut Cx", "&mut proa_core::WebContext");
    assert_eq!(
        code(&lint(&local), "render/content-coverage-unchecked"),
        1,
        "{:?}",
        lint(&local)
    );
    let shadowed = fixture(
        "html_sync! {{self.title}}.render(cx)",
        "let cx = custom_context(); cx.load(query); md_sync! {r###\"\"###}.render_md(cx)",
    )
    .replace("&mut Cx", "&mut proa_core::WebContext");
    assert_eq!(
        code(&lint(&shadowed), "render/content-coverage-unchecked"),
        1
    );
    let reassigned = fixture(
        "html_sync! {{self.title}}.render(cx)",
        "let mut locals = cx.locals_mut(); locals = custom_storage(); locals.insert(value); md_sync! {r###\"\"###}.render_md(cx)",
    ).replace("&mut Cx", "&mut proa_core::WebContext");
    assert_eq!(
        code(&lint(&reassigned), "render/content-coverage-unchecked"),
        1
    );
}
#[test]
fn line_omissions_are_precise_and_checked_for_rot() {
    let source = "html_sync! {{RestockNotice::new(self.data)} {Impact::new(self.data)}}.render(cx)";
    let target =
        "omit!(RestockNotice); md_sync! {r###\"{Impact::new(self.data)}\"###}.render_md(cx)";
    assert!(lint(&fixture(source, target)).is_empty());
    let ambiguous = fixture(
        source,
        "omit!(self.data);md_sync! {r###\"\"###}.render_md(cx)",
    );
    assert_eq!(code(&lint(&ambiguous), "render/content-omit-ambiguous"), 1);
    assert_eq!(missing(&ambiguous).len(), 2);
    let stale = fixture(
        "html_sync! {{self.title}}.render(cx)",
        "omit!(self.image);md_sync! {r###\"{self.title}\"###}.render_md(cx)",
    );
    assert_eq!(code(&lint(&stale), "render/content-omit-stale"), 1);
    let redundant = fixture(
        "html_sync! {{self.title}}.render(cx)",
        "omit!(self.title);md_sync! {r###\"{self.title}\"###}.render_md(cx)",
    );
    assert_eq!(code(&lint(&redundant), "render/content-omit-redundant"), 1);
}
#[test]
fn local_and_role_omissions_allow_intentional_differences() {
    let src=fixture("html_sync! {{RestockNotice::new(self.data)}}.render(cx)","let notice=RestockNotice::new(self.data);omit!(&notice, \"Web only\", from=\"html\");md_sync! {r###\"\"###}.render_md(cx)");
    assert!(lint(&src).is_empty());
    let src = fixture(
        "html_sync! {{content_part!(\"notice\", RestockNotice::new(self.data))}}.render(cx)",
        "omit!(\"notice\");md_sync! {r###\"\"###}.render_md(cx)",
    );
    assert!(lint(&src).is_empty());
}
#[test]
fn named_parts_map_transforms_and_static_copy() {
    let src = fixture(
        "html_sync! {{content_part!(\"status\", badge(self.stock))}}.render(cx)",
        "md_sync! {r###\"{content_part!(\"status\", status_text(self.stock))}\"###}.render_md(cx)",
    );
    assert!(lint(&src).is_empty());
    let src = fixture(
        "html_sync! {{content_part!(\"notice\", text(\"Available soon\"))}}.render(cx)",
        "md_sync! {r###\"\"###}.render_md(cx)",
    );
    assert_eq!(missing(&src).len(), 1);
}
#[test]
fn ordinary_static_copy_does_not_hide_a_missing_dynamic_child() {
    let src = fixture(
        "html_sync! { \"Notice\" {RestockNotice::new(self.data)}}.render(cx)",
        "md_sync! {r###\"# Product\"###}.render_md(cx)",
    );
    assert_eq!(missing(&src).len(), 2);
    let static_only = fixture(
        "html_sync! {\"Available\"}.render(cx)",
        "md_sync! {r###\"\"###}.render_md(cx)",
    );
    assert_eq!(code(&lint(&static_only), "render/content-coverage"), 1);
}
#[test]
fn semantic_attributes_count_styling_does_not() {
    let src = fixture(
        "html_sync! {<img class={self.title} alt={self.image} />}.render(cx)",
        "md_sync! {r###\"{self.image}\"###}.render_md(cx)",
    );
    assert!(lint(&src).is_empty());
    let omitted = src.replace("{self.image}\"###", "\"###");
    assert_eq!(missing(&omitted).len(), 1);
}
#[test]
fn adapter_and_actual_sink_initializer_are_supported() {
    let src=fixture("let output=html_sync! {{text(self.title)}};let result=output.render(cx);result", "let output=md_sync! {r###\"{text(self.title)}\"###};let result=output.render_md(cx);result");
    assert!(lint(&src).is_empty());
    let src = fixture(
        "html! {{to_async(RestockNotice::new(self.data))}}.render(cx)",
        "md! {r###\"{to_md_async(RestockNotice::new(self.data))}\"###}.render_md(cx)",
    );
    assert!(lint(&src).is_empty());
}
#[test]
fn loop_elements_are_not_a_collection_length() {
    let src = fixture(
        "html_sync! {{for item in self.items {html_sync! {{item}}}}}.render(cx)",
        "md_sync! {r###\"{for item in self.items {{item}}}\"###}.render_md(cx)",
    );
    assert!(lint(&src).is_empty());
    let bad = src.replace(
        "{for item in self.items {{item}}}\"###",
        "{self.items.len()}\"###",
    );
    assert_eq!(missing(&bad).len(), 1);
}
#[test]
fn whole_view_skip_on_impl_or_method_and_single_target_are_valid() {
    for attr in [
        "#[proa_macros::content(skip)] impl MdRenderSync",
        "impl MdRenderSync",
    ] {
        let mut src = fixture(
            "html_sync! {{self.title}}.render(cx)",
            "md_sync! {r###\"\"###}.render_md(cx)",
        );
        src = src.replace("impl MdRenderSync", attr);
        if attr == "impl MdRenderSync" {
            src = src.replace("fn render_md", "#[proa_macros::content(skip)] fn render_md");
        }
        assert!(lint(&src).is_empty());
    }
    let src=format!("{IMPORTS} struct OnlyMd;impl MdRenderSync for OnlyMd {{fn render_md(self,cx:&mut Cx)->Result {{md_sync! {{r###\"\"###}}.render_md(cx)}}}}");
    assert!(lint(&src).is_empty());
    let html = fixture(
        "html_sync! {{self.title}}.render(cx)",
        "md_sync! {r###\"\"###}.render_md(cx)",
    );
    let html = html.split("impl MdRenderSync").next().unwrap();
    assert!(lint(html).is_empty());
}
#[test]
fn removed_counterpart_requirement_is_rejected_as_an_unknown_option() {
    let source = fixture(
        "html_sync! {{self.title}}.render(cx)",
        "md_sync! {r###\"{self.title}\"###}.render_md(cx)",
    )
    .replace(
        "impl WebRenderSync",
        "#[proa_macros::content(require=\"markdown\")] impl WebRenderSync",
    );
    let diagnostics = lint(&source);
    assert_eq!(code(&diagnostics, "render/content-policy-invalid"), 1);
    assert!(diagnostics[0]
        .message
        .contains("expected `skip`, `reason`, `source`, or `from`"));
}
#[test]
fn registered_future_target_reuses_coverage_and_omissions() {
    let mut options = ContentOptions::default();
    let mut target = options.targets[1].clone();
    target.name = "text".into();
    target.display_name = "Text".into();
    target.traits = vec!["my_render::TextRender".into()];
    target.methods = vec!["render_text".into()];
    target.templates = vec!["my_render::plain".into()];
    options.targets.push(target);
    options.comparisons.push(("html".into(), "text".into()));
    let src = fixture(
        "html_sync! {{self.title}}.render(cx)",
        "md_sync! {r###\"{self.title}\"###}.render_md(cx)",
    );
    let src=format!("{src}\nimpl my_render::TextRender for Product {{fn render_text(self,cx:&mut Cx)->Result {{my_render::plain! {{r###\"\"###}}.render_text(cx)}}}}");
    // External qualified registrations must not need suffix guessing.
    let path = PathBuf::from("/coverage/src/lib.rs");
    let diags = lint_sources(
        &[(path.clone(), src)],
        &BTreeSet::from([path]),
        false,
        &options,
    );
    assert_eq!(code(&diags, "render/content-coverage"), 1);
    assert!(diags[0].message.contains("Text"));
}

#[test]
fn child_local_does_not_waive_another_child_sharing_data() {
    let src=fixture("html_sync! {{RestockNotice::new(self.data)} {Impact::new(self.data)}}.render(cx)","let notice=RestockNotice::new(self.data);omit!(&notice);md_sync! {r###\"\"###}.render_md(cx)");
    let diags = missing(&src);
    assert_eq!(diags.len(), 1);
    assert!(diags[0].message.contains("Impact"));
    assert_eq!(code(&lint(&src), "render/content-omit-ambiguous"), 0);
}
#[test]
fn raw_literals_named_args_and_unicode_keep_authored_ranges() {
    let src = fixture(
        "html_sync! {{self.title}}.render(cx)",
        "md_sync! {r###\"\n    🦀 {self.title}\n    \"###}.render_md(cx)",
    );
    let source = src.replace("{self.title}\n", "{self.image}\n");
    let diags = missing(&source);
    assert_eq!(diags.len(), 1);
    let label = &diags[0].labels[1];
    assert!(label.source.iter().any(|l| l.text.contains("self.title")));
    let guards = fixture(
        "html_sync! {{if self.stock==0 {html_sync! {{self.title}}}}}.render(cx)",
        "md_sync! {r###\"\n    🦀 {if self.stock>0 {{self.title}}}\n    \"###}.render_md(cx)",
    );
    let diags = lint(&guards);
    let primary = &diags
        .iter()
        .find(|d| d.code == "render/content-coverage-unchecked")
        .unwrap()
        .labels[0];
    let text = guards.lines().nth(primary.range.line - 1).unwrap();
    assert_eq!(
        text.chars()
            .skip(primary.range.column - 1)
            .take(primary.range.end_column - primary.range.column)
            .collect::<String>(),
        "self.stock>0"
    );
    let named = fixture(
        "html_sync! {{self.title}}.render(cx)",
        "md_sync! {r###\"{title}\"###,title=self.title}.render_md(cx)",
    );
    assert!(lint(&named).is_empty());
}
#[test]
fn code_literal_braces_are_never_dynamic_coverage() {
    let src = fixture(
        "html_sync! {{self.title}}.render(cx)",
        "md_sync! {r###\"`{self.title}`\"###}.render_md(cx)",
    );
    assert_eq!(missing(&src).len(), 1);
}
#[test]
fn registered_traits_resolve_aliases_not_similar_suffixes() {
    let src = fixture(
        "html_sync! {{self.title}}.render(cx)",
        "md_sync! {r###\"\"###}.render_md(cx)",
    )
    .replace(
        "use proa_core::{",
        "use proa_core::MdRenderSync as Markdown; use proa_core::{",
    )
    .replace("impl MdRenderSync", "impl Markdown");
    assert_eq!(missing(&src).len(), 1);
    let false_trait = src.replace("impl Markdown", "impl OtherMdRenderSync");
    assert!(lint(&false_trait).is_empty());
}
fn multi(files: &[(&str, &str)], selected: &[&str]) -> Vec<Diagnostic> {
    let sources = files
        .iter()
        .map(|(p, s)| (PathBuf::from(format!("/fixture/{p}")), s.to_string()))
        .collect::<Vec<_>>();
    let reporting = selected
        .iter()
        .map(|p| PathBuf::from(format!("/fixture/{p}")))
        .collect();
    lint_sources(&sources, &reporting, false, &ContentOptions::default())
}
#[test]
fn changed_file_pairs_unchanged_counterpart_and_modules_do_not_collide() {
    let root = "mod product;mod html;mod markdown;mod unrelated;";
    let product = "pub struct Product;";
    let html=format!("{IMPORTS} use crate::product::Product;impl WebRenderSync for Product {{fn render(self,cx:&mut Cx)->Result{{html_sync! {{{{content_part!(\"notice\", text(\"available\"))}}}}.render(cx)}}}}");
    let md=format!("{IMPORTS} use crate::product::Product;impl MdRenderSync for Product {{fn render_md(self,cx:&mut Cx)->Result{{md_sync! {{r###\"\"###}}.render_md(cx)}}}}");
    let unrelated=format!("{IMPORTS} struct Product;impl MdRenderSync for Product {{fn render_md(self,cx:&mut Cx)->Result{{md_sync! {{r###\"\"###}}.render_md(cx)}}}}");
    let files = [
        ("src/lib.rs", root),
        ("src/product.rs", product),
        ("src/html.rs", html.as_str()),
        ("src/markdown.rs", md.as_str()),
        ("src/unrelated.rs", unrelated.as_str()),
    ];
    let diags = multi(&files, &["src/markdown.rs"]);
    assert_eq!(code(&diags, "render/content-coverage"), 1);
    assert_eq!(
        diags[0].labels[1].range.path,
        PathBuf::from("/fixture/src/html.rs")
    );
    assert_eq!(
        code(&multi(&files, &["src/html.rs"]), "render/content-coverage"),
        1
    );
    assert!(multi(&files, &["src/unrelated.rs"]).is_empty());
}
#[test]
fn separate_types_require_named_semantic_mapping() {
    let src=format!("{IMPORTS} struct HtmlProduct;struct MdProduct;impl WebRenderSync for HtmlProduct {{fn render(self,cx:&mut Cx)->Result{{html_sync! {{{{content_part!(\"notice\", text(\"Available\"))}}}}.render(cx)}}}} #[proa_macros::content(source=HtmlProduct,from=\"html\")] impl MdRenderSync for MdProduct {{fn render_md(self,cx:&mut Cx)->Result{{md_sync! {{r###\"{{content_part!(\"notice\", text(\"Available\"))}}\"###}}.render_md(cx)}}}}");
    assert_eq!(code(&lint(&src), "render/content-coverage"), 0);
    assert_eq!(code(&lint(&src), "render/content-coverage-unchecked"), 0);
    let bad = src.replace(
        "{content_part!(\"notice\", text(\"Available\"))}\"###",
        "\"###",
    );
    assert_eq!(missing(&bad).len(), 1);
}
#[test]
fn configuration_and_generic_identity_do_not_guess() {
    let src = fixture(
        "html_sync! {{self.title}}.render(cx)",
        "md_sync! {r###\"{self.title}\"###}.render_md(cx)",
    )
    .replace(
        "impl MdRenderSync",
        "#[cfg(feature=\"md\")] impl MdRenderSync",
    );
    assert_eq!(code(&lint(&src), "render/content-coverage-unchecked"), 1);
    let generic=format!("{IMPORTS} struct Product<T> {{value:T}} impl<L,T> WebRenderSync<L> for Product<T> {{fn render(self,cx:&mut Cx)->Result{{html_sync! {{{{self.value}}}}.render(cx)}}}} impl<X,U> MdRenderSync<X> for Product<U> {{fn render_md(self,cx:&mut Cx)->Result{{md_sync! {{r###\"{{self.value}}\"###}}.render_md(cx)}}}}");
    assert!(lint(&generic).is_empty());
    let refs = fixture(
        "html_sync! {{self.title}}.render(cx)",
        "md_sync! {r###\"\"###}.render_md(cx)",
    )
    .replace("MdRenderSync for Product", "MdRenderSync for &Product");
    assert!(lint(&refs).is_empty());
}

#[test]
fn incomplete_module_graph_does_not_create_a_counterpart_requirement() {
    for (render_trait, method, template) in [
        ("WebRenderSync", "render", "html_sync! {{self.title}}"),
        (
            "MdRenderSync",
            "render_md",
            "md_sync! {r###\"{self.title}\"###}",
        ),
    ] {
        let source = format!("{IMPORTS} mod missing;struct Product {{title:String}} impl {render_trait} for Product {{fn {method}(self,cx:&mut Cx)->Result {{{template}.{method}(cx)}}}}");
        assert!(lint(&source).is_empty());
    }
}
#[test]
fn nested_inline_module_resolves_out_of_line_counterpart() {
    let root=format!("{IMPORTS} pub struct Product;impl WebRenderSync for Product {{fn render(self,cx:&mut Cx)->Result{{html_sync! {{{{content_part!(\"notice\",text(\"Notice\"))}}}}.render(cx)}}}} mod views {{mod markdown;}}");
    let md=format!("{IMPORTS} use crate::Product;impl MdRenderSync for Product {{fn render_md(self,cx:&mut Cx)->Result{{md_sync! {{r###\"\"###}}.render_md(cx)}}}}");
    assert_eq!(
        code(
            &multi(
                &[("src/lib.rs", &root), ("src/views/markdown.rs", &md)],
                &["src/views/markdown.rs"]
            ),
            "render/content-coverage"
        ),
        1
    );
}
#[test]
fn unknown_selector_is_distinct_from_stale_selector() {
    let src = fixture(
        "html_sync! {{self.title}}.render(cx)",
        "omit!(&missing);md_sync! {r###\"\"###}.render_md(cx)",
    );
    assert_eq!(code(&lint(&src), "render/content-omit-unresolved"), 1);
    assert_eq!(code(&lint(&src), "render/content-omit-stale"), 0);
}

#[test]
fn async_outcomes_analyze_polled_output_but_unused_futures_never_count() {
    let src=fixture("proa_core::RenderOutcome::pending(async move {html! {{self.title}}.render(cx).await})","proa_core::RenderOutcome::pending(async move {md! {r###\"{self.title}\"###}.render_md(cx).await})");
    assert!(lint(&src).is_empty());
    let bad=src.replace("proa_core::RenderOutcome::pending(async move {md! {r###\"{self.title}\"###}.render_md(cx).await})","let future=async move {md! {r###\"{self.title}\"###}.render_md(cx).await};md_sync! {r###\"\"###}.render_md(cx)");
    assert_eq!(missing(&bad).len(), 1);
}
#[test]
fn another_grammar_registers_a_parser_and_reuses_targeted_omissions() {
    fn parser(tokens: proc_macro2::TokenStream) -> syn::Result<Vec<proa_dsl::md::TplNode>> {
        let expr: syn::Expr = syn::parse2(tokens)?;
        use syn::spanned::Spanned;
        let span = expr.span();
        Ok(vec![proa_dsl::md::TplNode::Expr(
            proa_dsl::md::TemplateExpr {
                expr,
                location: proa_dsl::md::ExprLocation::Rust(span),
            },
        )])
    }
    let mut options = ContentOptions::default();
    let mut t = options.targets[1].clone();
    t.name = "plain".into();
    t.display_name = "Plain text".into();
    t.traits = vec!["plain::PlainRender".into()];
    t.methods = vec!["render_plain".into()];
    t.templates = vec!["plain::plain".into()];
    t.syntax = TemplateSyntax::Custom(parser);
    options.targets.push(t);
    options.comparisons = vec![("html".into(), "plain".into())];
    let src=format!("{} impl plain::PlainRender for Product {{fn render_plain(self,cx:&mut Cx)->Result{{plain::plain! {{self.title}}.render_plain(cx)}}}}",fixture("html_sync! {{self.title} {RestockNotice::new(self.data)}}.render(cx)","md_sync! {r###\"\"###}.render_md(cx)"));
    let p = PathBuf::from("/fixture/src/lib.rs");
    let run = |s: String| {
        lint_sources(
            &[(p.clone(), s)],
            &BTreeSet::from([p.clone()]),
            false,
            &options,
        )
    };
    assert_eq!(code(&run(src.clone()), "render/content-coverage"), 1);
    let intentional = src.replace(
        "plain::plain! {self.title}",
        "omit!(RestockNotice,from=\"html\");plain::plain! {self.title}",
    );
    assert!(run(intentional).is_empty());
}

#[test]
fn constructor_argument_roles_and_literals_are_not_field_set_equality() {
    for (html, md) in [
        (
            "RestockNotice::new(self.data,true)",
            "RestockNotice::new(self.data,false)",
        ),
        (
            "RestockNotice {at:self.title,description:self.image}",
            "RestockNotice {at:self.image,description:self.title}",
        ),
    ] {
        let src = fixture(
            &format!("html_sync! {{{{{html}}}}}.render(cx)"),
            &format!("md_sync! {{r###\"{{{md}}}\"###}}.render_md(cx)"),
        );
        assert_eq!(code(&lint(&src), "render/content-coverage-unchecked"), 1);
        assert!(missing(&src).is_empty());
    }
}
#[test]
fn generic_non_render_bounds_and_execution_variants_are_explicit() {
    let src=format!("{IMPORTS} struct Product<T>{{value:T}} impl<T:Clone> WebRenderSync for Product<T>{{fn render(self,cx:&mut Cx)->Result{{html_sync! {{{{self.value}}}}.render(cx)}}}} impl<U:Copy> MdRenderSync for Product<U>{{fn render_md(self,cx:&mut Cx)->Result{{md_sync! {{r###\"{{self.value}}\"###}}.render_md(cx)}}}}");
    assert_eq!(code(&lint(&src), "render/content-coverage-unchecked"), 1);
    let src = fixture(
        "html_sync! {{self.title}}.render(cx)",
        "md_sync! {r###\"{self.title}\"###}.render_md(cx)",
    );
    let variant="impl WebRenderSend for Product {fn render_send(self,cx:&mut Cx)->Result{html_sync! {{self.title}}.render_send(cx)}}";
    assert!(lint(&format!("{src}{variant}")).is_empty());
    let different = variant.replace("self.title", "self.image");
    assert_eq!(
        code(
            &lint(&format!("{src}{different}")),
            "render/content-coverage-unchecked"
        ),
        1
    );
}
#[test]
fn macro_declared_components_are_identified_from_lexical_render_impls() {
    let src=fixture("html_sync! {{RestockNotice::new(self.data)}}.render(cx)","md_sync! {r###\"\"###}.render_md(cx)").replace("struct Product {title: String, data: Data, stock:u32, items: Vec<Data>, image: String}","component!(Product);").replace("struct RestockNotice;","component!(RestockNotice);impl WebRenderSync for RestockNotice {fn render(self,cx:&mut Cx)->Result{html_sync! {}.render(cx)}}");
    assert_eq!(missing(&src).len(), 1);
}

#[test]
fn skip_is_available_for_unresolved_types_and_status_is_honest() {
    let src=format!("{IMPORTS} use external::Product;#[proa_macros::content(skip)] impl MdRenderSync for Product {{fn render_md(self,cx:&mut Cx)->Result{{summary(self).render_md(cx)}}}}");
    assert!(lint(&src).is_empty());
    let options = ContentOptions {
        report_status: true,
        ..Default::default()
    };
    let p = PathBuf::from("/fixture/src/lib.rs");
    let d = lint_sources(&[(p.clone(), src)], &BTreeSet::from([p]), false, &options);
    assert_eq!(code(&d, "render/content-status"), 1);
    assert!(d[0].message.contains("exempt"));
}
#[test]
fn removing_source_impl_makes_its_omission_stale_without_forcing_a_pair() {
    let src=format!("{IMPORTS} struct Product;impl MdRenderSync for Product{{fn render_md(self,cx:&mut Cx)->Result{{omit!(\"notice\");md_sync!{{r###\"\"###}}.render_md(cx)}}}}");
    let d = lint(&src);
    assert_eq!(code(&d, "render/content-omit-stale"), 1);
    assert_eq!(d.len(), 1);
}
#[test]
fn package_entry_points_keep_equal_names_separate() {
    let a = fixture(
        "html_sync! {{self.title}}.render(cx)",
        "md_sync! {r###\"{self.title}\"###}.render_md(cx)",
    );
    let b=format!("{IMPORTS} struct Product;impl MdRenderSync for Product{{fn render_md(self,cx:&mut Cx)->Result{{md_sync! {{r###\"\"###}}.render_md(cx)}}}}");
    let files = [("a/src/lib.rs", a.as_str()), ("b/src/lib.rs", b.as_str())];
    assert!(multi(&files, &["b/src/lib.rs"]).is_empty());
}
#[test]
fn actual_product_fixture_and_real_incident_report_deletions_are_caught() {
    let path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../proa_macros/tests/content_coverage.rs");
    let source = std::fs::read_to_string(&path).unwrap();
    let mut options = ContentOptions::default();
    options.root = Some(path.parent().unwrap().into());
    let reporting = BTreeSet::from([discover::absolute(&path)]);
    let pristine = lint_sources(
        &[(path.clone(), source.clone())],
        &reporting,
        true,
        &options,
    );
    assert_eq!(code(&pristine, "render/content-coverage"), 0);
    assert_eq!(code(&pristine, "render/content-coverage-unchecked"), 0);
    let mutated = source.replace("{restock}", "");
    let d = lint_sources(&[(path.clone(), mutated)], &reporting, true, &options);
    assert_eq!(code(&d, "render/content-coverage"), 1);
    assert!(d[0].message.contains("RestockNotice"));
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../examples/mcp-incident-desk/src/components/issue_report/issue_report.rs");
    let source = std::fs::read_to_string(&path).unwrap();
    assert!(source.contains("{impact}"));
    let mutated = source.replace("{impact}", "");
    let d = lint_sources(
        &[(path.clone(), mutated)],
        &BTreeSet::from([discover::absolute(&path)]),
        false,
        &ContentOptions::default(),
    );
    assert!(d.iter().any(|d| d.code == "render/content-coverage"
        && d.message.contains("IssueReport")
        && d.message.contains("Impact")));
}
#[test]
fn rust_and_cooked_markdown_ranges_select_authored_conditions() {
    let src = fixture(
        "html_sync! {{if self.stock==0 {html_sync! {{self.title}}}}}.render(cx)",
        "md_sync! {\"\\n    🦀 {if self.stock>0 {{self.title}}}\\n\"}.render_md(cx)",
    );
    let d = lint(&src);
    let d = d
        .iter()
        .find(|d| d.code == "render/content-coverage-unchecked")
        .unwrap();
    let range = &d.labels[0].range;
    let line = src.lines().nth(range.line - 1).unwrap();
    assert_eq!(
        line.chars()
            .skip(range.column - 1)
            .take(range.end_column - range.column)
            .collect::<String>(),
        "self.stock>0"
    );
}

#[test]
fn guard_aliases_keep_definition_time_provenance_and_helpers_stay_unknown() {
    let src = fixture(
        "let show=self.stock==0;html_sync! {{if show {html_sync! {{self.title}}}}}.render(cx)",
        "let visible=self.stock==0;md_sync! {r###\"{if visible {{self.title}}}\"###}.render_md(cx)",
    );
    assert!(lint(&src).is_empty());
    let opaque=fixture("let show=first_policy(self.stock);html_sync! {{if show {html_sync! {{self.title}}}}}.render(cx)","let show=second_policy(self.stock);md_sync! {r###\"{if show {{self.title}}}\"###}.render_md(cx)");
    assert_eq!(code(&lint(&opaque), "render/content-coverage-unchecked"), 1);
    let shadowed=fixture("html_sync! {{if self.stock==0 {html_sync! {{self.title}}}}}.render(cx)","let stock=self.stock;let show=stock==0;let stock=self.image;md_sync! {r###\"{if show {{self.title}}}\"###}.render_md(cx)");
    assert!(lint(&shadowed).is_empty());
}
#[test]
fn missing_child_primary_is_the_template_and_alias_evidence_is_at_the_sink() {
    let src = fixture(
        "let child=RestockNotice::new(self.data);html_sync! {{child}}.render(cx)",
        "let notice=RestockNotice::new(self.data);md_sync! {r###\"\"###}.render_md(cx)",
    );
    let d = missing(&src);
    assert_eq!(d.len(), 1);
    let start = &d[0].labels[0].range;
    let primary = src
        .lines()
        .nth(start.line - 1)
        .unwrap()
        .chars()
        .skip(start.column - 1)
        .take(8)
        .collect::<String>();
    assert_eq!(primary, "md_sync!");
    let evidence = &d[0].labels[1].range;
    let text = src
        .lines()
        .nth(evidence.line - 1)
        .unwrap()
        .chars()
        .skip(evidence.column - 1)
        .take(evidence.end_column - evidence.column)
        .collect::<String>();
    assert_eq!(text, "child");
}

#[test]
fn named_parts_retain_structural_guards_and_can_map_direct_writes() {
    let src=fixture("html_sync! {{content_part!(\"notice\", if self.stock==0 {html_sync! {{self.title}}}else{()})}}.render(cx)","md_sync! {r###\"{content_part!(\"notice\", if self.stock>0 {md_sync! {r#\"{self.title}\"#}}else{()})}\"###}.render_md(cx)");
    assert_eq!(code(&lint(&src), "render/content-coverage-unchecked"), 1);
    let direct = fixture(
        "content_part!(\"notice\", write_html(self.data,cx));Ok(())",
        "content_part!(\"notice\", write_md(self.data,cx));Ok(())",
    );
    assert!(lint(&direct).is_empty());
    let unused = direct.replace(
        "content_part!(\"notice\", write_md(self.data,cx));Ok(())",
        "let unused=content_part!(\"notice\", md_sync! {r###\"{self.title}\"###});Ok(())",
    );
    assert_eq!(missing(&unused).len(), 1);
}
#[test]
fn one_opaque_boundary_retains_evidence_for_all_unchecked_children() {
    let src = fixture(
        "html_sync! {{RestockNotice::new(self.data)} {Impact::new(self.data)}}.render(cx)",
        "summary(self.data).render_md(cx)",
    );
    let d = lint(&src);
    assert_eq!(d.len(), 1);
    assert_eq!(d[0].labels.len(), 3);
    assert!(d[0].message.contains("2 HTML contributions"));
    assert!(d[0].labels[1].message.contains("RestockNotice"));
    assert!(d[0].labels[2].message.contains("Impact"));
}

#[test]
fn loop_exits_compound_assignment_and_obvious_mutation_are_unchecked() {
    let html = "html_sync! {{for item in self.items {html_sync! {{item}}}}}.render(cx)";
    let md="md_sync! { {for item in self.items { if item.stop {break;} md_sync! {{item}} }} }.render_md(cx)";
    assert_eq!(
        code(
            &lint(&fixture(html, md)),
            "render/content-coverage-unchecked"
        ),
        1
    );
    let src = fixture(
        "html_sync! {{self.stock}}.render(cx)",
        "let stock=self.stock;stock+=1;md_sync! {r###\"{stock}\"###}.render_md(cx)",
    );
    assert_eq!(code(&lint(&src), "render/content-coverage-unchecked"), 1);
    let src = fixture(
        "html_sync! {{self.title}}.render(cx)",
        "let title=self.title.clone();title.push('x');md_sync! {r###\"{title}\"###}.render_md(cx)",
    );
    assert_eq!(code(&lint(&src), "render/content-coverage-unchecked"), 1);
}
#[test]
fn item_macro_discovery_does_not_create_a_counterpart_requirement() {
    let source=format!("{IMPORTS} struct Product;generate_md!(Product);impl WebRenderSync for Product{{fn render(self,cx:&mut Cx)->Result{{html_sync! {{}}.render(cx)}}}}");
    assert!(lint(&source).is_empty());
}

#[test]
fn child_presentation_fields_can_differ_without_changing_content_inputs() {
    let source = fixture(
        "html_sync! {{RestockNotice {data:self.data,class:self.image}}}.render(cx)",
        "md_sync! {r###\"{RestockNotice {data:self.data,class:None}}\"###}.render_md(cx)",
    );
    assert!(lint(&source).is_empty());
}
#[test]
fn missing_field_repair_names_the_field_or_its_local_instead_of_self() {
    let source = fixture(
        "html_sync! {<img src={self.image} alt=\"\" />}.render(cx)",
        "md_sync! {r###\"{self.title}\"###}.render_md(cx)",
    );
    let diagnostics = missing(&source);
    assert_eq!(diagnostics.len(), 1);
    assert_eq!(
        diagnostics[0].help.as_deref(),
        Some("render `self.image` in the Markdown view")
    );
    let with_local = source.replace(
        "md_sync! {r###\"{self.title}\"###}",
        "let image=self.image;md_sync! {r###\"{self.title}\"###}",
    );
    let diagnostics = missing(&with_local);
    assert_eq!(diagnostics.len(), 1);
    assert_eq!(
        diagnostics[0].help.as_deref(),
        Some("add `{image}` to the Markdown template; constructing or testing a value does not render it")
    );
}

#[test]
fn omission_suggestions_never_use_method_calls_or_collection_pseudo_paths() {
    let source = fixture(
        "html_sync! {{self.items.len()}}.render(cx)",
        "let count=self.items.len();md_sync! {r###\"\"###}.render_md(cx)",
    );
    let d = missing(&source);
    assert_eq!(d.len(), 1);
    for suggestion in &d[0].suggestions {
        let tokens = suggestion
            .replacement
            .trim()
            .strip_prefix("proa_macros::omit!(")
            .unwrap()
            .strip_suffix(");")
            .unwrap();
        assert!(syn::parse_str::<proa_dsl::content::OmitContent>(tokens).is_ok());
    }
    let field = fixture(
        "html_sync! {{self.title}}.render(cx)",
        "md_sync! {r###\"\"###}.render_md(cx)",
    );
    let d = missing(&field);
    assert_eq!(
        d[0].suggestions[0].replacement,
        "proa_macros::omit!(self.title);"
    );
}

#[test]
fn nested_library_named_module_is_not_another_cargo_entry_point() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(temp.path().join("src/view")).unwrap();
    std::fs::write(
        temp.path().join("Cargo.toml"),
        "[package]\nname=\"fixture\"\nversion=\"0.1.0\"\n",
    )
    .unwrap();
    let root=format!("{IMPORTS} pub struct Product;impl WebRenderSync for Product{{fn render(self,cx:&mut Cx)->Result{{html_sync! {{{{content_part!(\"notice\",text(\"Notice\"))}}}}.render(cx)}}}} #[path=\"view/lib.rs\"] mod view;");
    let view=format!("{IMPORTS} use crate::Product;impl MdRenderSync for Product{{fn render_md(self,cx:&mut Cx)->Result{{md_sync! {{r###\"\"###}}.render_md(cx)}}}}");
    std::fs::write(temp.path().join("src/lib.rs"), root).unwrap();
    let path = temp.path().join("src/view/lib.rs");
    std::fs::write(&path, view).unwrap();
    let d = super::lint_paths(&[path], false, &ContentOptions::default()).unwrap();
    assert_eq!(code(&d, "render/content-coverage"), 1);
    assert_eq!(code(&d, "render/content-coverage-unchecked"), 0);
}

#[test]
fn destructuring_preserves_individual_inputs_and_condition_identity() {
    let src = fixture(
        "html_sync! {{self.title} {self.image}}.render(cx)",
        "let (title,image)=(self.title,self.image);md_sync! {r###\"{image}\"###}.render_md(cx)",
    );
    let d = missing(&src);
    assert_eq!(d.len(), 1);
    assert!(d[0].message.contains("self.title"));
    let src=fixture("let Product{stock,..}=self;html_sync! {{if stock==0 {html_sync! {{self.title}}}}}.render(cx)","let Product{stock,..}=self;md_sync! {r###\"{if stock==0 {{self.title}}}\"###}.render_md(cx)");
    assert!(lint(&src).is_empty());
    let src=fixture("html_sync! {{self.title}}.render(cx)","let RestockNotice{at}=RestockNotice {at:self.title};md_sync! {r###\"{at}\"###}.render_md(cx)");
    assert!(lint(&src).is_empty());
    let opaque=fixture("html_sync! {{RestockNotice::new(self.data)}}.render(cx)","let RestockNotice{at}=RestockNotice::new(self.data);md_sync! {r###\"{at}\"###}.render_md(cx)");
    assert_eq!(code(&lint(&opaque), "render/content-coverage-unchecked"), 1);
}

#[test]
fn unsupported_shadowing_patterns_never_reuse_outer_variable_provenance() {
    let src=fixture("html_sync! {{self.title}}.render(cx)","let title=self.title;let [title,..]=opaque(self.items);md_sync! {r###\"{title}\"###}.render_md(cx)");
    assert_eq!(code(&lint(&src), "render/content-coverage-unchecked"), 1);
    assert!(missing(&src).is_empty());
}
#[test]
fn an_unresolved_single_target_is_not_a_counterpart_warning() {
    let src=format!("{IMPORTS} use external::Product;impl MdRenderSync for Product{{fn render_md(self,cx:&mut Cx)->Result{{summary(self).render_md(cx)}}}}");
    assert!(lint(&src).is_empty());
}

#[test]
fn opaque_tuple_destructuring_does_not_render_the_whole_constructed_child() {
    let source = fixture(
        "html_sync! {{RestockNotice::new(self.data)}}.render(cx)",
        "let (value,)=RestockNotice::new(self.data);md_sync! {r###\"{value}\"###}.render_md(cx)",
    );
    assert_eq!(code(&lint(&source), "render/content-coverage-unchecked"), 1);
}

#[test]
fn locally_shadowed_render_trait_paths_are_not_registered_external_targets() {
    let source=format!("{IMPORTS} mod proa_core {{pub trait WebRenderSync{{}} pub trait MdRenderSync{{}}}} struct Product;impl proa_core::WebRenderSync for Product{{fn render(self,cx:&mut Cx)->Result{{html_sync! {{{{content_part!(\"notice\",text(\"Notice\"))}}}}.render(cx)}}}} impl proa_core::MdRenderSync for Product{{fn render_md(self,cx:&mut Cx)->Result{{md_sync! {{r###\"\"###}}.render_md(cx)}}}}");
    assert!(lint(&source).is_empty());
}

#[test]
fn unresolved_render_contexts_and_context_presence_are_not_coverage_proofs() {
    let source = fixture(
        "html_sync! {{self.title}}.render(cx)",
        "let wrapper=Some(cx);md_sync! {r###\"{self.title}\"###}.render_md(wrapper.unwrap())",
    );
    assert_eq!(code(&lint(&source), "render/content-coverage-unchecked"), 1);
    assert!(missing(&source).is_empty());
    let source = fixture(
        "html_sync! {{if cx.show {html_sync! {{self.title}}}}}.render(cx)",
        "md_sync! {r###\"{if cx.show {{self.title}}}\"###}.render_md(cx)",
    );
    assert_eq!(code(&lint(&source), "render/content-coverage-unchecked"), 1);
}

#[test]
fn child_types_resolve_imports_without_confusing_value_bindings() {
    let source = "html_sync! {{RestockNotice::new(self.data)}}.render(cx)";
    let target = "omit!(Notice);md_sync! {r###\"\"###}.render_md(cx)";
    let valid = format!(
        "use crate::RestockNotice as Notice;{}",
        fixture(source, target)
    );
    assert!(lint(&valid).is_empty());
    let unrelated = valid.replace(
        "use crate::RestockNotice as Notice;",
        "use crate::Impact as Notice;",
    );
    assert_eq!(code(&lint(&unrelated), "render/content-omit-stale"), 1);
    assert_eq!(code(&lint(&unrelated), "render/content-coverage"), 1);
    // A value with this name does not change which type the marker selects.
    let shadowed = valid.replace("omit!", "let Notice=|| ();omit!");
    assert!(lint(&shadowed).is_empty());
    let unresolved = valid.replace("omit!(Notice)", "omit!(MissingNotice)");
    assert_eq!(
        code(&lint(&unresolved), "render/content-omit-unresolved"),
        1
    );
    assert_eq!(code(&lint(&unresolved), "render/content-coverage"), 1);
}

#[test]
fn child_type_suggestions_include_lifetimes_and_never_guess_generic_arguments() {
    let source = fixture(
        "html_sync! {{RestockNotice::new(self.data)}}.render(cx)",
        "md_sync! {r###\"\"###}.render_md(cx)",
    );
    let lifetimes = source.replace(
        "struct RestockNotice;",
        "struct RestockNotice<'a>{at:&'a str}",
    );
    let d = missing(&lifetimes);
    assert_eq!(
        d[0].suggestions[0].replacement,
        "proa_macros::omit!(crate::RestockNotice<'_>);"
    );
    let generics = source.replace("struct RestockNotice;", "struct RestockNotice<T>{value:T}");
    assert!(missing(&generics)[0].suggestions.is_empty());
    let attempted = generics.replace("md_sync!", "omit!(RestockNotice<String>);md_sync!");
    assert_eq!(code(&lint(&attempted), "render/content-omit-unresolved"), 1);
    assert_eq!(missing(&attempted).len(), 1);

    let local = generics.replace(
        "md_sync!",
        "let notice=RestockNotice::new(self.data);md_sync!",
    );
    assert_eq!(
        missing(&local)[0].suggestions[0].replacement,
        "proa_macros::omit!(&notice);"
    );
    let d = missing(&local);
    let fix = &d[0].suggestions[0];
    assert!(fix.message.contains("after `notice` is declared"));
    let offset = crate::source::SourceMap::new(&local)
        .byte_for_line_column(fix.range.line, fix.range.column - 1)
        .unwrap();
    let mut acknowledged = local.clone();
    acknowledged.insert_str(offset, &fix.replacement);
    assert_eq!(code(&lint(&acknowledged), "render/content-coverage"), 0);
    assert_eq!(
        code(&lint(&acknowledged), "render/content-omit-unresolved"),
        0
    );
}

#[test]
fn literal_copy_matches_markdown_blocks_and_links_without_annotations() {
    let source = fixture(
        r#"html_sync! { <h1>"Available soon"</h1><a href="/stock">"View stock"</a> }.render(cx)"#,
        r####"md_sync! {r###"# Available **soon**

[View stock](/stock)
"###}.render_md(cx)"####,
    );
    assert!(lint(&source).is_empty(), "{:?}", lint(&source));
    let deleted = source.replace("Available **soon**", "Available");
    assert_eq!(missing(&deleted).len(), 1);
    let missing_url = source.replace("[View stock](/stock)", "View stock");
    assert_eq!(missing(&missing_url).len(), 1);
}

#[test]
fn literal_omissions_are_explicit_and_detect_stale_exceptions() {
    let source = fixture(
        r#"html_sync! { <span aria-label="Loading"></span> }.render(cx)"#,
        r#"omit!("Loading"); md_sync! {}.render_md(cx)"#,
    );
    assert!(lint(&source).is_empty());
    let stale = source.replace("aria-label=\"Loading\"", "aria-label=\"Working\"");
    assert_eq!(code(&lint(&stale), "render/content-omit-stale"), 1);
}

#[test]
fn proven_markdown_writers_preserve_child_coverage() {
    let source = fixture(
        "html_sync! {{self.title}}.render(cx)",
        "cx.md_block_boundary()?; cx.md_heading_with(1, |out| self.title.render_md(out))",
    )
    .replace("&mut Cx", "&mut proa_core::WebContext");
    assert!(lint(&source).is_empty(), "{:?}", lint(&source));
    let deleted = source.replace("self.title.render_md(out)", "md_sync! {}.render_md(out)");
    assert_eq!(missing(&deleted).len(), 1);
    let opaque = source.replace("md_heading_with", "custom_heading_with");
    assert_eq!(code(&lint(&opaque), "render/content-coverage-unchecked"), 1);
    let opaque_level = source.replace("md_heading_with(1,", "md_heading_with(custom_level(cx),");
    assert_eq!(
        code(&lint(&opaque_level), "render/content-coverage-unchecked"),
        1
    );
}

#[test]
fn typed_loader_fields_match_by_query_and_payload_not_local_spelling() {
    let source = fixture(
        "let html_data = cx.load_typed::<Data>(query()).await?; html_sync! {{html_data.title}}.render(cx)",
        "let markdown_data = cx.load_typed::<Data>(query()).await?; md_sync! {{markdown_data.title}}.render_md(cx)",
    ).replace("&mut Cx", "&mut proa_core::WebContext");
    assert!(lint(&source).is_empty(), "{:?}", lint(&source));
    let deleted = source.replace("{{markdown_data.title}}", "{}");
    assert_eq!(missing(&deleted).len(), 1);
    let changed_key = source.replace(
        "::<Data>(query()).await?; md_sync",
        "::<Data>(other_query()).await?; md_sync",
    );
    assert_eq!(missing(&changed_key).len(), 1);
    let generic = source.replace("::<Data>", "::<Data<First>>");
    let changed_type = generic.replace(
        "::<Data<First>>(query()).await?; md_sync",
        "::<Data<Second>>(query()).await?; md_sync",
    );
    assert_eq!(missing(&changed_type).len(), 1);
}

#[test]
fn optional_attributes_match_if_let_bindings_without_suppressing_missing_fields() {
    let source = r#"
use proa_core::{WebRenderSync, MdRenderSync, WebContext};
use proa_macros::{html_sync, md_sync};
struct Input { placeholder: Option<String> }
impl WebRenderSync for Input {
    fn render(self, cx: &mut WebContext) -> Result { html_sync! {<input placeholder={self.placeholder}/>} .render(cx) }
}
impl MdRenderSync for Input {
    fn render_md(self, cx: &mut WebContext) -> Result {
        if let Some(label) = self.placeholder { md_sync! {{label}}.render_md(cx)?; }
        Ok(())
    }
}
"#;
    assert!(lint(source).is_empty(), "{:?}", lint(source));
    let qualified = source.replace(
        "if let Some(label)",
        "if let std::option::Option::Some(label)",
    );
    assert!(lint(&qualified).is_empty(), "{:?}", lint(&qualified));
    let borrowed = source.replace("= self.placeholder {", "= self.placeholder.as_ref() {");
    assert!(lint(&borrowed).is_empty(), "{:?}", lint(&borrowed));
    let deleted = source.replace("md_sync! {{label}}", "md_sync! {}");
    assert_eq!(missing(&deleted).len(), 1);
    let guarded = source
        .replace("if let Some(label)", "if self.allowed { if let Some(label)")
        .replace("        Ok(())", "        } Ok(())");
    assert_eq!(
        code(&lint(&guarded), "render/content-coverage-unchecked"),
        1
    );
}

#[test]
fn only_fully_static_markup_is_inferred_for_the_framework_raw_adapter() {
    let source = fixture(
        "html_sync! {{proa_core::StaticRaw(ICON)} {self.title}}.render(cx)",
        "md_sync! {{self.title}}.render_md(cx)",
    );
    let source = format!("const ICON: &str = r#\"<svg aria-hidden=\"true\"><path d=\"M0 0L1 1\"/></svg>\"#;\n{source}");
    assert!(lint(&source).is_empty(), "{:?}", lint(&source));
    let dynamic_raw = source.replace(
        "proa_core::StaticRaw(ICON)",
        "proa_core::StaticRaw(self.image)",
    );
    assert_eq!(
        code(&lint(&dynamic_raw), "render/content-coverage-unchecked"),
        1
    );
    let opaque_raw = source.replace("proa_core::StaticRaw", "custom::StaticRaw");
    assert_eq!(
        code(&lint(&opaque_raw), "render/content-coverage-unchecked"),
        1
    );
    let erased_title = source.replace("md_sync! {{self.title}}", "md_sync! {}");
    assert_eq!(missing(&erased_title).len(), 1);
}

#[test]
fn shadowed_some_function_is_not_treated_as_the_option_constructor() {
    let source = fixture(
        "html_sync! {{Some(self.title)}}.render(cx)",
        "md_sync! {}.render_md(cx)",
    );
    let source = format!("fn Some(value: String) -> String {{ value }}\n{source}");
    assert_eq!(code(&lint(&source), "render/content-coverage-unchecked"), 1);
    assert!(missing(&source).is_empty());
}

#[test]
fn child_options_and_unit_variants_keep_input_identity() {
    let source = fixture(
        "html_sync! {{RestockNotice { size: Some(Size::Small), id: None }}}.render(cx)",
        "md_sync! {{RestockNotice { size: Some(Size::Small), id: None }}}.render_md(cx)",
    );
    let source = format!("enum Size {{ Small, Large }}\n{source}");
    assert!(lint(&source).is_empty(), "{:?}", lint(&source));
    let changed = source.replace(
        "size: Some(Size::Small), id: None }}}.render_md",
        "size: Some(Size::Large), id: None }}}.render_md",
    );
    assert_eq!(
        code(&lint(&changed), "render/content-coverage-unchecked"),
        1
    );
    let deleted = source.replace(
        "md_sync! {{RestockNotice { size: Some(Size::Small), id: None }}}",
        "md_sync! {}",
    );
    assert_eq!(missing(&deleted).len(), 1);
    let unknown = source.replace("Size::Small", "external::Size::Small");
    assert_eq!(
        code(&lint(&unknown), "render/content-coverage-unchecked"),
        1
    );
    let shadowed = format!("const None: &str = \"visible\";\n{source}");
    assert_eq!(
        code(&lint(&shadowed), "render/content-coverage-unchecked"),
        1
    );
}

#[test]
fn raw_markup_constants_cannot_override_shadowing_or_configuration() {
    let source = fixture(
        "html_sync! {{proa_core::StaticRaw(ICON)} {self.title}}.render(cx)",
        "md_sync! {{self.title}}.render_md(cx)",
    );
    let constant = "const ICON: &str = r#\"<svg aria-hidden=\"true\"/>\"#;";
    for prefix in [
        format!("#[cfg(feature=\"icon\")] {constant}"),
        format!("#[cfg_attr(feature=\"icon\", cfg(unix))] {constant}"),
        format!("{constant}\nconst ICON: &str = other();"),
    ] {
        let conditional = format!("{prefix}\n{source}");
        assert_eq!(
            code(&lint(&conditional), "render/content-coverage-unchecked"),
            1,
            "{:?}",
            lint(&conditional)
        );
    }
    let shadowed = format!(
        "{constant}\n{}",
        source.replace("html_sync!", "let ICON = self.image; html_sync!")
    );
    assert_eq!(
        code(&lint(&shadowed), "render/content-coverage-unchecked"),
        1
    );
}

#[test]
fn inline_code_projection_is_supported_but_arbitrary_callbacks_are_opaque() {
    let source = fixture(
        "html_sync! {{self.title}}.render(cx)",
        "cx.write_md_inline_code(Some(self.title).as_ref().map_or(\"\", proa_core::MdCodeContent::md_code_content))",
    ).replace("&mut Cx", "&mut proa_core::WebContext");
    assert!(lint(&source).is_empty(), "{:?}", lint(&source));
    let deleted = source.replace("cx.write_md_inline_code(Some(self.title).as_ref().map_or(\"\", proa_core::MdCodeContent::md_code_content))", "cx.write_md_inline_code(\"\")");
    assert_eq!(missing(&deleted).len(), 1);
    let callback = source.replace(
        "proa_core::MdCodeContent::md_code_content",
        "custom::md_code_content",
    );
    assert_eq!(
        code(&lint(&callback), "render/content-coverage-unchecked"),
        1
    );
}
