// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use super::*;

#[test]
fn public_asset_lints_follow_html_and_typed_component_url_fields() {
    let source = r#"
impl WebRenderSync for Product {
    fn render(self, cx: &mut WebContext) -> Result<(), WriteError> {
        let thumbnail = Image { src: Some(String::from("public/image.png")) };
        html_sync! {
            <a href="public/catalog.pdf">"Catalog"</a>
            <img src={"./public/cover.png"} alt="Cover" />
            <source srcset="https://cdn.example/logo.png 1x, public/large.png 2x" />
            <img src={cx.url("/public/logo.png")} alt="Bad prefix" />
            <img src="/shop/_proa/immutable/img/logo.a1b2c3d4e5f60718.svg" alt="Hard-coded" />
            <img src={cx.url("public/ok.png")} alt="Resolved" />
            <img srcset="data:image/png,public/fake.png 1x, https://cdn.example/image.png 2x" alt="Remote" />
            <a href="/products">"Products"</a>
            <a href="//cdn.example/public/a">"Remote"</a>
            <div data-source="public/label.txt"></div>
            {Image { src: "public/child.png".into() }}
            {thumbnail}
        }.render(cx)
    }
}
fn copy_input() { let _ = FileCopy { src: "public/local.png" }; }
"#;
    let diagnostics: Vec<_> = lint_file(Path::new("src/product.rs"), source)
        .into_iter()
        .filter(|diagnostic| diagnostic.code.starts_with("asset/"))
        .collect();
    let mut hits: Vec<_> = diagnostics
        .iter()
        .map(|diagnostic| (diagnostic.line, diagnostic.code))
        .collect();
    hits.sort_unstable();
    assert_eq!(
        hits,
        [
            (4, "asset/public-url"),
            (6, "asset/public-url"),
            (7, "asset/public-url"),
            (8, "asset/public-url"),
            (9, "asset/logical-path"),
            (10, "asset/generated-url"),
            (16, "asset/public-url"),
        ]
    );
    assert!(diagnostics
        .iter()
        .all(|diagnostic| diagnostic.severity == Severity::Warning));
}

#[test]
fn head_hooks_require_explicit_metadata_roles() {
    for (render_trait, hook) in [
        ("WebRender", "render_head"),
        ("WebRenderSend", "render_head_send"),
        ("WebRenderSync", "render_head"),
        ("WebRenderSendSync", "render_head_send_sync"),
    ] {
        for role in [
            "",
            "const ROLE: ComponentRole = ComponentRole::COMPONENT;",
            "const ROLE: ComponentRole = (ComponentRole::COMPONENT);",
            "const ROLE: ComponentRole = { proa_core::ComponentRole::COMPONENT };",
        ] {
            let source = format!("impl proa_core::{render_trait} for Page {{ {role} async fn {hook}(&self, cx: &mut WebContext) -> Result<(), WriteError> {{ Ok(()) }} }}");
            let diagnostics = lint_file(Path::new("page.rs"), &source);
            let errors: Vec<_> = diagnostics
                .iter()
                .filter(|d| d.code == "render/head-role-required")
                .collect();
            assert_eq!(errors.len(), 1, "{render_trait}: {diagnostics:?}");
            assert_eq!(errors[0].severity, Severity::Error);
        }
        for role in [
            "ComponentRole::LAYOUT",
            "ComponentRole::PAGE",
            "T::ROLE",
            "APP_PAGE_ROLE",
        ] {
            let source = format!("mod pages {{ impl proa_core::{render_trait} for Page {{ const ROLE: ComponentRole = {role}; async fn {hook}(&self, cx: &mut WebContext) -> Result<(), WriteError> {{ Ok(()) }} }} }}");
            assert!(
                !lint_file(Path::new("page.rs"), &source)
                    .iter()
                    .any(|d| d.code == "render/head-role-required"),
                "{source}"
            );
        }
    }
}

#[test]
fn head_role_lint_ignores_body_only_components_other_traits_and_excluded_tests() {
    let source = r#"
        impl WebRender for BodyOnly { fn render(self, cx: &mut WebContext) {} }
        impl WebRender for InheritedPage {
            const ROLE: ComponentRole = ComponentRole::PAGE;
            fn render(self, cx: &mut WebContext) {}
        }
        impl OtherTrait for Other { async fn render_head(&self) {} }
        #[cfg(test)]
        mod tests {
            impl WebRender for Fixture { async fn render_head(&self) {} }
        }
    "#;
    assert!(!lint_file(Path::new("page.rs"), source)
        .iter()
        .any(|d| d.code == "render/head-role-required"));
    let diagnostics = lint_file_with_options(
        Path::new("page.rs"),
        source,
        LintOptions {
            include_tests: true,
            ..Default::default()
        },
    );
    assert_eq!(
        diagnostics
            .iter()
            .filter(|d| d.code == "render/head-role-required")
            .count(),
        1
    );
}

#[test]
fn head_role_help_names_the_public_facade() {
    let source = "impl WebRender for Page { async fn render_head(&self, cx: &mut WebContext) -> Result<(), WriteError> { Ok(()) } }";
    let diagnostics = lint_file(Path::new("page.rs"), source);
    let help = diagnostics
        .iter()
        .find(|d| d.code == "render/head-role-required")
        .and_then(|d| d.help.as_deref())
        .unwrap();
    assert!(help.contains("proa::ComponentRole::LAYOUT"), "{help}");
    assert!(!help.contains("proa_core"), "{help}");
}

fn codes_at(diagnostics: &[Diagnostic], code: &str) -> Vec<usize> {
    let mut lines: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.code == code)
        .map(|d| d.line)
        .collect();
    lines.sort_unstable();
    lines
}

#[test]
fn head_mutation_in_body_flags_checked_setters_in_every_render_body() {
    let source = r#"
impl WebRender for Product {
    async fn render_head(&self, cx: &mut WebContext) -> Result<(), WriteError> {
        cx.head_mut().title("Allowed in the hook")
    }
    fn render(self, cx: &mut WebContext) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>> {
        RenderOutcome::pending(async move {
            cx.head_mut().title("Too late")?;
            Ok(())
        })
    }
}
impl WebRenderSync for Card {
    fn render(self, cx: &mut WebContext) -> Result<(), WriteError> {
        let mut head = cx.head_mut();
        head.description("Too late")?;
        cx.head_mut().set_title("legacy manual setter");
        cx.head_mut().push_meta("robots", "noindex");
        Ok(())
    }
}
impl WebRenderSend for SendCard {
    fn render_send(self, cx: &mut SendWebContext) -> RenderOutcome<impl Future<Output = Result<(), WriteError>> + Send> {
        RenderOutcome::pending(async move { (cx.head_mut()).metadata(HeadMetadata::default()) })
    }
}
impl WebRenderSendSync for SyncSendCard {
    fn render_send_sync(self, cx: &mut SendWebContext) -> Result<(), WriteError> {
        let title = cx.head_state().title();
        cx.head_mut().canonical("/x")
    }
}
impl OtherTrait for NotRender {
    fn render(self, cx: &mut WebContext) { cx.head_mut().title("other trait"); }
}
"#;
    let diagnostics = lint_file(Path::new("product.rs"), source);
    assert_eq!(
        codes_at(&diagnostics, "render/head-mutation-in-body"),
        [8, 16, 24, 30],
        "{diagnostics:#?}"
    );
    let first = diagnostics
        .iter()
        .find(|d| d.code == "render/head-mutation-in-body")
        .unwrap();
    assert_eq!(first.severity, Severity::Error);
    assert!(first.message.contains("`head_mut().title(..)` in `render`"));
    assert!(first.help.as_deref().unwrap().contains("render_head"));
    assert!(first
        .help
        .as_deref()
        .unwrap()
        .contains("proa::ComponentRole::PAGE"));
}

#[test]
fn head_roles_in_suspense_and_inside_a_page_are_errors() {
    let source = r#"
use crate::components::Card;
use proa::stream::Suspense;
struct Shell<C> { page: C }
struct ProductPage;
struct ReviewsPage;
struct Fallback;
impl<C: WebRender> WebRender for Shell<C> {
    const ROLE: ComponentRole = ComponentRole::LAYOUT;
    fn render(self, cx: &mut WebContext) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>> {
        html! { <main>{ProductPage}</main> }.render(cx)
    }
}
impl WebRender for ProductPage {
    const ROLE: ComponentRole = ComponentRole::PAGE;
    fn render(self, cx: &mut WebContext) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>> {
        html! {
            <section>
                {Card { title: "ok" }}
                {Suspense::new(Card { title: "slow" }, Fallback)}
                {to_async(ReviewsPage)}
                {Shell { page: () }}
            </section>
        }
        .render(cx)
    }
}
impl WebRender for ReviewsPage {
    const ROLE: ComponentRole = ComponentRole::PAGE;
    fn render(self, cx: &mut WebContext) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>> {
        RenderOutcome::pending(async move { render_child(cx, Fallback).await })
    }
}
impl WebRenderSync for Fallback {
    fn render(self, cx: &mut WebContext) -> Result<(), WriteError> { Ok(()) }
}
async fn route(cx: &mut WebContext) -> Result<(), WriteError> {
    render_child(cx, Suspense::new(ProductPage, Fallback)).await?;
    render_child(cx, Shell { page: ProductPage }).await
}
"#;
    let diagnostics = lint_file(Path::new("src/pages.rs"), source);
    assert_eq!(
        codes_at(&diagnostics, "render/head-role-in-page"),
        [21, 22],
        "{diagnostics:#?}"
    );
    assert_eq!(
        codes_at(&diagnostics, "render/head-role-in-suspense"),
        [38],
        "{diagnostics:#?}"
    );
    let nested = diagnostics
        .iter()
        .find(|d| d.code == "render/head-role-in-page" && d.line == 22)
        .unwrap();
    assert_eq!(nested.severity, Severity::Error);
    assert!(nested.message.contains(
        "`Shell` declares `ComponentRole::LAYOUT` but is rendered inside the Page `ProductPage`"
    ));
    assert!(nested
        .help
        .as_deref()
        .unwrap()
        .contains("render `ProductPage` inside `Shell`"));
    assert_eq!(nested.labels.len(), 2);
    assert_eq!(nested.labels[1].range.line, 9);
    let suspense = diagnostics
        .iter()
        .find(|d| d.code == "render/head-role-in-suspense")
        .unwrap();
    assert!(suspense
        .notes
        .iter()
        .any(|note| note.contains("HeadError::Excluded")));
}

#[test]
fn head_role_placement_lints_skip_names_they_cannot_resolve() {
    let source = r#"
use ui::Header;
use crate::layouts::*;
struct ProductPage;
struct Header;
mod a { pub struct Banner; }
mod b { pub struct Banner; }
impl WebRender for Header {
    const ROLE: ComponentRole = ComponentRole::LAYOUT;
    fn render(self, cx: &mut WebContext) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>> { todo!() }
}
impl WebRender for a::Banner {
    const ROLE: ComponentRole = ComponentRole::LAYOUT;
    fn render(self, cx: &mut WebContext) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>> { todo!() }
}
impl<T: WebRender> WebRender for Wrapper<T> {
    const ROLE: ComponentRole = T::ROLE;
    fn render(self, cx: &mut WebContext) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>> { todo!() }
}
impl WebRender for ProductPage {
    const ROLE: ComponentRole = ComponentRole::PAGE;
    fn render(self, cx: &mut WebContext) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>> {
        let link = ProductPage::new().href();
        html! {
            {Header}
            {a::Banner}
            {Wrapper(Inner)}
            {RootLayout { page: () }}
            {self.layout}
        }
        .render(cx)
    }
}
"#;
    let diagnostics = lint_file(Path::new("src/pages.rs"), source);
    assert!(
        !diagnostics
            .iter()
            .any(|d| d.code.starts_with("render/head-role-in")),
        "{diagnostics:#?}"
    );
}

#[test]
fn head_role_placement_lints_skip_generic_parameters_named_like_layouts() {
    let source = r#"
pub struct Shell;
impl WebRender for Shell {
    const ROLE: ComponentRole = ComponentRole::LAYOUT;
    fn render(self, cx: &mut WebContext) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>> { todo!() }
}
pub struct Article<Shell>(Shell);
impl<Shell: WebRender + Default> WebRender for Article<Shell> {
    const ROLE: ComponentRole = ComponentRole::PAGE;
    fn render(self, cx: &mut WebContext) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>> {
        html! { {Shell::default()} }.render(cx)
    }
}
pub fn deferred<Shell: WebRender + Default>() -> impl WebRender {
    Suspense::new(Shell::default(), ())
}
pub struct Note;
impl WebRender for Note {
    const ROLE: ComponentRole = ComponentRole::PAGE;
    fn render(self, cx: &mut WebContext) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>> {
        html! { {Shell} }.render(cx)
    }
}
"#;
    let diagnostics = lint_file(Path::new("src/pages.rs"), source);
    assert_eq!(
        codes_at(&diagnostics, "render/head-role-in-page"),
        [21],
        "only the module's own Shell is a layout: {diagnostics:#?}"
    );
    assert!(
        codes_at(&diagnostics, "render/head-role-in-suspense").is_empty(),
        "{diagnostics:#?}"
    );
}

#[test]
fn head_role_placement_lints_resolve_names_per_module() {
    let source = r#"
mod ui { pub struct Card; }
mod layouts {
    pub struct Panel;
    impl WebRender for Panel {
        const ROLE: ComponentRole = ComponentRole::LAYOUT;
        fn render(self, cx: &mut WebContext) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>> { todo!() }
    }
}
mod aliased {
    use crate::ui::Card as Panel;
    pub struct APage;
    impl WebRender for APage {
        const ROLE: ComponentRole = ComponentRole::PAGE;
        fn render(self, cx: &mut WebContext) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>> {
            html! { {Panel} }.render(cx)
        }
    }
}
mod direct {
    use crate::layouts::Panel;
    pub struct BPage;
    impl WebRender for BPage {
        const ROLE: ComponentRole = ComponentRole::PAGE;
        fn render(self, cx: &mut WebContext) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>> {
            html! { {Panel} {super::layouts::Panel} }.render(cx)
        }
    }
}
mod globbed {
    use crate::layouts::*;
    use proa::*;
    pub struct CPage;
    impl WebRender for CPage {
        const ROLE: ComponentRole = ComponentRole::PAGE;
        fn render(self, cx: &mut WebContext) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>> {
            html! { {Panel} }.render(cx)
        }
    }
}
mod shadowed {
    use crate::layouts::Panel;
    pub struct DPage;
    impl WebRender for DPage {
        const ROLE: ComponentRole = ComponentRole::PAGE;
        fn render(self, cx: &mut WebContext) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>> {
            use crate::ui::Card as Panel;
            html! { {Panel} }.render(cx)
        }
    }
}
mod unimported {
    pub struct EPage;
    impl WebRender for EPage {
        const ROLE: ComponentRole = ComponentRole::PAGE;
        fn render(self, cx: &mut WebContext) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>> {
            html! { {Panel} }.render(cx)
        }
    }
}
"#;
    let diagnostics = lint_file(Path::new("src/lib.rs"), source);
    assert_eq!(
        codes_at(&diagnostics, "render/head-role-in-page"),
        [26, 26],
        "{diagnostics:#?}"
    );
}

#[test]
fn head_role_placement_lints_follow_rendered_branches_and_loop_bodies() {
    let source = r#"
struct Nested;
struct Home;
impl WebRender for Nested {
    const ROLE: ComponentRole = ComponentRole::PAGE;
    fn render(self, cx: &mut WebContext) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>> { todo!() }
}
impl WebRender for Home {
    const ROLE: ComponentRole = ComponentRole::PAGE;
    fn render(self, cx: &mut WebContext) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>> {
        html! {
            {if show { Nested }}
            {if show { () } else if other { () } else { Nested }}
            {match kind { 1 => Nested, _ => () }}
            {for _ in 0..2 { Nested }}
            {while more { Nested }}
            {{ let n = 1; Nested }}
            {if show { Nested; }}
            {'done: { break 'done (); Nested }}
            {for _ in 0..2 { let _ = Nested; }}
            {if show { return; }}
        }
        .render(cx)
    }
}
"#;
    let diagnostics = lint_file(Path::new("src/pages.rs"), source);
    assert_eq!(
        codes_at(&diagnostics, "render/head-role-in-page"),
        [12, 13, 14, 15, 16, 17],
        "{diagnostics:#?}"
    );
}

#[test]
fn head_mutation_in_body_checks_template_slots() {
    let source = r#"
impl WebRenderSync for Card {
    fn render(self, cx: &mut WebContext) -> Result<(), WriteError> {
        html_sync! {
            <p>{{ cx.head_mut().title("Late")?; () }}</p>
            <div>{html_sync! { <span>{{ let mut head = cx.head_mut(); head.description("Late")?; () }}</span> }}</div>
            <a title={cx.head_mut().canonical("/x").is_ok()}></a>
            <p>{{ cx.head_mut().set_title("legacy"); () }}</p>
        }
        .render(cx)
    }
}
"#;
    let diagnostics = lint_file(Path::new("src/card.rs"), source);
    assert_eq!(
        codes_at(&diagnostics, "render/head-mutation-in-body"),
        [5, 6, 7],
        "{diagnostics:#?}"
    );
}

#[test]
fn head_mutation_writers_follow_rust_scoping() {
    // Each `// flag` line must be reported; every other `title`,
    // `description`, or `canonical` call reaches a non-writer.
    let source = r#"
impl WebRenderSync for TemplateSlot {
    fn render(self, cx: &mut WebContext) -> Result<(), WriteError> {
        let head = &self.article;
        html_sync! {
            <p>{{ let mut head = cx.head_mut(); head.title("Late")?; () }}</p> // flag
            <p>{head.title()}</p>
        }
        .render(cx)?;
        head.title();
        Ok(())
    }
}
impl WebRenderSync for NestedBlocks {
    fn render(self, cx: &mut WebContext) -> Result<(), WriteError> {
        let head = &self.article;
        {
            let mut head = cx.head_mut();
            head.title("Late")?; // flag
        }
        head.title();
        let mut writer = cx.head_mut();
        {
            let writer = &self.article;
            writer.description();
        }
        writer.description("Late") // flag
    }
}
impl WebRenderSync for Closures {
    fn render(self, cx: &mut WebContext) -> Result<(), WriteError> {
        let mut head = cx.head_mut();
        let by_param = |head: &Article| head.title();
        let local = || {
            let head = &self.article;
            head.title()
        };
        let inner = || {
            let mut other = cx.head_mut();
            other.title("Late") // flag
        };
        let other = &self.article;
        other.title();
        head.title("Late") // flag
    }
}
impl WebRenderSync for MatchArms {
    fn render(self, cx: &mut WebContext) -> Result<(), WriteError> {
        let mut head = cx.head_mut();
        match self.article {
            Some(head) => head.title(),
            Wrapper { head, .. } if head.title() => (),
            _ if [true].into_iter().any(|head| head) => head.title("Late")?, // flag
            _ => head.title("Late")?, // flag
        }
        if let Some(head) = self.article {
            head.title();
        } else {
            head.canonical("/late")?; // flag
        }
        while let Some(head) = next() {
            head.title();
        }
        for head in articles {
            head.title();
        }
        Ok(())
    }
}
impl WebRenderSync for Shadowing {
    fn render(self, cx: &mut WebContext) -> Result<(), WriteError> {
        let mut head = cx.head_mut();
        let head = &self.article;
        head.title();
        let (head, _) = (cx.head_mut(), 1);
        head.title();
        let head = &self.article;
        let mut head = {
            head.title();
            cx.head_mut()
        };
        head.title("Late")?; // flag
        let Some(head) = maybe else { return Ok(()) };
        head.title();
        let mut writer = cx.head_mut();
        rebind!(writer);
        writer.title("hidden by a macro that may rebind it");
        Ok(())
    }
}
"#;
    let expected: Vec<_> = source
        .lines()
        .enumerate()
        .filter(|(_, line)| line.contains("// flag"))
        .map(|(index, _)| index + 1)
        .collect();
    let diagnostics = lint_file(Path::new("src/head.rs"), source);
    assert_eq!(
        codes_at(&diagnostics, "render/head-mutation-in-body"),
        expected,
        "{diagnostics:#?}"
    );
}

#[test]
fn head_role_placement_lints_resolve_package_local_imports_across_files() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    std::fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"app\"\nversion = \"0.0.0\"\n",
    )
    .unwrap();
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(
        root.join("src/layouts.rs"),
        r#"
pub struct Root<C> { pub page: C }
impl<C: WebRender> WebRender for Root<C> {
    const ROLE: proa::ComponentRole = proa::ComponentRole::LAYOUT;
    fn render(self, cx: &mut WebContext) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>> {
        html! { <body>{self.page}</body> }.render(cx)
    }
}
"#,
    )
    .unwrap();
    let pages = root.join("src/pages.rs");
    std::fs::write(
        &pages,
        r#"
use crate::layouts::Root as AppRoot;
pub struct Product;
impl WebRender for Product {
    const ROLE: proa::ComponentRole = proa::ComponentRole::PAGE;
    fn render(self, cx: &mut WebContext) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>> {
        html! { <main>{AppRoot { page: () }}</main> }.render(cx)
    }
}
"#,
    )
    .unwrap();
    let diagnostics = lint_paths(&[pages.clone()], LintOptions::default()).unwrap();
    let found: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.code == "render/head-role-in-page")
        .collect();
    assert_eq!(found.len(), 1, "{diagnostics:#?}");
    assert_eq!(found[0].line, 7);
    assert!(found[0].labels[1].range.path.ends_with("src/layouts.rs"));
    assert_eq!(found[0].labels[1].range.line, 4);
}

fn write_strategy_sidecar_with_fingerprint(
    root: &Path,
    analysis_id: &str,
    struct_name: &str,
    struct_path: &str,
    render_strategy: &str,
    input_fingerprint: Option<&str>,
    hydration_strategy: Option<(&str, &str)>,
) {
    let dir = root.join("target").join("rsjs").join("strategy");
    std::fs::create_dir_all(&dir).unwrap();
    let needs_client_js = render_strategy != "server_only";
    let (hydration_kind, hydration_label) =
        hydration_strategy.unwrap_or(("immediate", "immediate"));
    let value = serde_json::json!({
        "format": "rsjs.strategy",
        "strategy_version": 1,
        "strategy_mode": "manifest",
        "analysis_id": analysis_id,
        "runtime_asset_id": needs_client_js.then_some(analysis_id),
        "struct_name": struct_name,
        "struct_path": struct_path,
        "input_fingerprint": input_fingerprint,
        "render_strategy": render_strategy,
        "hydration_strategy": {
            "kind": hydration_kind,
            "label": hydration_label
        },
        "declared_client_modules": [],
        "client_modules": [],
        "roots": if needs_client_js {
            serde_json::json!([{ "kind": "text_binding", "node": 1 }])
        } else {
            serde_json::json!([])
        },
        "emitted": {
            "wrapper": needs_client_js,
            "seed_script": needs_client_js,
            "manifest_registration": needs_client_js,
            "js_asset": needs_client_js,
            "component_fragment_asset": needs_client_js,
            "uses_queries": false,
            "uses_mutations": false
        }
    });
    std::fs::write(dir.join(format!("{analysis_id}.json")), value.to_string()).unwrap();
}

fn first_rsjs_impl_fingerprint(source: &str) -> String {
    let file = syn::parse_file(source).unwrap();
    file.items
        .iter()
        .find_map(|item| match item {
            Item::Impl(item_impl) if has_attr(&item_impl.attrs, &["rsjs"]) => {
                rsjs_input_fingerprint_for_impl(item_impl)
            }
            _ => None,
        })
        .expect("test source should contain an rsjs impl")
}

#[test]
fn lint_reports_perf_warnings() {
    let source = r#"
fn render() {
    html_sync! {
        <div class={"card"}>{format!("hello {}", name)}{count.to_string()}</div>
        <span>{"literal"}</span>
    };
}
"#;
    let diagnostics = lint_file(Path::new("sample.rs"), source);
    let codes: Vec<_> = diagnostics.iter().map(|diag| diag.code).collect();
    assert!(codes.contains(&"perf/dynamic-class-literal"));
    assert!(codes.contains(&"perf/format"));
    assert!(codes.contains(&"perf/to-string"));
    assert!(codes.contains(&"perf/static-fragment-value"));
}

#[test]
fn lint_reports_template_parse_errors_with_documented_code() {
    let source = r#"
fn render() {
    html_sync! {
        <div>
    };
}
"#;
    let diagnostics = lint_file(Path::new("sample.rs"), source);
    assert!(diagnostics.iter().any(|diag| diag.code == "parse/template"));
}

#[test]
fn lint_reports_static_consolidation_attr_warnings() {
    let source = r#"
fn render() {
    html_sync! {
        <div class="" data-kind={"card"}></div>
        <button disabled={true} hidden={false} class={""}></button>
    };
}
"#;
    let diagnostics = lint_file(Path::new("sample.rs"), source);
    let codes: Vec<_> = diagnostics.iter().map(|diag| diag.code).collect();
    assert!(codes.contains(&"perf/empty-class"));
    assert!(codes.contains(&"perf/dynamic-literal-attr"));
    assert!(codes.contains(&"perf/static-bool-attr"));
    assert!(codes.contains(&"perf/false-bool-attr"));
}

#[test]
fn lint_reports_staticraw_doctype_inside_html_macro() {
    let source = r#"
fn render() {
    html! {
        {StaticRaw("<!DOCTYPE html>")}
        <html><body>"Hello"</body></html>
    };
}
"#;
    let diagnostics = lint_file(Path::new("sample.rs"), source);
    let diag = diagnostics
        .iter()
        .find(|diag| diag.code == "html/staticraw-doctype")
        .expect("expected StaticRaw doctype diagnostic");
    assert_eq!(diag.severity, Severity::Warning);
    assert!(
        diag.message.contains("hides the document doctype"),
        "unexpected message: {diag:?}"
    );
    assert!(
        diag.help
            .as_deref()
            .is_some_and(|help| help.contains("write `<!DOCTYPE html>` directly")),
        "unexpected help: {diag:?}"
    );
}

#[test]
fn lint_allows_direct_doctype_inside_html_macro() {
    let source = r#"
fn render() {
    html! {
        <!DOCTYPE html>
        <html><body>"Hello"</body></html>
    };
}
"#;
    let diagnostics = lint_file(Path::new("sample.rs"), source);
    assert!(
        !diagnostics
            .iter()
            .any(|diag| diag.code == "html/staticraw-doctype"),
        "direct doctype syntax must not be flagged: {diagnostics:?}"
    );
}

#[test]
fn lint_reports_simple_correctness_warnings() {
    let source = r#"
fn render() {
    html_sync! {
        <p data-Foo="x" on-click={handler} id="a" id="b"><div /></p>
    };
}
"#;
    let diagnostics = lint_file(Path::new("sample.rs"), source);
    let codes: Vec<_> = diagnostics.iter().map(|diag| diag.code).collect();
    assert!(codes.contains(&"html/invalid-nesting"));
    assert!(codes.contains(&"attr/duplicate"));
    assert!(codes.contains(&"attr/data-name"));
    assert!(codes.contains(&"attr/event-name"));
}

#[test]
fn lint_reports_manual_static_writes_in_markdown_renderers() {
    let source = r#"
impl<B: WriteBuf> MdRenderSync<::proa_core::NoLoader, B> for Callout {
    fn render_md(self, cx: &mut WebContext<::proa_core::NoLoader, B>) -> Result<(), WriteError> {
        cx.write_md_prepared_static(b"> **Note:** ")?;
        self.children.render_md(cx)
    }
}
"#;
    let diagnostics = lint_file(Path::new("md.rs"), source);
    let diagnostic = diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code == "md/manual-write")
        .expect("expected manual Markdown write diagnostic");

    assert_eq!(diagnostic.severity, Severity::Warning);
    assert!(
        diagnostic
            .help
            .as_deref()
            .is_some_and(|help| help.contains("md_sync!")),
        "unexpected help: {diagnostic:?}"
    );
}

#[test]
fn lint_reports_manual_writes_in_markdown_helpers() {
    let source = r#"
fn render_prefix<B: WriteBuf>(
    cx: &mut WebContext<::proa_core::NoLoader, B>,
    prefix: &[u8],
) -> Result<(), WriteError> {
    cx.write_md_prepared(prefix)?;
    cx.out.extend_static(b": ");
    WriteBuf::extend_static(&mut cx.out, b"!")?;
    Ok(())
}

impl<B: WriteBuf> MdRenderSync<::proa_core::NoLoader, B> for Callout {
    fn render_md(self, cx: &mut WebContext<::proa_core::NoLoader, B>) -> Result<(), WriteError> {
        render_prefix(cx, b"Note")?;
        self.children.render_md(cx)
    }
}
"#;
    let diagnostics = lint_file(Path::new("md.rs"), source);
    let manual_writes: Vec<_> = diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.code == "md/manual-write")
        .collect();

    assert_eq!(
        manual_writes.len(),
        3,
        "context writes and both output-buffer call forms in helpers must be rejected: {diagnostics:?}"
    );
}

#[test]
fn lint_reports_manual_writes_in_markdown_helper_methods() {
    let source = r#"
impl Callout {
    fn render_prefix<B: WriteBuf>(
        &self,
        cx: &mut WebContext<::proa_core::NoLoader, B>,
    ) -> Result<(), WriteError> {
        cx.write_md_prepared_static(b"> **Note:** ")
    }
}

impl<B: WriteBuf> MdRenderSync<::proa_core::NoLoader, B> for Callout {
    fn render_md(self, cx: &mut WebContext<::proa_core::NoLoader, B>) -> Result<(), WriteError> {
        self.render_prefix(cx)?;
        self.children.render_md(cx)
    }
}
"#;
    let diagnostics = lint_file(Path::new("md.rs"), source);
    assert!(
        diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "md/manual-write"),
        "raw writes in called helper methods must be rejected: {diagnostics:?}"
    );
}

#[test]
fn lint_allows_md_sync_in_markdown_renderers() {
    let source = r#"
impl<B: WriteBuf> MdRenderSync<::proa_core::NoLoader, B> for Callout {
    fn render_md(self, cx: &mut WebContext<::proa_core::NoLoader, B>) -> Result<(), WriteError> {
        md_sync! { "> **Note:** {self.children}" }.render_md(cx)
    }
}
"#;
    let diagnostics = lint_file(Path::new("md.rs"), source);
    assert!(
        !diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "md/manual-write"),
        "md_sync! must be the accepted renderer shape: {diagnostics:?}"
    );
}

#[test]
fn lint_allows_typed_markdown_context_helpers() {
    let source = r#"
impl<B: WriteBuf> MdRenderSync<::proa_core::NoLoader, B> for Link {
    fn render_md(self, cx: &mut WebContext<::proa_core::NoLoader, B>) -> Result<(), WriteError> {
        cx.write_md_text(self.label)?;
        cx.write_md_url(self.href)?;
        cx.md_code(self.code)?;
        cx.md_link(self.href, self.label)
    }
}
"#;
    let diagnostics = lint_file(Path::new("md.rs"), source);
    assert!(
        !diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "md/manual-write"),
        "typed Markdown context helpers must remain available: {diagnostics:?}"
    );
}

#[test]
fn lint_reports_formatter_subcodes() {
    let source = r#"
fn render() {
    html_sync! { <img src = "x"/> };
}
"#;
    let diagnostics = lint_file(Path::new("sample.rs"), source);
    let codes: Vec<_> = diagnostics.iter().map(|diag| diag.code).collect();
    assert!(codes.contains(&"format/macro-body"));
    assert!(codes.contains(&"format/attrs"));
    assert!(codes.contains(&"format/self-closing"));
    assert_eq!(
        diagnostics
            .iter()
            .find(|diag| diag.code == "format/attrs")
            .map(|diag| diag.severity),
        Some(Severity::Warning)
    );
}

#[test]
fn lint_warns_about_noncanonical_raw_markdown_layout() {
    let source = r####"
fn render(self, cx: &mut WebContext<::proa_core::NoLoader, B>) {
    md_sync! { r###"
            # {self.title}

            {self.body}
            "### }
    .render_md(cx)
}
"####;

    let diagnostics = lint_file(Path::new("sample.rs"), source);
    let layout = diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code == "format/markdown-layout");

    assert_eq!(
        layout.map(|diagnostic| diagnostic.severity),
        Some(Severity::Warning)
    );
    assert!(
        layout
            .and_then(|diagnostic| diagnostic.help.as_deref())
            .is_some_and(|help| help.contains("closing `}`")),
        "unexpected diagnostics: {diagnostics:?}"
    );
}

#[test]
fn lint_accepts_canonical_raw_markdown_layout() {
    let source = r####"
fn render(self, cx: &mut WebContext<::proa_core::NoLoader, B>) {
    md_sync! { r###"
        # {self.title}

        {self.body}
        "###
    }
    .render_md(cx)
}
"####;

    let diagnostics = lint_file(Path::new("sample.rs"), source);

    assert!(
        !diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "format/markdown-layout"),
        "unexpected diagnostics: {diagnostics:?}"
    );
}

#[test]
fn lint_reports_rustfmt_spaced_elements_and_attributes() {
    let source = r#"
fn render() {
    html_sync! {
        < html lang = "en" > < body > < main class = "main-content" > {
        LargeSections } </ main > </ body > </ html >
    }
}
"#;
    let diagnostics = lint_file(Path::new("sample.rs"), source);
    let spacing = diagnostics.iter().find(|diag| diag.code == "format/attrs");
    assert_eq!(spacing.map(|diag| diag.severity), Some(Severity::Warning));
}

#[test]
fn lint_warns_about_noncanonical_rust_expression_layout() {
    let source = r#"
fn render() {
    html_sync! {
        <div>
            {Card { class: Some("shadow-none"), children: Some(html_sync! {
                                                          {CardHeader { children: Some("Profile"), ..Default::default() }}
                                                      }), }}
        </div>
    }
}
"#;
    let diagnostics = lint_file(Path::new("sample.rs"), source);
    let layout = diagnostics
        .iter()
        .find(|diag| diag.code == "format/rust-expression-layout");
    assert_eq!(layout.map(|diag| diag.severity), Some(Severity::Warning));
}

#[test]
fn lint_does_not_confuse_rust_operators_with_markup_spacing() {
    let source = r#"
fn render() {
    html_sync! {
        <div class={if width == 0 { "wide" } else { "narrow" }}>{value <= limit}</div>
    }
}
"#;
    let diagnostics = lint_file(Path::new("sample.rs"), source);
    assert!(!diagnostics.iter().any(|diag| diag.code == "format/attrs"));
}

#[test]
fn lint_reports_class_and_style_specific_warnings() {
    let source = r#"
fn render() {
    html_sync! {
        <div class={"flex gap-2"}></div>
        <div class={format!("btn {}", size)}></div>
        <div class={classes.join(" ")}></div>
        <div class={vec!["flex", "gap-2"].join(" ")}></div>
        <div class={format!("{} {}", "flex", "gap-2")}></div>
        <div style={"color:red"}></div>
    };
}
"#;
    let diagnostics = lint_file(Path::new("sample.rs"), source);
    let codes: Vec<_> = diagnostics.iter().map(|diag| diag.code).collect();
    assert!(codes.contains(&"perf/dynamic-class-literal"));
    assert!(codes.contains(&"perf/class-format"));
    assert!(codes.contains(&"perf/class-join"));
    assert!(codes.contains(&"perf/class-vec"));
    assert!(codes.contains(&"perf/runtime-tailwind"));
    assert!(codes.contains(&"perf/static-style-string"));
}

#[test]
fn lint_reports_additional_allocation_warnings() {
    let source = r#"
fn render() {
    html_sync! {
        <div>
            {String::new()}
            {Vec::with_capacity(4)}
            {serde_json::to_string(&data).unwrap()}
            {component.render_to_string()}
        </div>
    };
}
"#;
    let diagnostics = lint_file(Path::new("sample.rs"), source);
    let codes: Vec<_> = diagnostics.iter().map(|diag| diag.code).collect();
    assert!(codes.contains(&"perf/string-new"));
    assert!(codes.contains(&"perf/vec-with-capacity"));
    assert!(codes.contains(&"perf/serde-json-to-string"));
    assert!(codes.contains(&"perf/render-to-string"));
}

#[test]
fn lint_scans_render_trait_methods() {
    let source = r#"
impl<B: WriteBuf> WebRenderSync<::proa_core::NoLoader, B> for Card {
    fn render(self, cx: &mut WebContext<::proa_core::NoLoader, B>) -> Result<(), WriteError> {
        let mut html = String::new();
        html.push_str(&format!("<div>{}</div>", self.title));
        WriteBuf::extend(&mut cx.out, b"<span>")?;
        let raw = UnsafeRaw(&html);
        raw.render(cx)
    }
}
"#;
    let diagnostics = lint_file(Path::new("sample.rs"), source);
    let codes: Vec<_> = diagnostics.iter().map(|diag| diag.code).collect();
    assert!(codes.contains(&"perf/string-new"));
    assert!(codes.contains(&"perf/push-str"));
    assert!(codes.contains(&"perf/format"));
    assert!(codes.contains(&"perf/format-push-str-loop"));
    assert!(codes.contains(&"perf/raw-write-html"));
    assert!(codes.contains(&"perf/unsafe-raw-string"));
}

#[test]
fn lint_flags_text_writer_methods_like_their_byte_forms() {
    let source = r#"
impl<B: WriteBuf> WebRenderSync<::proa_core::NoLoader, B> for Card {
    fn render(self, cx: &mut WebContext<::proa_core::NoLoader, B>) -> Result<(), WriteError> {
        WriteBuf::extend_str(&mut cx.out, "<span>")?;
        cx.out.extend_static_str("<div class=\"card\">")
    }
}
impl AttrValue for AuditHref<'_> {
    fn render_attr_value<B: WriteBuf>(&self, cx: &mut WebContext<::proa_core::NoLoader, B>) -> Result<(), WriteError> {
        cx.out.extend_static_str("/audit/")?;
        cx.out.extend_str(self.id)
    }
}
"#;
    let diagnostics = lint_file(Path::new("sample.rs"), source);
    let codes: Vec<_> = diagnostics.iter().map(|diag| diag.code).collect();
    assert!(codes.contains(&"perf/raw-write-html"));
    assert!(codes.contains(&"perf/raw-attr-segment"));
    assert!(codes.contains(&"render/manual-attr-stream"));
}

#[test]
fn lint_reports_render_string_builder_attr_patterns() {
    let source = r#"
impl<B: WriteBuf> WebRenderSync<::proa_core::NoLoader, B> for Chart {
    fn render(self, cx: &mut WebContext<::proa_core::NoLoader, B>) -> Result<(), WriteError> {
        let mut html = String::new();
        html.push_str("<span class=\"segment\">");
        let mut path = String::new();
        path.push_str("M 0 0");
        let aria = format!("Score {}", self.score);
        html_sync! {
            <path d={path} aria-label={aria} style={format!("left:{:.1}%;width:{:.1}%", self.left, self.width)}></path>
        }
        .render(cx)
    }
}
"#;
    let diagnostics = lint_file(Path::new("sample.rs"), source);
    let codes: Vec<_> = diagnostics.iter().map(|diag| diag.code).collect();
    assert!(codes.contains(&"perf/raw-html-buffer"));
    assert!(codes.contains(&"perf/attr-string-builder"));
    assert!(codes.contains(&"perf/preformatted-aria"));
    assert!(codes.contains(&"perf/dynamic-style-percent"));
}

#[test]
fn lint_reports_attr_value_raw_segments() {
    let source = r#"
impl AttrValue for AuditHref<'_> {
    fn render_attr_value<B: WriteBuf>(&self, cx: &mut WebContext<::proa_core::NoLoader, B>) -> Result<(), WriteError> {
        cx.out.extend_static(b"/audit/")?;
        cx.out.extend(self.id.as_bytes())
    }
}
"#;
    let diagnostics = lint_file(Path::new("sample.rs"), source);
    let codes: Vec<_> = diagnostics.iter().map(|diag| diag.code).collect();
    assert!(codes.contains(&"perf/raw-attr-segment"));
    assert!(codes.contains(&"render/manual-attr-stream"));
    assert_eq!(
        diagnostics
            .iter()
            .find(|diag| diag.code == "render/manual-attr-stream")
            .map(|diag| diag.severity),
        Some(Severity::Info)
    );
}

#[test]
fn lint_reports_one_off_attr_wrappers() {
    let source = r#"
struct AuditHref<'a> {
    id: &'a str,
}

impl AttrValue for AuditHref<'_> {
    fn render_attr_value<B: WriteBuf>(&self, cx: &mut WebContext<::proa_core::NoLoader, B>) -> Result<(), WriteError> {
        cx.out.extend_static(b"/audit/")?;
        cx.out.extend(self.id.as_bytes())
    }
}

impl<B: WriteBuf> WebRenderSync<::proa_core::NoLoader, B> for Link<'_> {
    fn render(self, cx: &mut WebContext<::proa_core::NoLoader, B>) -> Result<(), WriteError> {
        html_sync! { <a href={AuditHref { id: self.id }} /> }.render(cx)
    }
}
"#;
    let diagnostics = lint_file(Path::new("sample.rs"), source);
    let one_off = diagnostics
        .iter()
        .find(|diag| diag.code == "render/one-off-attr-wrapper");
    assert!(one_off.is_some());
    assert_eq!(one_off.map(|diag| diag.severity), Some(Severity::Info));
}

#[test]
fn lint_reports_boolean_attr_wrappers() {
    let source = r#"
struct Selected(bool);

impl AttrValue for Selected {
    fn should_render(&self) -> bool {
        self.0
    }

    fn is_boolean_attr(&self) -> bool {
        true
    }

    fn render_attr_value<B: WriteBuf>(&self, _cx: &mut WebContext<::proa_core::NoLoader, B>) -> Result<(), WriteError> {
        Ok(())
    }
}

impl<B: WriteBuf> WebRenderSync<::proa_core::NoLoader, B> for Filter {
    fn render(self, cx: &mut WebContext<::proa_core::NoLoader, B>) -> Result<(), WriteError> {
        html_sync! { <option selected={Selected(self.active)}>"Current"</option> }.render(cx)
    }
}
"#;
    let diagnostics = lint_file(Path::new("sample.rs"), source);
    let bool_wrapper = diagnostics
        .iter()
        .find(|diag| diag.code == "render/bool-attr-wrapper");
    assert!(bool_wrapper.is_some());
    assert_eq!(bool_wrapper.map(|diag| diag.severity), Some(Severity::Info));
    assert!(bool_wrapper
        .and_then(|diag| diag.help.as_deref())
        .is_some_and(|help| help.contains("selected={condition}")));
}

#[test]
fn lint_allows_attr_value_text_macro() {
    let source = r#"
impl AttrValue for AuditHref<'_> {
    fn render_attr_value<B: WriteBuf>(&self, cx: &mut WebContext<::proa_core::NoLoader, B>) -> Result<(), WriteError> {
        text!("/audit/{}", self.id).render_attr_value(cx)
    }
}
"#;
    let diagnostics = lint_file(Path::new("sample.rs"), source);
    assert!(!diagnostics
        .iter()
        .any(|diag| diag.code == "perf/raw-attr-segment"));
    assert!(!diagnostics
        .iter()
        .any(|diag| diag.code == "render/manual-attr-stream"));
}

#[test]
fn lint_skips_complex_attr_value_streams() {
    let source = r#"
impl AttrValue for AuditClass<'_> {
    fn render_attr_value<B: WriteBuf>(&self, cx: &mut WebContext<::proa_core::NoLoader, B>) -> Result<(), WriteError> {
        cx.out.extend_static(b"status-")?;
        for part in self.parts {
            part.render_attr_value(cx)?;
        }
        Ok(())
    }
}
"#;
    let diagnostics = lint_file(Path::new("sample.rs"), source);
    assert!(!diagnostics
        .iter()
        .any(|diag| diag.code == "render/manual-attr-stream"));
}

#[test]
fn lint_reports_render_component_data_shape_warnings() {
    let source = r#"
pub struct Card<C> {
    pub title: String,
    pub children: Vec<C>,
    pub body: Box<dyn WebRenderSync<::proa_core::NoLoader, Vec<u8>>>,
    pub dynamic_body: proa_core::BoxedWebRenderSync<::proa_core::NoLoader, Vec<u8>>,
}

impl<B: WriteBuf, C> WebRenderSync<::proa_core::NoLoader, B> for Card<C> {
    fn render(self, cx: &mut WebContext<::proa_core::NoLoader, B>) -> Result<(), WriteError> {
        Ok(())
    }
}
"#;
    let diagnostics = lint_file(Path::new("sample.rs"), source);
    let codes: Vec<_> = diagnostics.iter().map(|diag| diag.code).collect();
    assert!(codes.contains(&"render/string-child"));
    assert!(codes.contains(&"render/vec-children"));
    assert_eq!(
        codes
            .iter()
            .filter(|code| **code == "render/boxed-child")
            .count(),
        2
    );
}

#[test]
fn lint_ignores_non_render_data_shapes() {
    let source = r#"
pub struct CardData {
    pub title: String,
    pub children: Vec<Row>,
}
"#;
    let diagnostics = lint_file(Path::new("sample.rs"), source);
    assert!(!diagnostics
        .iter()
        .any(|diag| diag.code.starts_with("render/")));
}

#[test]
fn lint_reports_owned_view_models_constructed_in_render() {
    let source = r#"
struct OriginPanelView {
    rows: Vec<String>,
    label: String,
}

impl<B: WriteBuf> WebRenderSync<::proa_core::NoLoader, B> for Panel {
    fn render(self, cx: &mut WebContext<::proa_core::NoLoader, B>) -> Result<(), WriteError> {
        let view = OriginPanelView {
            rows: Vec::new(),
            label: String::new(),
        };
        html_sync! { <section>{view.label}</section> }.render(cx)
    }
}
"#;
    let diagnostics = lint_file(Path::new("sample.rs"), source);
    assert!(diagnostics
        .iter()
        .any(|diag| diag.code == "render/owned-view-model"));
}

#[test]
fn lint_reports_large_render_data_clones() {
    let source = r#"
impl<B: WriteBuf> WebRenderSync<::proa_core::NoLoader, B> for Panel {
    fn render(self, cx: &mut WebContext<::proa_core::NoLoader, B>) -> Result<(), WriteError> {
        let snapshots = self.snapshots.clone();
        html_sync! { <section>{snapshots.len()}</section> }.render(cx)
    }
}
"#;
    let diagnostics = lint_file(Path::new("sample.rs"), source);
    assert!(diagnostics
        .iter()
        .any(|diag| diag.code == "render/clone-large-render-data"));
    assert!(!diagnostics.iter().any(|diag| diag.code == "perf/clone"));
}

#[test]
fn render_copy_flags_unsafe_raw_splice_of_rendered_output() {
    let source = r#"
pub fn render_document(query: &str) -> String {
    let inner = proa_core::render_to_string(Inner { query });
    proa_core::render_to_string(
        Outer { body: UnsafeRaw(&inner) },
    )
}
"#;
    let diagnostics = lint_file(Path::new("sample.rs"), source);
    assert!(
        diagnostics
            .iter()
            .any(|diag| diag.code == "perf/render-copy"),
        "expected perf/render-copy, got: {diagnostics:?}"
    );
}

#[test]
fn render_copy_flags_push_str_concat_of_rendered_output() {
    let source = r#"
pub fn assemble(query: &str) -> String {
    let (chips_html, used) = render_capturing_islands(Chips::default(), Some(1024));
    let mut page = String::from("<!DOCTYPE html>");
    page.push_str(&chips_html);
    page
}
"#;
    let diagnostics = lint_file(Path::new("sample.rs"), source);
    assert!(
        diagnostics
            .iter()
            .any(|diag| diag.code == "perf/render-copy"),
        "expected perf/render-copy, got: {diagnostics:?}"
    );
}

#[test]
fn render_copy_flags_rendered_output_passed_to_another_render() {
    let source = r#"
pub fn page(listings: &[Listing]) -> String {
    let inner = proa_core::render_to_string(List { listings });
    render_document("query", listings, &inner, "")
}
"#;
    let diagnostics = lint_file(Path::new("sample.rs"), source);
    assert!(
        diagnostics
            .iter()
            .any(|diag| diag.code == "perf/render-copy"),
        "expected perf/render-copy, got: {diagnostics:?}"
    );
}

#[test]
fn render_copy_ignores_single_pass_composition() {
    let source = r#"
pub fn page(listings: &[Listing]) -> String {
    proa_core::render_to_string(
        Document { river: List { listings }, tail: Tail },
    )
}
"#;
    let diagnostics = lint_file(Path::new("sample.rs"), source);
    assert!(
        !diagnostics
            .iter()
            .any(|diag| diag.code == "perf/render-copy"),
        "single-pass composition must not flag render-copy: {diagnostics:?}"
    );
}

#[test]
fn render_copy_respects_scope_suppression() {
    let source = r#"
// proa-perf-ok(scope): legacy two-phase path kept for the props editor
pub fn assemble(query: &str) -> String {
    let inner = proa_core::render_to_string(Inner { query });
    let mut page = String::new();
    page.push_str(&inner);
    page
}
"#;
    let diagnostics = lint_file(Path::new("sample.rs"), source);
    assert!(
        !diagnostics
            .iter()
            .any(|diag| diag.code.starts_with("perf/")),
        "scope suppression should cover the whole fn: {diagnostics:?}"
    );
}

#[test]
fn scope_suppression_without_item_reports_invalid() {
    let source = r#"
// proa-perf-ok(scope): floating marker

// just a comment wall, no item for a while
// more comments
// and more
// and more
"#;
    let diagnostics = lint_file(Path::new("sample.rs"), source);
    assert!(
        diagnostics
            .iter()
            .any(|diag| diag.code == "perf/invalid-suppression"),
        "unattached scope markers must be reported: {diagnostics:?}"
    );
}

#[test]
fn empty_sentinel_return_is_info_not_warning() {
    let source = r#"
fn render_tags(cx: &mut WebContext<::proa_core::NoLoader, B>) -> String {
    if nothing_to_do() {
        return String::new();
    }
    build_tags()
}
"#;
    let diagnostics = lint_file(Path::new("sample.rs"), source);
    let sentinel: Vec<_> = diagnostics
        .iter()
        .filter(|diag| diag.code == "perf/empty-sentinel-return")
        .collect();
    assert_eq!(sentinel.len(), 1, "{diagnostics:?}");
    assert_eq!(sentinel[0].severity, Severity::Info);
    assert!(
        !diagnostics
            .iter()
            .any(|diag| diag.code == "perf/string-new"),
        "sentinel return must not double-report as perf/string-new: {diagnostics:?}"
    );
}

#[test]
fn string_with_capacity_is_info_in_render_paths() {
    let source = r#"
fn render_tags(cx: &mut WebContext<::proa_core::NoLoader, B>) -> String {
    let mut out = String::with_capacity(256);
    out
}
"#;
    let diagnostics = lint_file(Path::new("sample.rs"), source);
    let diag = diagnostics
        .iter()
        .find(|diag| diag.code == "perf/string-with-capacity")
        .expect("with_capacity should still be reported");
    assert_eq!(diag.severity, Severity::Info);
}

#[test]
fn unsafe_raw_static_argument_suggests_static_raw() {
    let source = r#"
impl<B: WriteBuf> WebRenderSync<::proa_core::NoLoader, B> for Page {
    fn render(self, cx: &mut WebContext<::proa_core::NoLoader, B>) -> Result<(), WriteError> {
        html_sync! { <style>{ UnsafeRaw(CHROME_STYLE) }</style> }.render(cx)
    }
}
"#;
    let diagnostics = lint_file(Path::new("sample.rs"), source);
    let diag = diagnostics
        .iter()
        .find(|diag| diag.code == "perf/unsafe-raw-string")
        .expect("UnsafeRaw should be reported");
    assert!(
        diag.help.as_deref().unwrap_or("").contains("StaticRaw"),
        "static-looking args should suggest StaticRaw: {:?}",
        diag.help
    );
}

#[test]
fn unsafe_raw_local_static_markup_is_reported() {
    let source = r##"
impl<B: WriteBuf> WebRenderSync<::proa_core::NoLoader, B> for Page {
    fn render(self, cx: &mut WebContext<::proa_core::NoLoader, B>) -> Result<(), WriteError> {
        let check_svg = r#"<svg viewBox="0 0 24 24"><polyline points="20 6 9 17 4 12"/></svg>"#;
        let static_html = r#"<span class="badge">Verified</span>"#;
        html_sync! {
            <span>
                {UnsafeRaw(check_svg)}
                {UnsafeRaw(static_html)}
            </span>
        }
        .render(cx)
    }
}
"##;
    let diagnostics = lint_file(Path::new("sample.rs"), source);
    let static_markup_raw_count = diagnostics
        .iter()
        .filter(|diag| diag.code == "perf/static-markup-raw-local")
        .count();
    assert_eq!(
        static_markup_raw_count, 2,
        "local SVG and HTML strings passed to UnsafeRaw should be reported: {diagnostics:?}"
    );
}

#[test]
fn lint_scans_root_template_blocks_for_unsafe_raw() {
    let source = r#"
impl<B: WriteBuf> WebRenderSync<::proa_core::NoLoader, B> for Page {
    fn render(self, cx: &mut WebContext<::proa_core::NoLoader, B>) -> Result<(), WriteError> {
        html_sync! {
            {UnsafeRaw("<!DOCTYPE html>")}
            <html lang="en"></html>
        }
        .render(cx)
    }
}
"#;
    let diagnostics = lint_file(Path::new("sample.rs"), source);
    assert!(
        diagnostics
            .iter()
            .any(|diag| diag.code == "perf/unsafe-raw-string"),
        "root-level UnsafeRaw blocks should be scanned: {diagnostics:?}"
    );
}

#[test]
fn lint_reports_uncontrolled_static_listbox() {
    let source = r#"
fn render() {
    html_sync! {
        <div>
            <input id="search-input" />
            <div id="search-results" role="listbox"></div>
        </div>
    };
}
"#;
    let diagnostics = lint_file(Path::new("sample.rs"), source);
    let diag = diagnostics
        .iter()
        .find(|diag| diag.code == "a11y/listbox-control")
        .expect("uncontrolled listbox should be reported");
    assert_eq!(diag.severity, Severity::Info);
}

#[test]
fn lint_allows_listbox_with_static_controller() {
    let source = r#"
fn render() {
    html_sync! {
        <div>
            <input id="search-input" aria-controls="search-results" />
            <div id="search-results" role="listbox"></div>
        </div>
    };
}
"#;
    let diagnostics = lint_file(Path::new("sample.rs"), source);
    assert!(
        !diagnostics
            .iter()
            .any(|diag| diag.code == "a11y/listbox-control"),
        "aria-controls should associate the listbox with its controller: {diagnostics:?}"
    );
}

#[test]
fn lint_reports_lowercase_allocation_inside_loop() {
    let source = r#"
fn collect_matches(pages: &[Page], needle: &str) -> usize {
    let mut count = 0;
    for page in pages {
        if page.title.to_ascii_lowercase().contains(needle) {
            count += 1;
        }
    }
    count
}
"#;
    let diagnostics = lint_file(Path::new("sample.rs"), source);
    let diag = diagnostics
        .iter()
        .find(|diag| diag.code == "perf/lowercase-in-loop")
        .expect("lowercase allocation in loop should be reported");
    assert_eq!(diag.severity, Severity::Info);
}

#[test]
fn generated_files_skip_format_lints_but_keep_perf_lints() {
    let source = r#"
//! GENERATED by scripts/gen_example.py. Do not edit.

impl<B: WriteBuf> WebRenderSync<::proa_core::NoLoader, B> for Page {
    fn render(self, cx: &mut WebContext<::proa_core::NoLoader, B>) -> Result<(), WriteError> {
        let label = "x".to_string();
        html_sync! { <div>
        <span>{label}</span>
            </div> }
        .render(cx)
    }
}
"#;
    let diagnostics = lint_file(Path::new("sample.rs"), source);
    assert!(
        !diagnostics
            .iter()
            .any(|diag| diag.code.starts_with("format/")),
        "generated files must skip format lints: {diagnostics:?}"
    );
    assert!(
        diagnostics
            .iter()
            .any(|diag| diag.code == "render/static-string-to-string"),
        "perf lints must still run on generated files: {diagnostics:?}"
    );
}

#[test]
fn raw_html_buffer_flags_push_str_assembly_outside_render_paths() {
    let source = r#"
pub fn build_banner(user_name: &str) -> String {
    let mut out = String::new();
    out.push_str("<div class=\"banner\"><b>");
    out.push_str(user_name);
    out.push_str("</b></div>");
    out
}
"#;
    let diagnostics = lint_file(Path::new("sample.rs"), source);
    assert!(
        diagnostics
            .iter()
            .any(|diag| diag.code == "perf/raw-html-buffer"),
        "route helpers assembling HTML must be flagged: {diagnostics:?}"
    );
}

#[test]
fn raw_html_buffer_ignores_formatter_source_generation() {
    let source = r#"
fn format_element(out: &mut String, tag_name: &str) {
    out.push_str("<>");
    out.push_str("<!DOCTYPE html>");
    out.push_str("</");
    out.push_str(tag_name);
    out.push('>');
}
"#;
    let diagnostics = lint_file(Path::new("proa_cli/src/fmt.rs"), source);
    assert!(
        !diagnostics
            .iter()
            .any(|diag| diag.code == "perf/raw-html-buffer"),
        "formatter source generation must not be reported as raw render assembly: {diagnostics:?}"
    );
}

#[test]
fn raw_html_buffer_flags_write_macro_assembly() {
    let source = r#"
fn tags(urls: &[String]) -> String {
    use std::fmt::Write as _;
    let mut out = String::with_capacity(256);
    for url in urls {
        let _ = write!(out, "<script src=\"{url}\"></script>");
    }
    out
}
"#;
    let diagnostics = lint_file(Path::new("sample.rs"), source);
    assert!(
        diagnostics
            .iter()
            .any(|diag| diag.code == "perf/raw-html-buffer"),
        "write! HTML assembly must be flagged: {diagnostics:?}"
    );
}

#[test]
fn raw_html_buffer_ignores_non_html_strings() {
    let source = r#"
fn label(parts: &[String]) -> String {
    let mut out = String::new();
    for part in parts {
        out.push_str(part);
        out.push_str(", ");
    }
    out
}
"#;
    let diagnostics = lint_file(Path::new("sample.rs"), source);
    assert!(
        !diagnostics
            .iter()
            .any(|diag| diag.code == "perf/raw-html-buffer"),
        "plain string assembly must not flag as HTML: {diagnostics:?}"
    );
}

#[test]
fn scope_suppression_attaches_below_doc_comments() {
    let source = r#"
/// Builds script tags for the islands a page used.
///
/// URLs are content-hashed; no user data flows in.
// proa-perf-ok(scope): sanctioned narrow emitter
fn tags(urls: &[String]) -> String {
    use std::fmt::Write as _;
    let mut out = String::new();
    for url in urls {
        let _ = write!(out, "<script src=\"{url}\"></script>");
    }
    out
}
"#;
    let diagnostics = lint_file(Path::new("sample.rs"), source);
    assert!(
        !diagnostics
            .iter()
            .any(|diag| diag.code.starts_with("perf/")),
        "marker between docs and fn must attach: {diagnostics:?}"
    );
}

#[test]
fn scope_suppression_targets_single_impl_method() {
    let source = r#"
impl Chart {
    // proa-perf-ok(scope): streaming SVG emitter; escaped via escape_text_into
    fn write_svg(&self, out: &mut String) {
        out.push_str("<rect x=\"1\"/>");
    }

    fn unguarded(&self, out: &mut String) {
        out.push_str("<rect x=\"2\"/>");
    }
}
"#;
    let diagnostics = lint_file(Path::new("sample.rs"), source);
    let hits: Vec<_> = diagnostics
        .iter()
        .filter(|diag| diag.code == "perf/raw-html-buffer")
        .collect();
    assert_eq!(
        hits.len(),
        1,
        "only the unguarded method flags: {diagnostics:?}"
    );
    assert!(
        hits[0].line > 8,
        "the suppressed method must not flag: {diagnostics:?}"
    );
}

#[test]
fn lint_reports_static_string_to_string_in_render_paths() {
    let source = r#"
impl<B: WriteBuf> WebRenderSync<::proa_core::NoLoader, B> for Filter {
    fn render(self, cx: &mut WebContext<::proa_core::NoLoader, B>) -> Result<(), WriteError> {
        let label = "Any region".to_string();
        let title = String::from("Current filter");
        html_sync! { <button title={title}>{label}</button> }.render(cx)
    }
}
"#;
    let diagnostics = lint_file(Path::new("sample.rs"), source);
    let static_string_count = diagnostics
        .iter()
        .filter(|diag| diag.code == "render/static-string-to-string")
        .count();
    assert_eq!(static_string_count, 2);
    assert!(!diagnostics.iter().any(|diag| diag.code == "perf/to-string"));
    assert!(!diagnostics
        .iter()
        .any(|diag| diag.code == "perf/string-from"));
}

#[test]
fn lint_reports_async_render_without_await_and_sync_child_candidates() {
    let source = r#"
impl<B: WriteBuf> WebRender<::proa_core::NoLoader, B> for Page {
    fn render(
        self,
        cx: &mut WebContext<::proa_core::NoLoader, B>,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>>
    {
        html! { <main>{Badge { label: "ok" }}</main> }.render(cx)
    }
}
"#;
    let diagnostics = lint_file(Path::new("sample.rs"), source);
    let codes: Vec<_> = diagnostics.iter().map(|diag| diag.code).collect();
    assert!(codes.contains(&"render/async-unneeded"));
    assert!(codes.contains(&"render/sync-child-in-async"));
}

#[test]
fn lint_allows_async_render_with_await() {
    let source = r#"
impl<B: WriteBuf> WebRender<::proa_core::NoLoader, B> for Page {
    fn render(
        self,
        cx: &mut WebContext<::proa_core::NoLoader, B>,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>>
    {
        let data = self.loader.load().await?;
        html! { <main>{data}</main> }.render(cx)
    }
}
"#;
    let diagnostics = lint_file(Path::new("sample.rs"), source);
    assert!(!diagnostics
        .iter()
        .any(|diag| diag.code == "render/async-unneeded"));
}

#[test]
fn awaiting_head_keeps_async_trait_but_body_can_use_sync_template() {
    let source = r#"
impl WebRender for Page {
    const ROLE: ComponentRole = ComponentRole::PAGE;
    async fn render_head(&self, cx: &mut WebContext) -> Result<(), WriteError> {
        let title = self.loader.load().await?;
        cx.head_mut().title(title)
    }
    fn render(self, cx: &mut WebContext) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>> {
        html! { <main>"Body"</main> }.render(cx)
    }
}
"#;
    let diagnostics = lint_file(Path::new("page.rs"), source);
    assert!(!has_code(&diagnostics, "render/async-unneeded"));
    assert!(has_code(&diagnostics, "perf/async-html-static-subtree"));
}

fn has_code(diagnostics: &[Diagnostic], code: &str) -> bool {
    diagnostics.iter().any(|diag| diag.code == code)
}

#[test]
fn lint_allows_async_render_with_generic_async_child_bound() {
    // `C` is only known to be `WebRender`, so this impl cannot be lowered to
    // `WebRenderSync` even though its own body never awaits.
    let inline = r#"
impl<L: DataLoader, B: WriteBuf, C: WebRender<L, B>> WebRender<L, B> for Card<C> {
    fn render(
        self,
        cx: &mut WebContext<L, B>,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>>
    {
        html! { <div class="card">{self.children}</div> }.render(cx)
    }
}
"#;
    let where_clause = r#"
impl<L, B, C> WebRender<L, B> for Card<C>
where
    B: WriteBuf,
    L: DataLoader,
    C: WebRender<L, B>,
{
    fn render(
        self,
        cx: &mut WebContext<L, B>,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>>
    {
        html! { <div class="card">{self.children}</div> }.render(cx)
    }
}
"#;
    for source in [inline, where_clause] {
        let diagnostics = lint_file(Path::new("sample.rs"), source);
        assert!(!has_code(&diagnostics, "render/async-unneeded"));
        assert!(!has_code(&diagnostics, "perf/async-html-static-subtree"));
    }
}

#[test]
fn lint_reports_async_render_whose_only_marker_is_to_async() {
    // `to_async` lifts a *sync* child into an async tree, so it is evidence
    // the render could be synchronous rather than evidence it must be async.
    let source = r#"
impl<B: WriteBuf> WebRender<::proa_core::NoLoader, B> for Page {
    fn render(
        self,
        cx: &mut WebContext<::proa_core::NoLoader, B>,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>>
    {
        html! { <main>{to_async(Badge { label: "ok" })}</main> }.render(cx)
    }
}
"#;
    let diagnostics = lint_file(Path::new("sample.rs"), source);
    assert!(has_code(&diagnostics, "render/async-unneeded"));
}

#[test]
fn lint_allows_async_render_awaiting_inside_a_template_slot() {
    // `syn::visit` stops at the macro boundary, so the await here is only
    // visible to a token-level scan.
    let source = r#"
impl<B: WriteBuf> WebRender<::proa_core::NoLoader, B> for Page {
    fn render(
        self,
        cx: &mut WebContext<::proa_core::NoLoader, B>,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>>
    {
        html! { <main>{self.price.load().await?}</main> }.render(cx)
    }
}
"#;
    let diagnostics = lint_file(Path::new("sample.rs"), source);
    assert!(!has_code(&diagnostics, "render/async-unneeded"));
    assert!(!has_code(&diagnostics, "perf/async-html-static-subtree"));
}

#[test]
fn lint_reports_static_html_subtree_in_a_truly_async_render() {
    let source = r#"
impl<L: DataLoader, B: WriteBuf> WebRender<L, B> for Price {
    fn render(
        self,
        cx: &mut WebContext<L, B>,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>>
    {
        RenderOutcome::pending(async move {
            let amount = self.loader.load().await?;
            html! { <span class="price">"$"{amount}</span> }.render(cx).resolve().await
        })
    }
}
"#;
    let diagnostics = lint_file(Path::new("sample.rs"), source);
    // The impl itself genuinely awaits, so only the macro-level rule fires.
    assert!(!has_code(&diagnostics, "render/async-unneeded"));
    assert!(has_code(&diagnostics, "perf/async-html-static-subtree"));
}

/// The "Running mutations" example from
/// `proa_marketing/src/docs/rsjs/queries-mutations.mdx`, verbatim.
const DOCS_CART_EDITOR: &str = r#"
use std::future::Future;
use proa::{DataLoader, RenderOutcome, WebContext, WebRender, WriteError};
use proa::html;
use rsjs::{rsjs, QueryKey};

pub struct CartEditor;

#[rsjs(component, client)]
impl<L: DataLoader> WebRender<L> for CartEditor {
    fn render(
        self,
        cx: &mut WebContext<L>,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>> {
        RenderOutcome::pending(async move {
            let draft = rsjs::signal(String::new());
            let save = rsjs::mutation::<String, ()>(
                QueryKey::new(("cart", "update")),
                rsjs::FetchRequest {
                    url: "/api/cart",
                    apply_base_path: true,
                    method: Some(rsjs::HttpMethod::Patch),
                    credentials: Some(rsjs::FetchCredentials::SameOrigin),
                    response: Some(rsjs::ResponseKind::Empty),
                    timeout_ms: Some(2_500),
                    ..Default::default()
                },
                &[QueryKey::new(("cart", "summary"))],
            );
            let update_draft = rsjs::event_handler(
                |event: rsjs::InputEvent<rsjs::elem::Input>| {
                    draft.set(event.current_value());
                },
            );
            let submit = rsjs::event_handler(
                |_: rsjs::MouseEvent<rsjs::elem::Button>| {
                    save.run(draft.get());
                },
            );

            html! {
                <div>
                    <input value={draft.get()} on_input={update_draft()} />
                    <button
                        type="button"
                        disabled={save.is_pending()}
                        on_click={submit()}
                    >
                        {if save.is_pending() { "Saving..." } else { "Save" }}
                    </button>
                </div>
            }
            .render(cx)
            .resolve()
            .await
        })
    }
}
"#;

fn blocking_diagnostics(source: &str) -> Vec<Diagnostic> {
    lint_file(Path::new("src/components/cart_editor.rs"), source)
        .into_iter()
        .filter(|diag| diag.severity.rank() >= Severity::Warning.rank())
        .collect()
}

#[test]
fn docs_mutation_island_passes_the_build_check() {
    // `proa build` fails on any warning. RSJS requires `html!` for the tail
    // of an async island and only accepts mutations in one, so neither rule
    // may suggest `html_sync!` here.
    let blocking = blocking_diagnostics(DOCS_CART_EDITOR);
    assert!(blocking.is_empty(), "{blocking:#?}");

    let explicit_return = DOCS_CART_EDITOR
        .replacen("            html! {", "            return html! {", 1)
        .replace("            .await\n", "            .await;\n");
    assert_ne!(explicit_return, DOCS_CART_EDITOR);
    let blocking = blocking_diagnostics(&explicit_return);
    assert!(blocking.is_empty(), "{blocking:#?}");

    let with_options = DOCS_CART_EDITOR
        .replace("rsjs::mutation::<", "rsjs::mutation_with_options::<")
        .replace(
            "&[QueryKey::new((\"cart\", \"summary\"))],",
            "&[QueryKey::new((\"cart\", \"summary\"))],
                rsjs::MutationOptions {
                    retry_count: 1,
                    on_success_invalidate_strategy: rsjs::InvalidationStrategy::Refetch,
                },",
        );
    assert!(with_options.contains("MutationOptions"));
    let blocking = blocking_diagnostics(&with_options);
    assert!(blocking.is_empty(), "{blocking:#?}");
}

/// The ```rust fence in `doc` that contains `marker`.
fn doc_rust_fence<'a>(doc: &'a str, marker: &str) -> &'a str {
    doc.split("\n```")
        .filter_map(|block| block.strip_prefix("rust"))
        .filter_map(|block| block.split_once('\n').map(|(_, body)| body))
        .find(|body| body.contains(marker))
        .unwrap_or_else(|| panic!("no rust fence contains {marker:?}"))
}

#[test]
fn docs_agent_rsjs_examples_pass_the_build_check() {
    // These examples once failed `proa build` on `String::new()`: an
    // empty-string comparison in a slot, and an island seed.
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    for (doc, marker) in [
        ("docs/agent/rsjs-islands.md", "for UserForm"),
        ("docs/agent/cookbook.md", "for LoginForm"),
        ("docs/agent/rsjs-islands.md", "rsjs::island(SearchInput"),
        (
            "docs/agent/api-reference/slot-grammar.md",
            "rsjs::island(SearchInput",
        ),
    ] {
        let text = std::fs::read_to_string(root.join(doc)).unwrap();
        let fence = doc_rust_fence(&text, marker);
        let indent = |text: &str, by: &str| {
            text.lines()
                .map(|line| format!("{by}{line}\n"))
                .collect::<String>()
        };
        let source = if fence.contains("impl") {
            fence.to_owned()
        } else {
            // A template or slot fragment, rendered by its parent component
            // in canonical layout so only the example itself is checked.
            let template = if fence.starts_with("html_sync!") {
                indent(fence, "        ")
            } else {
                format!(
                    "        html_sync! {{\n            <div>\n{}            </div>\n        }}\n",
                    indent(fence, "                ")
                )
            };
            format!(
                "#[rsjs(component, client)]\n\
                 impl<L: DataLoader> WebRenderSync<L> for StandaloneSearch {{\n    \
                 fn render(self, cx: &mut WebContext<L>) -> Result<(), WriteError> {{\n\
                 {template}        .render(cx)\n    }}\n}}\n"
            )
        };
        let blocking = blocking_diagnostics(&source);
        assert!(blocking.is_empty(), "{doc} ({marker}): {blocking:#?}");
    }
}

#[test]
fn rsjs_async_island_awaiting_a_query_initial_keeps_its_html_tail() {
    let source = r#"
#[rsjs(component, client)]
impl<L: DataLoader> WebRender<L> for Price {
    fn render(
        self,
        cx: &mut WebContext<L>,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>> {
        RenderOutcome::pending(async move {
            let initial: Quote = cx.load_json(LoadKey::named("quote", "AAPL")).await?;
            let badge = html! { <b>"live"</b> };
            let quote = rsjs::query_with_initial(
                rsjs::QueryKey::new(("quote", "AAPL")),
                initial,
                "/api/quote",
                rsjs::QueryOptions::default(),
            );
            html! { <span>{quote.data().value}</span> }.render(cx).resolve().await
        })
    }
}
"#;
    let diagnostics = lint_file(Path::new("price.rs"), source);
    let static_subtrees: Vec<_> = diagnostics
        .iter()
        .filter(|diag| diag.code == "perf/async-html-static-subtree")
        .collect();
    // Only the analyzed tail is exempt; a prelude template can still be sync.
    assert_eq!(static_subtrees.len(), 1, "{static_subtrees:#?}");
    assert_eq!(static_subtrees[0].line, 10);
}

#[test]
fn rsjs_async_island_without_async_work_is_told_to_become_sync() {
    let source = r#"
#[rsjs(component, client)]
impl<L: DataLoader> WebRender<L> for Toggle {
    fn render(
        self,
        cx: &mut WebContext<L>,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>> {
        RenderOutcome::pending(async move {
            let open = rsjs::signal(false);
            html! { <button on_click={|_| open.set(!open.get())}>"Toggle"</button> }
                .render(cx)
                .resolve()
                .await
        })
    }
}
"#;
    let diagnostics = lint_file(Path::new("toggle.rs"), source);
    let diag = diagnostics
        .iter()
        .find(|diag| diag.code == "perf/async-html-static-subtree")
        .expect("an island with no async work should still be reported");
    assert!(
        diag.help
            .as_deref()
            .is_some_and(|help| help.contains("implement WebRenderSync")),
        "{diag:#?}"
    );
}

#[test]
fn rsjs_browser_closure_bodies_are_not_render_allocations() {
    let source = r#"
#[rsjs(component, client)]
impl<L: DataLoader> WebRenderSync<L> for Panel {
    fn render(self, cx: &mut WebContext<L>) -> Result<(), WriteError> {
        let mode = rsjs::signal(String::new());
        let choose = rsjs::event_handler(|_: rsjs::MouseEvent<rsjs::elem::Button>| {
            rsjs::batch(|| mode.set("edit".to_string()));
        });
        let pick = rsjs::handler(|value: u32| mode.set(value.to_string()));
        rsjs::interval(1_000, || mode.set(format!("{}", 1)));
        let label = rsjs::memo(|| mode.get().to_string());
        let server = "server".to_string();
        html_sync! {
            <div>
                <button on_click={choose()}>"Edit"</button>
                <button on_click={|_| mode.set("inline".to_string())}>"Inline"</button>
                <input on_input={rsjs::debounce(|event| mode.set(event.value().to_string()), 300)} />
            </div>
        }
        .render(cx)
    }
}
"#;
    let diagnostics = lint_file(Path::new("panel.rs"), source);
    let mut lines: Vec<_> = diagnostics
        .iter()
        .filter(|diag| diag.severity.rank() >= Severity::Warning.rank())
        .map(|diag| (diag.line, diag.code))
        .collect();
    lines.sort();
    // `memo` runs its closure during SSR, so only it and the prelude local
    // remain render-path allocations.
    assert_eq!(
        lines,
        [
            (11, "perf/to-string"),
            (12, "render/static-string-to-string"),
        ],
        "{diagnostics:#?}"
    );
}

#[test]
fn shared_state_provider_seeds_may_own_their_strings() {
    let source = r##"
#[rsjs(component)]
impl<L: DataLoader> WebRenderSync<L> for WorkbenchPage {
    fn render(self, cx: &mut WebContext<L>) -> Result<(), WriteError> {
        html_sync! {
            {rsjs::SharedStateProvider {
                state: WorkbenchState {
                    mode: "select".to_string(),
                    button_bg: "#3366cc".to_string(),
                },
                children: Panel { title: "Tools".to_string() },
            }}
        }
        .render(cx)
    }
}
"##;
    let diagnostics = lint_file(Path::new("workbench.rs"), source);
    let lines: Vec<_> = diagnostics
        .iter()
        .filter(|diag| diag.code == "render/static-string-to-string")
        .map(|diag| diag.line)
        .collect();
    // The derive requires owned fields; the children literal is still checked.
    assert_eq!(lines, [11], "{diagnostics:#?}");
}

#[test]
fn empty_owned_signal_seeds_are_not_render_allocations() {
    let source = r#"
impl<L: DataLoader> WebRenderSync<L> for Form {
    fn render(self, cx: &mut WebContext<L>) -> Result<(), WriteError> {
        let draft = rsjs::signal(String::new());
        let rows = signal::<Vec<Row>>(Vec::new());
        let mut label = String::new();
        let mut items = Vec::new();
        html_sync! { <input value={draft.get()} /> }.render(cx)
    }
}
"#;
    let diagnostics = lint_file(Path::new("form.rs"), source);
    let lines = |code: &str| {
        diagnostics
            .iter()
            .filter(|diag| diag.code == code)
            .map(|diag| diag.line)
            .collect::<Vec<_>>()
    };
    assert_eq!(lines("perf/string-new"), [6]);
    assert_eq!(lines("perf/vec-new"), [7]);
}

#[test]
fn browser_closure_exemption_keeps_server_arguments_and_methods() {
    let source = r#"
#[rsjs(component, client)]
impl<L: DataLoader> WebRenderSync<L> for Probe {
    fn render(self, cx: &mut WebContext<L>) -> Result<(), WriteError> {
        let row = self.handler(format!("row {}", id));
        rsjs::interval(period(format!("{}", ms)), || tick.set(format!("{}", 1)));
        let draft = rsjs::signal(
            String::new(),
        );
        html_sync! { <p>{row}</p> }.render(cx)
    }
}
"#;
    let diagnostics = lint_file(Path::new("probe.rs"), source);
    let mut found: Vec<_> = diagnostics
        .iter()
        .filter(|diag| diag.severity.rank() >= Severity::Warning.rank())
        .map(|diag| (diag.line, diag.column, diag.code))
        .collect();
    found.sort();
    // A method named `handler` and an `interval` period run on the server;
    // only the timer closure is browser code.
    assert_eq!(
        found,
        [(5, 32, "perf/format"), (6, 31, "perf/format")],
        "{diagnostics:#?}"
    );
}

/// Blocking render-allocation findings as sorted `(line, code)` pairs.
fn render_allocations(source: &str) -> Vec<(usize, &'static str)> {
    let mut found: Vec<_> = lint_file(Path::new("src/components/probe.rs"), source)
        .into_iter()
        .filter(|diag| diag.severity.rank() >= Severity::Warning.rank())
        .filter(|diag| {
            diag.code.starts_with("perf/") || diag.code == "render/static-string-to-string"
        })
        .map(|diag| (diag.line, diag.code))
        .collect();
    found.sort();
    found
}

#[test]
fn browser_closure_names_outside_rsjs_items_are_server_calls() {
    // Without `#[rsjs]`, `handler` is whatever the file defines, and even
    // `rsjs::handler` returns its closure unchanged, so the closures can run
    // during SSR.
    let source = r#"
fn handler<F: Fn() -> String>(f: F) -> String { f() }

impl<L: DataLoader> WebRenderSync<L> for Card {
    fn render(self, cx: &mut WebContext<L>) -> Result<(), WriteError> {
        let label = handler(|| format!("{} items", self.count));
        let other = rsjs::handler(|| self.count.to_string());
        html_sync! {
            <p on_click={|_| log(format!("{}", 1))}>{label}{other}</p>
        }
        .render(cx)
    }
}
"#;
    assert_eq!(
        render_allocations(source),
        [
            (6, "perf/format"),
            (7, "perf/to-string"),
            (9, "perf/format"),
        ]
    );

    // The same calls inside an RSJS item are browser code.
    let rsjs = source.replace(
        "impl<L: DataLoader> WebRenderSync<L> for Card",
        "#[rsjs(component, client)]\nimpl<L: DataLoader> WebRenderSync<L> for Card",
    );
    assert_eq!(render_allocations(&rsjs), []);
}

#[test]
fn shared_state_provider_seed_exempts_only_owned_literal_initializers() {
    let source = r##"
#[rsjs(component)]
impl<L: DataLoader> WebRenderSync<L> for WorkbenchPage {
    fn render(self, cx: &mut WebContext<L>) -> Result<(), WriteError> {
        html_sync! {
            {rsjs::SharedStateProvider::<WorkbenchState, Panel> {
                state: WorkbenchState {
                    mode: "select".to_string(),
                    accent: String::from("#3366cc"),
                    tone: "warm".to_owned(),
                    query: String::new(),
                    draft: String::default(),
                    rows: Vec::new(),
                    filters: Filters { owner: std::string::String::new() },
                    title: format!("{} ({})", self.name, self.count),
                    tags: self.tags.iter().cloned().collect(),
                    json: serde_json::to_string(&self.rows).unwrap(),
                    preview: render_to_string(Panel { title: "x" }),
                    name: String::from(self.name),
                    label: self.label.to_string(),
                    ..WorkbenchState::seeded(format!("{}", 1))
                },
                children: Panel { title: "Tools" },
            }}
        }
        .render(cx)
    }
}
"##;
    // Everything except an owned literal or empty seed is per-render work.
    assert_eq!(
        render_allocations(source),
        [
            (15, "perf/format"),
            (16, "perf/collect"),
            (17, "perf/serde-json-to-string"),
            (18, "perf/render-to-string"),
            (19, "perf/string-from"),
            (20, "perf/to-string"),
            (21, "perf/format"),
        ]
    );
}

#[test]
fn rsjs_async_tail_exemption_ignores_awaits_in_nested_futures() {
    // RSJS accepts this body as `WebRenderSync`: the awaits belong to the
    // future and closure the prelude builds, not to the island.
    let source = r#"
#[rsjs(component, client)]
impl<L: DataLoader> WebRender<L> for Toggle {
    fn render(
        self,
        cx: &mut WebContext<L>,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>> {
        RenderOutcome::pending(async move {
            let open = rsjs::signal(false);
            let later = async move { load().await };
            let reload = || async { load().await };
            html! { <button on_click={|_| open.set(!open.get())}>"Toggle"</button> }
                .render(cx)
                .resolve()
                .await
        })
    }
}
"#;
    let diagnostics = lint_file(Path::new("toggle.rs"), source);
    let diag = diagnostics
        .iter()
        .find(|diag| diag.code == "perf/async-html-static-subtree")
        .expect("an await inside a nested future does not make the island async");
    assert!(
        diag.help
            .as_deref()
            .is_some_and(|help| help.contains("implement WebRenderSync")),
        "{diag:#?}"
    );

    let awaited = source.replace(
        "let later = async move { load().await };",
        "let later = load().await;",
    );
    let diagnostics = lint_file(Path::new("toggle.rs"), &awaited);
    assert!(
        !has_code(&diagnostics, "perf/async-html-static-subtree"),
        "{diagnostics:#?}"
    );
}

#[test]
fn rsjs_seed_and_closure_spellings_match_the_regex_lint() {
    // `scripts/proa_perf_lint.py` exempts the same spellings; see
    // `test_rsjs_seed_and_closure_spellings_match_cargo_proa_lint`.
    let source = r#"
#[rsjs(component, client)]
impl<L: DataLoader> WebRenderSync<L> for Panel {
    fn render(self, cx: &mut WebContext<L>) -> Result<(), WriteError> {
        let mode = rsjs::signal(String::new());
        let pairs = signal::<Vec<(u32, String)>>(Vec::new());
        let named = rsjs::signal::<std::string::String>(std::string::String::new());
        html_sync! {
            <div>
                <button on_click={move |_| {
                    mode.set("inline".to_string());
                }}>"Inline"</button>
                {rsjs::SharedStateProvider::<WorkbenchState, Tools> {
                    state: WorkbenchState {
                        mode: "select".to_string(),
                    },
                    children: Tools {},
                }}
            </div>
        }
        .render(cx)
    }
}
"#;
    assert_eq!(render_allocations(source), []);
}

#[test]
fn lint_allows_async_html_with_an_unwrapped_component_child() {
    // An un-wrapped component child is presumed async; `render/sync-child-in-async`
    // owns the case where it is not.
    let source = r#"
impl<L: DataLoader, B: WriteBuf> WebRender<L, B> for Page {
    fn render(
        self,
        cx: &mut WebContext<L, B>,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>>
    {
        RenderOutcome::pending(async move {
            let _ = self.loader.load().await?;
            html! { <main>{AsyncPrice { sku: 1 }}</main> }.render(cx).resolve().await
        })
    }
}
"#;
    let diagnostics = lint_file(Path::new("sample.rs"), source);
    assert!(!has_code(&diagnostics, "perf/async-html-static-subtree"));
}

#[test]
fn lint_allows_async_html_holding_a_suspense_boundary() {
    let source = r#"
impl<L: DataLoader, B: WriteBuf> WebRender<L, B> for Page {
    fn render(
        self,
        cx: &mut WebContext<L, B>,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>>
    {
        html! { <main>{Suspense { child: self.child, fallback: () }}</main> }.render(cx)
    }
}
"#;
    let diagnostics = lint_file(Path::new("sample.rs"), source);
    assert!(!has_code(&diagnostics, "render/async-unneeded"));
    assert!(!has_code(&diagnostics, "perf/async-html-static-subtree"));
}

#[test]
fn lint_allows_sync_completion_bridges() {
    // `SyncToAsync`-style adapters already implement the sync trait; the async
    // impl exists to lift them into an async tree.
    let source = r#"
impl<T, L, B> WebRender<L, B> for SyncToAsync<T>
where
    T: WebRenderSync<L, B>,
    B: WriteBuf,
    L: DataLoader,
{
    fn render(
        self,
        cx: &mut WebContext<L, B>,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>>
    {
        RenderOutcome::<std::future::Ready<Result<(), WriteError>>>::Done(self.0.render(cx))
    }
}
"#;
    let diagnostics = lint_file(Path::new("sample.rs"), source);
    assert!(!has_code(&diagnostics, "render/async-unneeded"));
}

#[test]
fn lint_allows_bodies_that_forward_a_foreign_render_outcome() {
    // `Pending(<call>)` hands back an erased future, and a tail `render*` call
    // forwards whatever the delegate decides. Neither body reveals the answer.
    let erased = r#"
impl<'env, L, B> WebRender<L, B> for AsyncHtml<'env, B, L>
where
    B: WriteBuf,
    L: DataLoader,
{
    fn render(
        self,
        cx: &mut WebContext<L, B>,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>>
    {
        RenderOutcome::Pending((self.render_fn)(cx))
    }
}
"#;
    let delegated = r#"
impl<T, L, B> WebRender<L, B> for LinkedSite<T>
where
    B: WriteBuf,
    L: DataLoader,
{
    fn render(
        self,
        cx: &mut WebContext<L, B>,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>>
    {
        let plan = T::linked_site(self.site.candidate_id, self.site.plan_id);
        T::render_at(self.component, cx, plan)
    }
}
"#;
    for source in [erased, delegated] {
        let diagnostics = lint_file(Path::new("sample.rs"), source);
        assert!(!has_code(&diagnostics, "render/async-unneeded"));
    }
}

#[test]
fn lint_reports_inline_pending_with_no_await() {
    // `pending(async move { .. })` is fully visible, unlike `Pending(<call>)`.
    let source = r#"
impl<T, L, B> WebRender<L, B> for ImmediateAsync<T>
where
    T: WebRenderSync<L, B>,
    B: WriteBuf,
    L: DataLoader,
{
    fn render(
        self,
        cx: &mut WebContext<L, B>,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>>
    {
        RenderOutcome::pending(async move { self.0.render(cx) })
    }
}
"#;
    let diagnostics = lint_file(Path::new("sample.rs"), source);
    assert!(has_code(&diagnostics, "render/async-unneeded"));
}

#[test]
fn lint_allows_templates_holding_a_unit_struct_component_child() {
    // `{StreamingPanels}` parses to `Expr::Path`, not a call or struct literal.
    let source = r#"
impl<L: DataLoader> WebRender<L, Vec<u8>> for MarketplaceStreamPage {
    fn render(
        self,
        cx: &mut WebContext<L, Vec<u8>>,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>>
    {
        html! {
            <main>
                {to_async(MarketplaceHero)}
                <div class="content-grid">{StreamingPanels}</div>
            </main>
        }
        .render(cx)
    }
}
"#;
    let diagnostics = lint_file(Path::new("sample.rs"), source);
    assert!(!has_code(&diagnostics, "render/async-unneeded"));
    assert!(!has_code(&diagnostics, "perf/async-html-static-subtree"));
}

#[test]
fn lint_treats_screaming_case_slots_as_values_not_components() {
    // `{MAX_ITEMS}` is a const, not a unit-struct child. Reading it as an
    // opaque component would silence both rules on any template using one.
    let source = r#"
impl<L: DataLoader, B: WriteBuf> WebRender<L, B> for Page {
    fn render(
        self,
        cx: &mut WebContext<L, B>,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>>
    {
        html! { <div data-cap={MAX_ITEMS}>{PAGE_TITLE}</div> }.render(cx)
    }
}
"#;
    let diagnostics = lint_file(Path::new("sample.rs"), source);
    assert!(has_code(&diagnostics, "render/async-unneeded"));
    assert!(has_code(&diagnostics, "perf/async-html-static-subtree"));
}

#[test]
fn lint_reports_static_subtree_when_every_child_is_lifted() {
    // Both children are `to_async(..)`-lifted sync components, so the whole
    // template can be `html_sync!` and the wrappers dropped.
    let source = r#"
impl<L: DataLoader> WebRender<L, Vec<u8>> for MarketplaceLoadedPage {
    fn render(
        self,
        cx: &mut WebContext<L, Vec<u8>>,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>>
    {
        RenderOutcome::pending(async move {
            let _ = cx.load_json::<()>("panels").await?;
            html! {
                <main>
                    {to_async(MarketplaceHero)}
                    <div class="content-grid">{to_async(LoadedPanels)}</div>
                </main>
            }
            .render(cx)
            .resolve()
            .await
        })
    }
}
"#;
    let diagnostics = lint_file(Path::new("sample.rs"), source);
    assert!(!has_code(&diagnostics, "render/async-unneeded"));
    assert!(has_code(&diagnostics, "perf/async-html-static-subtree"));
}

#[test]
fn lint_allows_md_bodies_that_forward_a_foreign_render_outcome() {
    let source = r#"
impl<'env, B> MdRender<::proa_core::NoLoader, B> for AsyncMd<'env, ::proa_core::NoLoader, B>
where
    B: WriteBuf,
{
    fn render_md(
        self,
        cx: &mut WebContext<::proa_core::NoLoader, B>,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>> {
        RenderOutcome::Pending((self.render_fn)(cx))
    }
}
"#;
    let diagnostics = lint_file(Path::new("sample.rs"), source);
    assert!(!has_code(&diagnostics, "render/md-async-unneeded"));
}

#[test]
fn lint_reports_md_async_unneeded_and_static_md_body() {
    let source = r####"
impl<B: WriteBuf> MdRender<::proa_core::NoLoader, B> for Note {
    fn render_md(
        self,
        cx: &mut WebContext<::proa_core::NoLoader, B>,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>> {
        md! { r###"# Title"### }.render_md(cx)
    }
}
"####;
    let diagnostics = lint_file(Path::new("sample.rs"), source);
    assert!(has_code(&diagnostics, "render/md-async-unneeded"));
    assert!(has_code(&diagnostics, "perf/async-md-static-subtree"));
}

/// Applications import the `proa` facade, so help must not name the
/// internal `proa_core` crate.
#[test]
fn async_template_help_names_the_proa_facade() {
    let source = r####"
impl WebRender for Page {
    fn render(self, cx: &mut WebContext) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>> {
        html! { <main>{Badge { label: "ok" }}</main> }.render(cx)
    }
}
impl WebRender for Shell {
    fn render(self, cx: &mut WebContext) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>> {
        html! { <main>"Body"</main> }.render(cx)
    }
}
impl MdRender for Note {
    fn render_md(self, cx: &mut WebContext) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>> {
        md! { r###"# Title"### }.render_md(cx)
    }
}
"####;
    let diagnostics = lint_file(Path::new("sample.rs"), source);
    for (code, wrapper) in [
        ("render/sync-child-in-async", "proa::to_async(...)"),
        ("perf/async-html-static-subtree", "proa::to_async(...)"),
        ("perf/async-md-static-subtree", "proa::to_md_async(...)"),
    ] {
        let help = diagnostics
            .iter()
            .find(|diag| diag.code == code)
            .and_then(|diag| diag.help.as_deref())
            .unwrap_or_else(|| panic!("{code}: {diagnostics:?}"));
        assert!(help.contains(wrapper), "{code}: {help}");
    }
    for diag in &diagnostics {
        let text = format!("{diag}");
        assert!(!text.contains("proa_core::"), "{text}");
    }
}

#[test]
fn lint_allows_md_render_that_awaits_in_a_template_slot() {
    let source = r####"
impl<B: WriteBuf> MdRender<::proa_core::NoLoader, B> for Note {
    fn render_md(
        self,
        cx: &mut WebContext<::proa_core::NoLoader, B>,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>> {
        md! { r###"# {self.title.load().await?}"### }.render_md(cx)
    }
}
"####;
    let diagnostics = lint_file(Path::new("sample.rs"), source);
    assert!(!has_code(&diagnostics, "render/md-async-unneeded"));
    assert!(!has_code(&diagnostics, "perf/async-md-static-subtree"));
}

#[test]
fn lint_allows_md_render_with_a_generic_async_child_bound() {
    let source = r####"
impl<B: WriteBuf, C: MdRender<::proa_core::NoLoader, B>> MdRender<::proa_core::NoLoader, B> for Section<C> {
    fn render_md(
        self,
        cx: &mut WebContext<::proa_core::NoLoader, B>,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>> {
        md! { r###"# Title"### }.render_md(cx)
    }
}


"####;
    let diagnostics = lint_file(Path::new("sample.rs"), source);
    assert!(!has_code(&diagnostics, "render/md-async-unneeded"));
    assert!(!has_code(&diagnostics, "perf/async-md-static-subtree"));
}

#[test]
fn async_markdown_recognizes_opaque_children_but_ignores_code_fence_braces() {
    let source = r####"
impl MdRender for Page {
    fn render_md(self, cx: &mut WebContext) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>> {
        md! { r###"{DatabaseBadge}"### }.render_md(cx)
    }
}
"####;
    let diagnostics = lint_file(Path::new("sample.rs"), source);
    assert!(!has_code(&diagnostics, "render/md-async-unneeded"));
    assert!(!has_code(&diagnostics, "perf/async-md-static-subtree"));
    let code_fence = source.replace("{DatabaseBadge}", "```rust\n{DatabaseBadge}\n```");
    let diagnostics = lint_file(Path::new("sample.rs"), &code_fence);
    assert!(has_code(&diagnostics, "render/md-async-unneeded"));
    assert!(has_code(&diagnostics, "perf/async-md-static-subtree"));
}

#[test]
fn async_markdown_recognizes_opaque_children_inside_control_flow_slots() {
    for slot in [
        "{if self.ready { DatabaseBadge }}",
        "{if self.ready { \"static\" } else { DatabaseBadge }}",
        "{for _row in self.rows.iter() { DatabaseBadge }}",
        "{loop { break DatabaseBadge; }}",
        "{'done: loop { if self.ready { break 'done DatabaseBadge; } }}",
        "{match self.state { State::Ready => DatabaseBadge, _ => \"pending\" }}",
        r##"{if self.ready { md! { r#"{DatabaseBadge}"# } }}"##,
    ] {
        let source = format!(
            r####"
impl MdRender for Page {{
    fn render_md(self, cx: &mut WebContext) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>> {{
        md! {{ r###"{slot}"### }}.render_md(cx)
    }}
}}
"####
        );
        let diagnostics = lint_file(Path::new("sample.rs"), &source);
        assert!(
            !has_code(&diagnostics, "render/md-async-unneeded"),
            "{slot}"
        );
        assert!(
            !has_code(&diagnostics, "perf/async-md-static-subtree"),
            "{slot}"
        );
    }
    let source = r####"
impl MdRender for Page {
    fn render_md(self, cx: &mut WebContext) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>> {
        md! { r###"{if self.ready { "ready" } else { "pending" }}"### }.render_md(cx)
    }
}
"####;
    let diagnostics = lint_file(Path::new("sample.rs"), source);
    assert!(has_code(&diagnostics, "render/md-async-unneeded"));
    assert!(has_code(&diagnostics, "perf/async-md-static-subtree"));
}

#[test]
fn async_markdown_inline_templates_recognize_opaque_children_inside_control_flow() {
    let source = r####"
impl MdRender for Page {
    fn render_md(self, cx: &mut WebContext) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>> {
        md! { "# Status\n{if self.ready { DatabaseBadge }}" }.render_md(cx)
    }
}
"####;
    let diagnostics = lint_file(Path::new("sample.rs"), source);
    assert!(!has_code(&diagnostics, "render/md-async-unneeded"));
    assert!(!has_code(&diagnostics, "perf/async-md-static-subtree"));
}

#[test]
fn lint_leaves_md_sync_and_html_sync_alone() {
    let source = r####"
impl<B: WriteBuf> MdRenderSync<::proa_core::NoLoader, B> for Note {
    fn render_md(self, cx: &mut WebContext<::proa_core::NoLoader, B>) -> Result<(), WriteError> {
        md_sync! { r###"# Title"### }.render_md(cx)
    }
}

impl<L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for Card {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        html_sync! { <div class="card">"hi"</div> }.render(cx)
    }
}
"####;
    let diagnostics = lint_file(Path::new("sample.rs"), source);
    assert!(!has_code(&diagnostics, "render/md-async-unneeded"));
    assert!(!has_code(&diagnostics, "perf/async-md-static-subtree"));
    assert!(!has_code(&diagnostics, "perf/async-html-static-subtree"));
    assert!(!has_code(&diagnostics, "render/async-unneeded"));
}

#[test]
fn lint_reports_route_data_loader_hints() {
    let source = r#"
struct DataLoader;
struct AppDb;

pub async fn index(State(loader): State<DataLoader>) -> Html<String> {
    Html(String::new())
}

async fn audit_page(Extension(db): Extension<AppDb>) -> Html<String> {
    let snapshots = db.latest_snapshots();
    Html(String::new())
}
"#;
    let diagnostics = lint_file(Path::new("sample.rs"), source);
    let direct = diagnostics
        .iter()
        .find(|diag| diag.code == "route/direct-db-load");
    let prewarm = diagnostics
        .iter()
        .find(|diag| diag.code == "route/missing-prewarm");
    assert_eq!(direct.map(|diag| diag.severity), Some(Severity::Info));
    assert_eq!(prewarm.map(|diag| diag.severity), Some(Severity::Info));
}

#[test]
fn lint_does_not_scan_inherent_render_methods() {
    let source = r#"
impl Card {
    fn render(self) {
        let label = format!("{}", self.title);
    }
}
"#;
    let diagnostics = lint_file(Path::new("sample.rs"), source);
    assert!(diagnostics.is_empty());
}

#[test]
fn render_method_suppression_comments_work() {
    let source = r#"
impl<B: WriteBuf> WebRenderSync<::proa_core::NoLoader, B> for Card {
    fn render(self, cx: &mut WebContext<::proa_core::NoLoader, B>) -> Result<(), WriteError> {
        // proa-perf-ok: this fixture validates render-method suppression
        let label = format!("{}", self.title);
        label.render(cx)
    }
}
"#;
    let diagnostics = lint_file(Path::new("sample.rs"), source);
    assert!(diagnostics.is_empty());
}

#[test]
fn suppression_comments_survive_lifetimes_on_the_same_line() {
    let source = r#"
impl<B: WriteBuf> WebRenderSync<::proa_core::NoLoader, B> for Card {
    fn render(self, cx: &mut WebContext<::proa_core::NoLoader, B>) -> Result<(), WriteError> {
        let label: &'static str = leak(format!("{}", self.title)); // proa-perf-ok: lifetime fixture
        label.render(cx)
    }
}
"#;
    let diagnostics = lint_file(Path::new("sample.rs"), source);
    assert!(
        diagnostics.is_empty(),
        "suppression on a line containing a lifetime was dropped: {diagnostics:?}"
    );
}

#[test]
fn line_comment_start_handles_lifetimes_and_char_literals() {
    assert_eq!(
        line_comment_start("fn f<'a>() {} // proa-perf-ok: reason"),
        Some(16)
    );
    assert_eq!(line_comment_start("let c = 'x'; // note"), Some(15));
    assert_eq!(line_comment_start("let s = \"// not a comment\";"), None);
    assert_eq!(line_comment_start("let q = '\\''; // escaped"), Some(16));
}

#[test]
fn lint_scans_direct_render_helpers() {
    let source = r#"
impl<B: WriteBuf> WebRenderSync<::proa_core::NoLoader, B> for Card {
    fn render(self, cx: &mut WebContext<::proa_core::NoLoader, B>) -> Result<(), WriteError> {
        render_title(self.title).render(cx)
    }
}

fn render_title(title: &str) -> String {
    let mut html = String::new();
    html.push_str(&format!("<h1>{title}</h1>"));
    html
}
"#;
    let diagnostics = lint_file(Path::new("sample.rs"), source);
    let codes: Vec<_> = diagnostics.iter().map(|diag| diag.code).collect();
    assert!(codes.contains(&"perf/string-new"));
    assert!(codes.contains(&"perf/push-str"));
    assert!(codes.contains(&"perf/format"));
    assert!(codes.contains(&"perf/format-push-str-loop"));
}

#[test]
fn lint_scans_helpers_inside_macro_args_recursively() {
    let source = r#"
impl<B: WriteBuf> WebRenderSync<::proa_core::NoLoader, B> for Card {
    fn render(self, cx: &mut WebContext<::proa_core::NoLoader, B>) -> Result<(), WriteError> {
        macro_rules! render_section {
            ($name:literal, $expr:expr) => {{
                let result = $expr;
                result
            }};
        }
        render_section!("chart", render_chart(cx))
    }
}

fn render_chart<B: WriteBuf>(cx: &mut WebContext<::proa_core::NoLoader, B>) -> Result<(), WriteError> {
    render_chart_inner(cx)
}

fn render_chart_inner<B: WriteBuf>(_cx: &mut WebContext<::proa_core::NoLoader, B>) -> Result<(), WriteError> {
    let mut html = String::new();
    html.push_str("<div class=\"chart\">");
    html.push_str(&format!("<span>{}</span>", 1));
    Ok(())
}
"#;
    let diagnostics = lint_file(Path::new("sample.rs"), source);
    let codes: Vec<_> = diagnostics.iter().map(|diag| diag.code).collect();
    assert!(codes.contains(&"perf/raw-html-buffer"));
    assert!(codes.contains(&"perf/format-push-str-loop"));
}

#[test]
fn lint_scans_standalone_render_functions() {
    let source = r#"
pub(crate) fn render_fragment<B: WriteBuf>(cx: &mut WebContext<::proa_core::NoLoader, B>) -> Result<(), WriteError> {
    render_fragment_row(cx)
}

fn render_fragment_row<B: WriteBuf>(cx: &mut WebContext<::proa_core::NoLoader, B>) -> Result<(), WriteError> {
    WriteBuf::extend(&mut cx.out, b"<div class=\"row\">")?;
    WriteBuf::extend(&mut cx.out, b"</div>")
}
"#;
    let diagnostics = lint_file(Path::new("sample.rs"), source);
    assert!(diagnostics
        .iter()
        .any(|diag| diag.code == "perf/raw-write-html"));
}

#[test]
fn lint_reports_string_display_helpers_called_from_render() {
    let source = r#"
impl<B: WriteBuf> WebRenderSync<::proa_core::NoLoader, B> for Chart {
    fn render(self, cx: &mut WebContext<::proa_core::NoLoader, B>) -> Result<(), WriteError> {
        html_sync! { <span>{fmt_ms(self.ms)}</span> }.render(cx)
    }
}

fn fmt_ms(ms: u64) -> String {
    format!("{ms} ms")
}
"#;
    let diagnostics = lint_file(Path::new("sample.rs"), source);
    let codes: Vec<_> = diagnostics.iter().map(|diag| diag.code).collect();
    assert!(codes.contains(&"render/string-display-helper"));
    assert!(codes.contains(&"perf/format"));
}

#[test]
fn lint_reports_manual_escape_helpers_called_from_render() {
    let source = r#"
impl<B: WriteBuf> WebRenderSync<::proa_core::NoLoader, B> for Comment {
    fn render(self, cx: &mut WebContext<::proa_core::NoLoader, B>) -> Result<(), WriteError> {
        html_sync! { <span>{escape_label(self.label)}</span> }.render(cx)
    }
}

fn escape_label(label: &str) -> String {
    let mut escaped = String::with_capacity(label.len());
    for ch in label.chars() {
        match ch {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            _ => escaped.push(ch),
        }
    }
    escaped
}
"#;
    let diagnostics = lint_file(Path::new("sample.rs"), source);
    let codes: Vec<_> = diagnostics.iter().map(|diag| diag.code).collect();
    assert!(codes.contains(&"perf/manual-escape-helper"));
    assert!(codes.contains(&"perf/string-with-capacity"));
    assert!(codes.contains(&"perf/push-str"));
    assert!(!codes.contains(&"render/string-display-helper"));
}

#[test]
fn lint_reports_escape_helpers_defined_next_to_render_code() {
    let source = r#"
impl<B: WriteBuf> WebRenderSync<::proa_core::NoLoader, B> for GlossTerm {
    fn render(self, cx: &mut WebContext<::proa_core::NoLoader, B>) -> Result<(), WriteError> {
        html_sync! { <span>{self.code}</span> }.render(cx)
    }
}

fn esc(label: &str) -> String {
    let mut escaped = String::with_capacity(label.len());
    for ch in label.chars() {
        match ch {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            _ => escaped.push(ch),
        }
    }
    escaped
}
"#;
    let diagnostics = lint_file(Path::new("sample.rs"), source);
    assert!(diagnostics
        .iter()
        .any(|diag| diag.code == "perf/manual-escape-helper"
            && diag
                .help
                .as_deref()
                .is_some_and(|help| help.contains("let html_sync!/AttrValue rendering escape"))));
}

#[test]
fn lint_reports_html_string_helpers_defined_next_to_render_code() {
    let source = r##"
impl<B: WriteBuf> WebRenderSync<::proa_core::NoLoader, B> for GlossTerm {
    fn render(self, cx: &mut WebContext<::proa_core::NoLoader, B>) -> Result<(), WriteError> {
        html_sync! { <span>{self.code}</span> }.render(cx)
    }
}

pub fn term_html(code: &str) -> String {
    format!(
        r#"<a class="gloss-term" href="/terms/{code}"><span>{code}</span></a>"#,
        code = code,
    )
}
"##;
    let diagnostics = lint_file(Path::new("sample.rs"), source);
    assert!(diagnostics
        .iter()
        .any(|diag| diag.code == "perf/raw-html-buffer"
            && diag
                .help
                .as_deref()
                .is_some_and(|help| help.contains("return a WebRenderSync component"))));
}

#[test]
fn lint_does_not_report_escape_named_helpers_without_escape_shape() {
    let source = r#"
impl<B: WriteBuf> WebRenderSync<::proa_core::NoLoader, B> for Comment {
    fn render(self, cx: &mut WebContext<::proa_core::NoLoader, B>) -> Result<(), WriteError> {
        html_sync! { <span>{escape_velocity(self.speed)}</span> }.render(cx)
    }
}

fn escape_velocity(speed: u64) -> String {
    format!("{speed} km/s")
}
"#;
    let diagnostics = lint_file(Path::new("sample.rs"), source);
    assert!(!diagnostics
        .iter()
        .any(|diag| diag.code == "perf/manual-escape-helper"));
}

#[test]
fn lint_does_not_report_uncalled_string_display_helpers() {
    let source = r#"
fn fmt_ms(ms: u64) -> String {
    format!("{ms} ms")
}
"#;
    let diagnostics = lint_file(Path::new("sample.rs"), source);
    assert!(!diagnostics
        .iter()
        .any(|diag| diag.code == "render/string-display-helper"));
}

#[test]
fn lint_does_not_scan_uncalled_helpers() {
    let source = r#"
impl<B: WriteBuf> WebRenderSync<::proa_core::NoLoader, B> for Card {
    fn render(self, cx: &mut WebContext<::proa_core::NoLoader, B>) -> Result<(), WriteError> {
        self.title.render(cx)
    }
}

fn render_title(title: &str) -> String {
    format!("<h1>{title}</h1>")
}
"#;
    let diagnostics = lint_file(Path::new("sample.rs"), source);
    // Reachability-scoped allocation lints must not fire on the
    // uncalled helper, but hand-assembled HTML is flagged everywhere
    // (it bypasses escaping no matter who calls it).
    assert!(
        diagnostics
            .iter()
            .all(|diag| diag.code == "perf/raw-html-buffer"),
        "{diagnostics:?}"
    );
    assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
}

#[test]
fn direct_helper_scan_is_module_local() {
    let source = r#"
mod one {
    impl<B: WriteBuf> WebRenderSync<::proa_core::NoLoader, B> for Card {
        fn render(self, cx: &mut WebContext<::proa_core::NoLoader, B>) -> Result<(), WriteError> {
            render_title(self.title).render(cx)
        }
    }
}

mod two {
    fn render_title(title: &str) -> String {
        format!("<h1>{title}</h1>")
    }
}
"#;
    let diagnostics = lint_file(Path::new("sample.rs"), source);
    // The cross-module helper stays out of reachability-scoped lints,
    // but the hand-assembled HTML rule applies everywhere.
    assert!(
        diagnostics
            .iter()
            .all(|diag| diag.code == "perf/raw-html-buffer"),
        "{diagnostics:?}"
    );
    assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
}

#[test]
fn lint_suppression_comments_require_reason_and_suppress_previous_line() {
    let source = r#"
fn render() {
    // proa-perf-ok: fixture intentionally checks suppression plumbing
    html_sync! { <div>{format!("hello {}", name)}</div> };
    // proa-perf-ok
    html_sync! { <div>{count.to_string()}</div> };
}
"#;
    let diagnostics = lint_file(Path::new("sample.rs"), source);
    let codes: Vec<_> = diagnostics.iter().map(|diag| diag.code).collect();
    assert!(!codes.contains(&"perf/format"));
    assert!(codes.contains(&"perf/invalid-suppression"));
    assert!(codes.contains(&"perf/to-string"));
}

#[test]
fn lint_skips_cfg_test_modules_by_default() {
    let source = r#"
#[cfg(test)]
mod tests {
    fn render() {
        html_sync! { <div>{format!("hello {}", name)}</div> };
    }
}
"#;
    let diagnostics = lint_file(Path::new("sample.rs"), source);
    assert!(diagnostics.is_empty());

    let diagnostics = lint_file_with_options(
        Path::new("sample.rs"),
        source,
        LintOptions {
            include_tests: true,
            ..Default::default()
        },
    );
    assert!(diagnostics.iter().any(|diag| diag.code == "perf/format"));
}

#[test]
fn lint_scans_rsjs_impls_for_client_footguns() {
    let source = r#"
#[rsjs(component)]
impl<B: WriteBuf> WebRenderSync<::proa_core::NoLoader, B> for Counter {
    fn render(self, cx: &mut WebContext<::proa_core::NoLoader, B>) -> Result<(), WriteError> {
        let count = signal(0_i64);
        let cloned = count.clone();
        set_timeout(|| {}, 10);
        html_sync! { <button>{cloned.get()}</button> }.render(cx)
    }
}
"#;
    let diagnostics = lint_file(Path::new("sample.rs"), source);
    let codes: Vec<_> = diagnostics.iter().map(|diag| diag.code).collect();
    assert!(codes.contains(&"rsjs/signal-clone"));
    assert!(codes.contains(&"rsjs/effect-leak"));
}

#[test]
fn lint_allows_participant_component_without_client_primitives() {
    let source = r#"
#[rsjs(component)]
impl<B: WriteBuf> WebRenderSync<::proa_core::NoLoader, B> for StaticPanel {
    fn render(self, cx: &mut WebContext<::proa_core::NoLoader, B>) -> Result<(), WriteError> {
        let label = "Status";
        html_sync! {
            <section class={rsjs::server_attr("panel")}>
                <h2>{rsjs::server_only(label)}</h2>
                <p>"Ready"</p>
            </section>
        }.render(cx)
    }
}
"#;
    let diagnostics = lint_file(Path::new("sample.rs"), source);
    assert!(
        !diagnostics
            .iter()
            .any(|diag| diag.code == "rsjs/unnecessary-island"),
        "a server-only participant can still publish a composable component ABI: {diagnostics:#?}"
    );
}

#[test]
fn lint_recognizes_typed_shared_state_and_batch_as_client_primitives() {
    let source = r#"
#[rsjs(component, client)]
impl<B: WriteBuf> WebRenderSync<::proa_core::NoLoader, B> for ThemePanel {
    fn render(self, cx: &mut WebContext<::proa_core::NoLoader, B>) -> Result<(), WriteError> {
        let workbench = rsjs::shared::<WorkbenchState>();
        let button_bg = workbench.button_bg();
        html_sync! {
            <button on_click={|_| rsjs::batch(|| {
                button_bg.set("green".to_string());
            })}>
                {button_bg.get()}
            </button>
        }.render(cx)
    }
}
"#;
    let diagnostics = lint_file(Path::new("sample.rs"), source);

    assert!(
        !diagnostics
            .iter()
            .any(|diag| diag.code == "slot/invalid-expression"),
        "batch and typed shared fields are valid rsjs handler expressions: {diagnostics:#?}"
    );
    assert!(
        !diagnostics
            .iter()
            .any(|diag| diag.code == "rsjs/unnecessary-island"),
        "a reachable shared field requires hydration: {diagnostics:#?}"
    );
}

#[test]
fn lint_allows_module_path_server_helpers_named_batch_and_shared() {
    let source = r#"
#[rsjs(component, client)]
impl<B: WriteBuf> WebRenderSync<::proa_core::NoLoader, B> for PricePanel {
    fn render(self, cx: &mut WebContext<::proa_core::NoLoader, B>) -> Result<(), WriteError> {
        let rows = pricing::batch(&self.ids);
        let cache = registry::shared();
        html_sync! {
            <section>
                {rsjs::server_only(pricing::batch(&self.skus))}
                {rsjs::server_only(rows)}
                {rsjs::server_only(cache)}
            </section>
        }.render(cx)
    }
}
"#;
    let diagnostics = lint_file(Path::new("sample.rs"), source);
    assert!(
        !diagnostics
            .iter()
            .any(|diag| diag.code == "rsjs/unsupported-capture"),
        "module-path server helpers named batch/shared are not rsjs primitives, so \
         wrapping them (or locals seeded from them) in rsjs::server_only must lint clean: \
         {diagnostics:#?}"
    );
}

#[test]
fn lint_reports_rsjs_dialect_batch_and_shared_in_server_boundary() {
    let source = r#"
#[rsjs(component, client)]
impl<B: WriteBuf> WebRenderSync<::proa_core::NoLoader, B> for Counter {
    fn render(self, cx: &mut WebContext<::proa_core::NoLoader, B>) -> Result<(), WriteError> {
        html_sync! {
            <div>
                {rsjs::server_only(rsjs::shared::<WorkbenchState>())}
                <span data-run={rsjs::server_attr(batch(|| ()))}>"Run"</span>
            </div>
        }.render(cx)
    }
}
"#;
    let diagnostics = lint_file(Path::new("sample.rs"), source);
    let captures: Vec<_> = diagnostics
        .iter()
        .filter(|diag| diag.code == "rsjs/unsupported-capture")
        .collect();
    assert_eq!(
        captures.len(),
        2,
        "dialect-form batch/shared calls stay client primitives inside server boundaries: \
         {diagnostics:#?}"
    );
    assert!(captures
        .iter()
        .any(|diag| diag.message.contains("`shared`")));
    assert!(captures.iter().any(|diag| diag.message.contains("`batch`")));
}

#[test]
fn lint_defers_module_batch_helper_identity_to_the_semantic_linker() {
    let source = r#"
#[rsjs(component, client)]
impl<B: WriteBuf> WebRenderSync<::proa_core::NoLoader, B> for Tracker {
    fn render(self, cx: &mut WebContext<::proa_core::NoLoader, B>) -> Result<(), WriteError> {
        html_sync! {
            <button on_click={|_| analytics::batch(1_i64)}>"Send"</button>
        }.render(cx)
    }
}
"#;
    let diagnostics = lint_file(Path::new("sample.rs"), source);
    assert!(
        !diagnostics
            .iter()
            .any(|diag| diag.code == "slot/invalid-expression"),
        "a statically spelled module helper is a provisional semantic-linker edge even when its \
         name resembles an RSJS primitive: {diagnostics:#?}"
    );
}

#[test]
fn lint_allows_participant_component_with_server_field_paths() {
    let source = r#"
#[rsjs(component)]
impl<B: WriteBuf> WebRenderSync<::proa_core::NoLoader, B> for UserPanel {
    fn render(self, cx: &mut WebContext<::proa_core::NoLoader, B>) -> Result<(), WriteError> {
        html_sync! {
            <section class={self.user.class}>
                <h2>{self.user.label}</h2>
            </section>
        }.render(cx)
    }
}
"#;
    let diagnostics = lint_file(Path::new("sample.rs"), source);
    assert!(
        !diagnostics
            .iter()
            .any(|diag| diag.code == "rsjs/unnecessary-island"),
        "server-only fields do not make the component ABI annotation unnecessary: {diagnostics:#?}"
    );
}

#[test]
fn lint_treats_prelude_utility_helper_as_server_only() {
    let source = r#"
#[rsjs(component)]
impl<B: WriteBuf> WebRenderSync<::proa_core::NoLoader, B> for PriceChip {
    fn render(self, cx: &mut WebContext<::proa_core::NoLoader, B>) -> Result<(), WriteError> {
        let label = crate::money_display::format_usd_cents(self.cents);
        html_sync! {
            <span>{label}</span>
        }.render(cx)
    }
}
"#;
    let diagnostics = lint_file(Path::new("sample.rs"), source);
    assert!(!diagnostics
        .iter()
        .any(|diag| diag.code == "rsjs/unnecessary-island"));
    assert!(
            !diagnostics
                .iter()
                .any(|diag| diag.code == "rsjs/dead-client-primitive"),
            "ordinary prelude helpers are server code and should not be treated as dead client primitives: {diagnostics:#?}"
        );
}

#[test]
fn lint_reports_removed_component_clients_allowlist() {
    let source = r#"
#[rsjs(component, client, clients(crate::pricing))]
impl<B: WriteBuf> WebRenderSync<::proa_core::NoLoader, B> for PriceChip {
    fn render(self, cx: &mut WebContext<::proa_core::NoLoader, B>) -> Result<(), WriteError> {
        html_sync! { <span>"Price"</span> }.render(cx)
    }
}
"#;
    let diagnostics = lint_file(Path::new("sample.rs"), source);
    let diag = diagnostics
        .iter()
        .find(|diag| diag.code == "rsjs/removed-clients-allowlist")
        .expect("removed clients syntax should get a focused migration diagnostic");
    assert_eq!(diag.severity, Severity::Error);
    assert!(diag.message.contains("was removed"));
    assert!(diag.help.as_deref().is_some_and(|help| {
        help.contains("#[rsjs::client]")
            && help.contains("linked automatically")
            && help.contains("#[rsjs::client_module]")
    }));
}

#[test]
fn lint_allows_participant_component_with_noop_handler_prop() {
    let source = r#"
#[rsjs(component)]
impl<B: WriteBuf> WebRenderSync<::proa_core::NoLoader, B> for StaticPanel {
    fn render(self, cx: &mut WebContext<::proa_core::NoLoader, B>) -> Result<(), WriteError> {
        let on_change = rsjs::noop_handler();
        html_sync! {
            <section>
                {SearchInput {
                    label: "Search",
                    on_change,
                }}
            </section>
        }.render(cx)
    }
}
"#;
    let diagnostics = lint_file(Path::new("sample.rs"), source);
    assert!(!diagnostics
        .iter()
        .any(|diag| diag.code == "rsjs/unnecessary-island"));
    assert!(
            !diagnostics
                .iter()
                .any(|diag| diag.code == "rsjs/dead-client-primitive"),
            "noop_handler locals are inert and should not be reported as dead client primitives: {diagnostics:#?}"
        );
}

#[test]
fn lint_reports_rsjs_unnecessary_server_only_for_simple_values() {
    let source = r#"
#[rsjs(component, client)]
impl<B: WriteBuf> WebRenderSync<::proa_core::NoLoader, B> for Counter {
    fn render(self, cx: &mut WebContext<::proa_core::NoLoader, B>) -> Result<(), WriteError> {
        let count = signal(0_i64);
        let label = "Count";
        let class = "counter";
        html_sync! {
            <button class={rsjs::server_attr(class)} on_click={|_| count.update(|n| n + 1)}>
                {rsjs::server_only(label)}
                {count.get()}
            </button>
        }.render(cx)
    }
}
"#;
    let diagnostics = lint_file(Path::new("sample.rs"), source);
    let count = diagnostics
        .iter()
        .filter(|diag| diag.code == "rsjs/unnecessary-server-only")
        .count();
    assert_eq!(
        count, 2,
        "expected simple server boundary cleanup diagnostics: {diagnostics:#?}"
    );
}

#[test]
fn lint_preserves_explicit_snapshot_boundaries_on_inbound_fields() {
    let source = r#"
#[rsjs(component, client)]
impl<B: WriteBuf> WebRenderSync<::proa_core::NoLoader, B> for Counter {
    fn render(self, cx: &mut WebContext<::proa_core::NoLoader, B>) -> Result<(), WriteError> {
        let count = signal(0_i64);
        html_sync! {
            <button class={rsjs::server_attr(self.class)} on_click={|_| count.update(|n| n + 1)}>
                {rsjs::server_only(self.label)}
                {count.get()}
            </button>
        }.render(cx)
    }
}
"#;
    let diagnostics = lint_file(Path::new("sample.rs"), source);
    assert!(
        !diagnostics
            .iter()
            .any(|diag| diag.code == "rsjs/unnecessary-server-only"),
        "inbound field wrappers are intentional snapshot boundaries: {diagnostics:#?}"
    );
}

#[test]
fn lint_strategy_cache_reports_anchored_unnecessary_server_only() {
    let source = r#"
#[rsjs(component, client)]
impl<B: WriteBuf> WebRenderSync<::proa_core::NoLoader, B> for Counter {
    fn render(self, cx: &mut WebContext<::proa_core::NoLoader, B>) -> Result<(), WriteError> {
        let count = signal(0_i64);
        let label = "Count";
        html_sync! {
            <button on_click={|_| count.update(|n| n + 1)}>
                {rsjs::server_only(label.as_ref())}
                {count.get()}
            </button>
        }.render(cx)
    }
}
"#;
    let surface_line = source
        .lines()
        .position(|line| line.contains("rsjs::server_only(label.as_ref())"))
        .map(|line| line + 1)
        .expect("test source should include a server_only wrapper");
    let surface_column = source
        .lines()
        .nth(surface_line - 1)
        .and_then(|line| line.find("rsjs::server_only"))
        .map(|column| column + 1)
        .expect("test source should include a server_only call");

    let tmp = tempfile::tempdir().unwrap();
    let fingerprint = first_rsjs_impl_fingerprint(source);
    let dir = tmp.path().join("target").join("rsjs").join("strategy");
    std::fs::create_dir_all(&dir).unwrap();
    let sidecar = serde_json::json!({
        "format": "rsjs.strategy",
        "strategy_version": 1,
        "strategy_mode": "manifest",
        "analysis_id": "counter_1234",
        "runtime_asset_id": "counter_1234",
        "struct_name": "Counter",
        "struct_path": "Counter",
        "input_fingerprint": fingerprint,
        "render_strategy": "ssr_hydrate",
        "server_surfaces": [{
            "kind": "server_slot",
            "node": 2,
            "source": "explicit_simple_ssr_boundary",
            "reason": "explicit wrapper around simple SSR projection",
            "source_anchor": {
                "start_line": surface_line,
                "start_column": surface_column
            }
        }],
        "roots": [{ "kind": "event_handler", "node": 1, "event": "click" }],
        "emitted": {
            "wrapper": true,
            "seed_script": true,
            "manifest_registration": true,
            "js_asset": true,
            "component_fragment_asset": true,
            "uses_queries": false,
            "uses_mutations": false
        }
    });
    std::fs::write(dir.join("counter_1234.json"), sidecar.to_string()).unwrap();

    let diagnostics = lint_file_with_options(
        Path::new("sample.rs"),
        source,
        LintOptions {
            rsjs_strategy_cache: Some(RsjsStrategyCacheOptions {
                root: tmp.path().to_path_buf(),
                require_fresh: false,
            }),
            ..Default::default()
        },
    );

    let diag = diagnostics
        .iter()
        .find(|diag| diag.code == "rsjs/unnecessary-server-only")
        .expect("anchored compiler surface should produce a wrapper cleanup warning");
    assert_eq!(diag.line, surface_line);
    assert_eq!(diag.column, surface_column);
    assert!(diag.message.contains("compiler strategy"));
}

#[test]
fn lint_strategy_cache_preserves_inbound_snapshot_boundaries() {
    let source = r#"
#[rsjs(component, client)]
impl<B: WriteBuf> WebRenderSync<::proa_core::NoLoader, B> for Counter {
    fn render(self, cx: &mut WebContext<::proa_core::NoLoader, B>) -> Result<(), WriteError> {
        let count = signal(0_i64);
        html_sync! {
            <button class={rsjs::server_attr(self.class)} on_click={|_| count.update(|n| n + 1)}>
                {count.get()}
            </button>
        }.render(cx)
    }
}
"#;
    let surface_line = source
        .lines()
        .position(|line| line.contains("rsjs::server_attr(self.class)"))
        .map(|line| line + 1)
        .expect("test source should include a server_attr wrapper");
    let surface_column = source
        .lines()
        .nth(surface_line - 1)
        .and_then(|line| line.find("rsjs::server_attr"))
        .map(|column| column + 1)
        .expect("test source should include a server_attr call");

    let tmp = tempfile::tempdir().unwrap();
    let fingerprint = first_rsjs_impl_fingerprint(source);
    let dir = tmp.path().join("target").join("rsjs").join("strategy");
    std::fs::create_dir_all(&dir).unwrap();
    let sidecar = serde_json::json!({
        "format": "rsjs.strategy",
        "strategy_version": 1,
        "strategy_mode": "manifest",
        "analysis_id": "counter_inbound_snapshot",
        "runtime_asset_id": "counter_inbound_snapshot",
        "struct_name": "Counter",
        "struct_path": "Counter",
        "input_fingerprint": fingerprint,
        "render_strategy": "ssr_hydrate",
        "server_surfaces": [{
            "kind": "server_attr",
            "node": 2,
            "attr": "class",
            "source": "explicit_simple_ssr_boundary",
            "reason": "explicit wrapper around simple SSR projection",
            "source_anchor": {
                "start_line": surface_line,
                "start_column": surface_column
            }
        }],
        "roots": [{ "kind": "event_handler", "node": 1, "event": "click" }],
        "emitted": {
            "wrapper": true,
            "seed_script": true,
            "manifest_registration": true,
            "js_asset": true,
            "component_fragment_asset": true,
            "uses_queries": false,
            "uses_mutations": false
        }
    });
    std::fs::write(
        dir.join("counter_inbound_snapshot.json"),
        sidecar.to_string(),
    )
    .unwrap();

    let diagnostics = lint_file_with_options(
        Path::new("sample.rs"),
        source,
        LintOptions {
            rsjs_strategy_cache: Some(RsjsStrategyCacheOptions {
                root: tmp.path().to_path_buf(),
                require_fresh: false,
            }),
            ..Default::default()
        },
    );

    assert!(
        !diagnostics
            .iter()
            .any(|diag| diag.code == "rsjs/unnecessary-server-only"),
        "sidecar-backed lint must preserve inbound snapshot semantics: {diagnostics:#?}"
    );
}

#[test]
fn lint_strategy_cache_skips_unanchored_unnecessary_server_only() {
    let source = r#"
#[rsjs(component, client)]
impl<B: WriteBuf> WebRenderSync<::proa_core::NoLoader, B> for Counter {
    fn render(self, cx: &mut WebContext<::proa_core::NoLoader, B>) -> Result<(), WriteError> {
        let count = signal(0_i64);
        let label = "Count";
        html_sync! {
            <button on_click={|_| count.update(|n| n + 1)}>
                {rsjs::server_only(label.as_ref())}
                {count.get()}
            </button>
        }.render(cx)
    }
}
"#;
    let tmp = tempfile::tempdir().unwrap();
    let fingerprint = first_rsjs_impl_fingerprint(source);
    let dir = tmp.path().join("target").join("rsjs").join("strategy");
    std::fs::create_dir_all(&dir).unwrap();
    let sidecar = serde_json::json!({
        "format": "rsjs.strategy",
        "strategy_version": 1,
        "strategy_mode": "manifest",
        "analysis_id": "counter_1234",
        "runtime_asset_id": "counter_1234",
        "struct_name": "Counter",
        "struct_path": "Counter",
        "input_fingerprint": fingerprint,
        "render_strategy": "ssr_hydrate",
        "server_surfaces": [{
            "kind": "server_slot",
            "node": 2,
            "source": "explicit_simple_ssr_boundary",
            "reason": "explicit wrapper around simple SSR projection"
        }],
        "roots": [{ "kind": "event_handler", "node": 1, "event": "click" }],
        "emitted": {
            "wrapper": true,
            "seed_script": true,
            "manifest_registration": true,
            "js_asset": true,
            "component_fragment_asset": true,
            "uses_queries": false,
            "uses_mutations": false
        }
    });
    std::fs::write(dir.join("counter_1234.json"), sidecar.to_string()).unwrap();

    let diagnostics = lint_file_with_options(
        Path::new("sample.rs"),
        source,
        LintOptions {
            rsjs_strategy_cache: Some(RsjsStrategyCacheOptions {
                root: tmp.path().to_path_buf(),
                require_fresh: false,
            }),
            ..Default::default()
        },
    );

    assert!(
        !diagnostics
            .iter()
            .any(|diag| diag.code == "rsjs/unnecessary-server-only"),
        "unanchored compiler surfaces should not produce method-level warnings: {diagnostics:#?}"
    );
}

#[test]
fn lint_strategy_cache_reports_client_crossing_info() {
    let source = r#"
#[rsjs(component, client)]
impl<B: WriteBuf> WebRenderSync<::proa_core::NoLoader, B> for Counter {
    fn render(self, cx: &mut WebContext<::proa_core::NoLoader, B>) -> Result<(), WriteError> {
        let count = signal(0_i64);
        html_sync! {
            <button on_click={|_| count.update(|n| n + 1)}>
                {count.get()}
            </button>
        }.render(cx)
    }
}
"#;
    let crossing_line = source
        .lines()
        .position(|line| line.contains("signal(0_i64)"))
        .map(|line| line + 1)
        .expect("test source should include signal initializer");
    let crossing_column = source
        .lines()
        .nth(crossing_line - 1)
        .and_then(|line| line.find("signal"))
        .map(|column| column + 1)
        .expect("test source should include signal call");

    let tmp = tempfile::tempdir().unwrap();
    let fingerprint = first_rsjs_impl_fingerprint(source);
    let dir = tmp.path().join("target").join("rsjs").join("strategy");
    std::fs::create_dir_all(&dir).unwrap();
    let sidecar = serde_json::json!({
        "format": "rsjs.strategy",
        "strategy_version": 1,
        "strategy_mode": "manifest",
        "analysis_id": "counter_1234",
        "runtime_asset_id": "counter_1234",
        "struct_name": "Counter",
        "struct_path": "Counter",
        "input_fingerprint": fingerprint,
        "render_strategy": "ssr_hydrate",
        "crossings": [{
            "kind": "signal_initial_value",
            "signal": 0,
            "label": "count",
            "surface": "signal_seed",
            "requirement": "client_encode",
            "source_anchor": {
                "start_line": crossing_line,
                "start_column": crossing_column
            }
        }],
        "roots": [{ "kind": "signal", "id": 0 }],
        "emitted": {
            "wrapper": true,
            "seed_script": true,
            "manifest_registration": true,
            "js_asset": true,
            "component_fragment_asset": true,
            "uses_queries": false,
            "uses_mutations": false
        }
    });
    std::fs::write(dir.join("counter_1234.json"), sidecar.to_string()).unwrap();

    let diagnostics = lint_file_with_options(
        Path::new("sample.rs"),
        source,
        LintOptions {
            rsjs_strategy_cache: Some(RsjsStrategyCacheOptions {
                root: tmp.path().to_path_buf(),
                require_fresh: false,
            }),
            ..Default::default()
        },
    );

    let diag = diagnostics
        .iter()
        .find(|diag| diag.code == "rsjs/client-crossing")
        .expect("anchored compiler crossing should produce an info diagnostic");
    assert_eq!(diag.severity, Severity::Info);
    assert_eq!(diag.line, crossing_line);
    assert_eq!(diag.column, crossing_column);
    assert!(diag.message.contains("count"));
    assert!(diag
        .help
        .as_deref()
        .is_some_and(|help| help.contains("client_encode")));
}

#[test]
fn lint_strategy_cache_keeps_distinct_client_crossings_at_same_anchor() {
    let source = r#"
#[rsjs(component, client)]
impl<B: WriteBuf> WebRenderSync<::proa_core::NoLoader, B> for Price {
    fn render(self, cx: &mut WebContext<::proa_core::NoLoader, B>) -> Result<(), WriteError> {
        let price = query_with_initial("price", "/api/price", self.initial).await?;
        html! {
            <span>{price.data()}</span>
        }.render(cx).await
    }
}
"#;
    let crossing_line = source
        .lines()
        .position(|line| line.contains("query_with_initial"))
        .map(|line| line + 1)
        .expect("test source should include query initializer");
    let crossing_column = source
        .lines()
        .nth(crossing_line - 1)
        .and_then(|line| line.find("query_with_initial"))
        .map(|column| column + 1)
        .expect("test source should include query call");

    let tmp = tempfile::tempdir().unwrap();
    let fingerprint = first_rsjs_impl_fingerprint(source);
    let dir = tmp.path().join("target").join("rsjs").join("strategy");
    std::fs::create_dir_all(&dir).unwrap();
    let source_anchor = serde_json::json!({
        "start_line": crossing_line,
        "start_column": crossing_column
    });
    let sidecar = serde_json::json!({
        "format": "rsjs.strategy",
        "strategy_version": 1,
        "strategy_mode": "manifest",
        "analysis_id": "price_1234",
        "runtime_asset_id": "price_1234",
        "struct_name": "Price",
        "struct_path": "Price",
        "input_fingerprint": fingerprint,
        "render_strategy": "ssr_hydrate",
        "crossings": [
            {
                "kind": "query_key",
                "query": 0,
                "label": "price",
                "surface": "query_metadata_blob",
                "requirement": "query_codec",
                "source_anchor": source_anchor.clone()
            },
            {
                "kind": "query_initial_value",
                "query": 0,
                "label": "price",
                "surface": "query_seed_blob",
                "requirement": "query_codec",
                "source_anchor": source_anchor
            }
        ],
        "roots": [{ "kind": "query", "id": 0 }],
        "emitted": {
            "wrapper": true,
            "seed_script": true,
            "manifest_registration": true,
            "js_asset": true,
            "component_fragment_asset": true,
            "uses_queries": true,
            "uses_mutations": false
        }
    });
    std::fs::write(dir.join("price_1234.json"), sidecar.to_string()).unwrap();

    let diagnostics = lint_file_with_options(
        Path::new("sample.rs"),
        source,
        LintOptions {
            rsjs_strategy_cache: Some(RsjsStrategyCacheOptions {
                root: tmp.path().to_path_buf(),
                require_fresh: false,
            }),
            ..Default::default()
        },
    );

    let crossings: Vec<_> = diagnostics
        .iter()
        .filter(|diag| diag.code == "rsjs/client-crossing")
        .collect();
    assert_eq!(
        crossings.len(),
        2,
        "same-anchor query key and seed crossings should both be reported: {diagnostics:#?}"
    );
    assert!(crossings
        .iter()
        .any(|diag| diag.message.contains("query_metadata_blob")));
    assert!(crossings
        .iter()
        .any(|diag| diag.message.contains("query_seed_blob")));
}

#[test]
fn lint_strategy_cache_reports_descriptor_reactive_prop_crossing() {
    let source = r#"
#[rsjs(component, client)]
impl<B: WriteBuf> WebRenderSync<::proa_core::NoLoader, B> for Parent {
    fn render(self, cx: &mut WebContext<::proa_core::NoLoader, B>) -> Result<(), WriteError> {
        let name = signal(self.name);
        html_sync! {
            <section>
                {NameInput {
                    name: name.get(),
                    on_change: rsjs::noop_handler(),
                }}
            </section>
        }.render(cx)
    }
}
"#;
    let crossing_line = source
        .lines()
        .position(|line| line.contains("name: name.get()"))
        .map(|line| line + 1)
        .expect("test source should include child reactive prop");
    let crossing_column = source
        .lines()
        .nth(crossing_line - 1)
        .and_then(|line| line.find("name:"))
        .map(|column| column + 1)
        .expect("test source should include child reactive prop");

    let tmp = tempfile::tempdir().unwrap();
    let fingerprint = first_rsjs_impl_fingerprint(source);
    let dir = tmp.path().join("target").join("rsjs").join("strategy");
    std::fs::create_dir_all(&dir).unwrap();
    let sidecar = serde_json::json!({
        "format": "rsjs.strategy",
        "strategy_version": 1,
        "strategy_mode": "manifest",
        "analysis_id": "parent_1234",
        "runtime_asset_id": "parent_1234",
        "struct_name": "Parent",
        "struct_path": "Parent",
        "input_fingerprint": fingerprint,
        "render_strategy": "ssr_hydrate",
        "crossings": [{
            "kind": "descriptor_reactive_prop",
            "asset_id": "name_input",
            "prop": "name",
            "root": 6,
            "child_signal": 8,
            "parent_signal": 0,
            "label": "name_input::name",
            "surface": "descriptor_reactive_prop",
            "requirement": "client_encode",
            "source_anchor": {
                "start_line": crossing_line,
                "start_column": crossing_column
            }
        }],
        "roots": [
            { "kind": "signal", "id": 0 },
            { "kind": "component_fragment_mount", "root": 6, "asset_id": "name_input" }
        ],
        "emitted": {
            "wrapper": true,
            "seed_script": true,
            "manifest_registration": true,
            "js_asset": true,
            "component_fragment_asset": true,
            "uses_queries": false,
            "uses_mutations": false
        }
    });
    std::fs::write(dir.join("parent_1234.json"), sidecar.to_string()).unwrap();

    let diagnostics = lint_file_with_options(
        Path::new("sample.rs"),
        source,
        LintOptions {
            rsjs_strategy_cache: Some(RsjsStrategyCacheOptions {
                root: tmp.path().to_path_buf(),
                require_fresh: false,
            }),
            ..Default::default()
        },
    );

    let diag = diagnostics
        .iter()
        .find(|diag| diag.code == "rsjs/client-crossing")
        .expect("descriptor reactive prop crossing should be reported");
    assert_eq!(diag.severity, Severity::Info);
    assert_eq!(diag.line, crossing_line);
    assert_eq!(diag.column, crossing_column);
    assert!(diag.message.contains("descriptor reactive prop `name`"));
    assert!(diag.message.contains("fragment `name_input`"));
    assert!(diag.message.contains("child signal 8"));
    assert!(diag.message.contains("parent signal 0"));
    assert!(diag
        .help
        .as_deref()
        .is_some_and(|help| help.contains("client_encode") && help.contains("server_only")));
}

#[test]
fn lint_allows_rsjs_server_only_for_opaque_values() {
    let source = r#"
#[rsjs(component, client)]
impl<B: WriteBuf> WebRenderSync<::proa_core::NoLoader, B> for Counter {
    fn render(self, cx: &mut WebContext<::proa_core::NoLoader, B>) -> Result<(), WriteError> {
        let count = signal(0_i64);
        html_sync! {
            <button class={rsjs::server_attr(theme_classes(self.kind))} on_click={|_| count.update(|n| n + 1)}>
                {rsjs::server_only(proa_core::UnsafeRaw("<svg></svg>"))}
                {rsjs::server_only(Icon { name: self.icon })}
                {count.get()}
            </button>
        }.render(cx)
    }
}
"#;
    let diagnostics = lint_file(Path::new("sample.rs"), source);
    assert!(
        !diagnostics
            .iter()
            .any(|diag| diag.code == "rsjs/unnecessary-server-only"),
        "opaque server boundaries must remain lint-clean: {diagnostics:#?}"
    );
}

#[test]
fn lint_allows_rsjs_island_with_client_primitives() {
    let source = r#"
#[rsjs(component, client)]
impl<B: WriteBuf> WebRenderSync<::proa_core::NoLoader, B> for Counter {
    fn render(self, cx: &mut WebContext<::proa_core::NoLoader, B>) -> Result<(), WriteError> {
        let count = signal(0_i64);
        html_sync! {
            <button type="button" on_click={|_| count.update(|n| n + 1)}>
                {count.get()}
            </button>
        }.render(cx)
    }
}
"#;
    let diagnostics = lint_file(Path::new("sample.rs"), source);
    assert!(
        !diagnostics
            .iter()
            .any(|diag| diag.code == "rsjs/unnecessary-island"),
        "interactive islands should not be reported as unnecessary: {diagnostics:#?}"
    );
}

#[test]
fn lint_allows_rsjs_island_with_interval_root() {
    let source = r#"
#[rsjs(component, client)]
impl<B: WriteBuf> WebRenderSync<::proa_core::NoLoader, B> for Poller {
    fn render(self, cx: &mut WebContext<::proa_core::NoLoader, B>) -> Result<(), WriteError> {
        rsjs::interval(1_000, || rsjs::document::dispatch_event("tick"));
        html_sync! {
            <p>"Polling"</p>
        }.render(cx)
    }
}
"#;
    let diagnostics = lint_file(Path::new("sample.rs"), source);
    assert!(
        !diagnostics
            .iter()
            .any(|diag| diag.code == "rsjs/unnecessary-island"),
        "interval roots should keep the island necessary: {diagnostics:#?}"
    );
}

#[test]
fn lint_strategy_cache_allows_server_only_participant_component() {
    let source = r#"
#[rsjs(component)]
impl<B: WriteBuf> WebRenderSync<::proa_core::NoLoader, B> for StaticPanel {
    fn render(self, cx: &mut WebContext<::proa_core::NoLoader, B>) -> Result<(), WriteError> {
        html_sync! {
            <section>
                <p>"Ready"</p>
            </section>
        }.render(cx)
    }
}
"#;
    let diagnostics = lint_file(Path::new("sample.rs"), source);
    assert!(!diagnostics
        .iter()
        .any(|diag| diag.code == "rsjs/unnecessary-island"));

    let tmp = tempfile::tempdir().unwrap();
    let fingerprint = first_rsjs_impl_fingerprint(source);
    write_strategy_sidecar_with_fingerprint(
        tmp.path(),
        "static_panel_1234",
        "StaticPanel",
        "StaticPanel",
        "server_only",
        Some(&fingerprint),
        None,
    );
    let diagnostics = lint_file_with_options(
        Path::new("sample.rs"),
        source,
        LintOptions {
            rsjs_strategy_cache: Some(RsjsStrategyCacheOptions {
                root: tmp.path().to_path_buf(),
                require_fresh: false,
            }),
            ..Default::default()
        },
    );
    assert!(
        !diagnostics
            .iter()
            .any(|diag| diag.code == "rsjs/unnecessary-island"),
        "server-only is a valid strategy for a composable participant: {diagnostics:#?}"
    );
}

#[test]
fn lint_strategy_cache_reports_ignored_hydration_strategy_for_server_only_strategy() {
    let source = r#"
#[rsjs(component, strategy = "visible")]
impl<B: WriteBuf> WebRenderSync<::proa_core::NoLoader, B> for StaticPanel {
    fn render(self, cx: &mut WebContext<::proa_core::NoLoader, B>) -> Result<(), WriteError> {
        html_sync! {
            <section>
                <p>"Ready"</p>
            </section>
        }.render(cx)
    }
}
"#;
    let tmp = tempfile::tempdir().unwrap();
    let fingerprint = first_rsjs_impl_fingerprint(source);
    write_strategy_sidecar_with_fingerprint(
        tmp.path(),
        "static_panel_1234",
        "StaticPanel",
        "StaticPanel",
        "server_only",
        Some(&fingerprint),
        Some(("visible", "visible")),
    );
    let diagnostics = lint_file_with_options(
        Path::new("sample.rs"),
        source,
        LintOptions {
            rsjs_strategy_cache: Some(RsjsStrategyCacheOptions {
                root: tmp.path().to_path_buf(),
                require_fresh: false,
            }),
            ..Default::default()
        },
    );
    let codes: Vec<_> = diagnostics.iter().map(|diag| diag.code).collect();
    assert!(!codes.contains(&"rsjs/unnecessary-island"));
    let diag = diagnostics
        .iter()
        .find(|diag| diag.code == "rsjs/ignored-hydration-strategy")
        .expect("server-only deferred strategy should be reported");
    assert_eq!(diag.severity, Severity::Warning);
    assert!(diag
        .help
        .as_deref()
        .is_some_and(|help| help.contains("strategy = \"visible\"")));
}

#[test]
fn lint_strategy_cache_allows_component_for_hydrated_strategy() {
    let source = r#"
#[rsjs(component)]
impl<B: WriteBuf> WebRenderSync<::proa_core::NoLoader, B> for Counter {
    fn render(self, cx: &mut WebContext<::proa_core::NoLoader, B>) -> Result<(), WriteError> {
        html_sync! {
            <section>
                <p>"Ready"</p>
            </section>
        }.render(cx)
    }
}
"#;
    let diagnostics = lint_file(Path::new("sample.rs"), source);
    assert!(!diagnostics
        .iter()
        .any(|diag| diag.code == "rsjs/unnecessary-island"));

    let tmp = tempfile::tempdir().unwrap();
    let fingerprint = first_rsjs_impl_fingerprint(source);
    write_strategy_sidecar_with_fingerprint(
        tmp.path(),
        "counter_1234",
        "Counter",
        "Counter",
        "ssr_hydrate",
        Some(&fingerprint),
        None,
    );
    let diagnostics = lint_file_with_options(
        Path::new("sample.rs"),
        source,
        LintOptions {
            rsjs_strategy_cache: Some(RsjsStrategyCacheOptions {
                root: tmp.path().to_path_buf(),
                require_fresh: false,
            }),
            ..Default::default()
        },
    );
    assert!(
        !diagnostics
            .iter()
            .any(|diag| diag.code == "rsjs/unnecessary-island"),
        "a hydrated component must not be called unnecessary: {diagnostics:#?}"
    );
}

#[test]
fn lint_strategy_cache_reports_component_fragment_mounts() {
    let source = r#"
#[rsjs(component, client)]
impl<B: WriteBuf> WebRenderSync<::proa_core::NoLoader, B> for ParentPanel {
    fn render(self, cx: &mut WebContext<::proa_core::NoLoader, B>) -> Result<(), WriteError> {
        html_sync! {
            <section>
                <p>"Ready"</p>
            </section>
        }.render(cx)
    }
}
"#;
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("target").join("rsjs").join("strategy");
    std::fs::create_dir_all(&dir).unwrap();
    let fingerprint = first_rsjs_impl_fingerprint(source);
    let sidecar = serde_json::json!({
        "format": "rsjs.strategy",
        "strategy_version": 1,
        "strategy_mode": "manifest",
        "analysis_id": "parent_panel_1234",
        "runtime_asset_id": "parent_panel_1234",
        "struct_name": "ParentPanel",
        "struct_path": "ParentPanel",
        "input_fingerprint": fingerprint,
        "render_strategy": "ssr_hydrate",
        "hydration_strategy": {
            "kind": "immediate",
            "label": "immediate"
        },
        "declared_client_modules": [],
        "client_modules": [],
        "roots": [
            {
                "kind": "component_fragment_mount",
                "root": 3,
                "asset_id": "child_card_abcd1234"
            }
        ],
        "emitted": {
            "wrapper": true,
            "seed_script": true,
            "manifest_registration": true,
            "js_asset": true,
            "component_fragment_asset": true,
            "uses_queries": false,
            "uses_mutations": false
        }
    });
    std::fs::write(dir.join("parent_panel_1234.json"), sidecar.to_string()).unwrap();

    let diagnostics = lint_file_with_options(
        Path::new("sample.rs"),
        source,
        LintOptions {
            rsjs_strategy_cache: Some(RsjsStrategyCacheOptions {
                root: tmp.path().to_path_buf(),
                require_fresh: false,
            }),
            ..Default::default()
        },
    );
    assert!(
            !diagnostics
                .iter()
                .any(|diag| diag.code == "rsjs/unnecessary-island"),
            "a component with a hydrated child fragment must not be called unnecessary: {diagnostics:#?}"
        );
    let diag = diagnostics
        .iter()
        .find(|diag| diag.code == "rsjs/component-fragment-mount")
        .expect("fragment mount root should be surfaced from strategy sidecar");
    assert_eq!(diag.severity, Severity::Info);
    assert!(diag.message.contains("child_card_abcd1234"));
    assert!(diag.message.contains("root 3"));
    assert!(
            diag.help
                .as_deref()
                .is_some_and(|help| help.contains("schema-v4 descriptor fragments are parent-mounted")),
            "fragment mount diagnostic should explain the current schema-v4 parent-mounted policy: {diag:#?}"
        );
}

#[test]
fn lint_strategy_cache_reports_dead_client_primitives() {
    let source = r#"
#[rsjs(component, client)]
impl<B: WriteBuf> WebRenderSync<::proa_core::NoLoader, B> for StaticPanel {
    fn render(self, cx: &mut WebContext<::proa_core::NoLoader, B>) -> Result<(), WriteError> {
        let count = signal(0_i64);
        html_sync! {
            <section>
                <p>"Ready"</p>
            </section>
        }.render(cx)
    }
}
"#;
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("target").join("rsjs").join("strategy");
    std::fs::create_dir_all(&dir).unwrap();
    let fingerprint = first_rsjs_impl_fingerprint(source);
    let sidecar = serde_json::json!({
        "format": "rsjs.strategy",
        "strategy_version": 1,
        "strategy_mode": "manifest",
        "analysis_id": "static_panel_1234",
        "runtime_asset_id": null,
        "struct_name": "StaticPanel",
        "struct_path": "StaticPanel",
        "input_fingerprint": fingerprint,
        "render_strategy": "server_only",
        "hydration_strategy": {
            "kind": "immediate",
            "label": "immediate"
        },
        "declared_client_modules": [],
        "client_modules": [],
        "dead_primitives": [
            {
                "kind": "signal",
                "signal": 0,
                "label": "count",
                "source_anchor": {
                    "start_line": 5,
                    "start_column": 21
                }
            }
        ],
        "roots": [],
        "emitted": {
            "wrapper": false,
            "seed_script": false,
            "manifest_registration": false,
            "js_asset": false,
            "component_fragment_asset": false,
            "uses_queries": false,
            "uses_mutations": false
        }
    });
    std::fs::write(dir.join("static_panel_1234.json"), sidecar.to_string()).unwrap();

    let diagnostics = lint_file_with_options(
        Path::new("sample.rs"),
        source,
        LintOptions {
            rsjs_strategy_cache: Some(RsjsStrategyCacheOptions {
                root: tmp.path().to_path_buf(),
                require_fresh: false,
            }),
            ..Default::default()
        },
    );
    let diag = diagnostics
        .iter()
        .find(|diag| diag.code == "rsjs/dead-client-primitive")
        .expect("dead primitive sidecar entry should be reported");
    assert_eq!(diag.severity, Severity::Info);
    assert_eq!(diag.line, 5);
    assert_eq!(diag.column, 21);
    assert!(diag.message.contains("`count` signal"));
    assert!(diag.help.as_deref().is_some_and(|help| help.contains(
        "signal seeds, including prop-derived seeds, are serialized only when reachable"
    )));
}

#[test]
fn lint_strategy_cache_reports_unused_client_modules() {
    let source = r#"
#[rsjs(component, client)]
impl<B: WriteBuf> WebRenderSync<::proa_core::NoLoader, B> for PriceTag {
    fn render(self, cx: &mut WebContext<::proa_core::NoLoader, B>) -> Result<(), WriteError> {
        let count = signal(0_i64);
        html_sync! {
            <button on_click={|_| count.set(count.get() + 1)}>
                {count.get()}
            </button>
        }.render(cx)
    }
}
"#;
    let tmp = tempfile::tempdir().unwrap();
    let fingerprint = first_rsjs_impl_fingerprint(source);
    let dir = tmp.path().join("target").join("rsjs").join("strategy");
    std::fs::create_dir_all(&dir).unwrap();
    let value = serde_json::json!({
        "format": "rsjs.strategy",
        "strategy_version": 1,
        "strategy_mode": "manifest",
        "analysis_id": "price_tag_1234",
        "runtime_asset_id": "price_tag_1234",
        "struct_name": "PriceTag",
        "struct_path": "PriceTag",
        "input_fingerprint": fingerprint,
        "render_strategy": "ssr_hydrate",
        "declared_client_modules": ["crate::money", "crate::dates"],
        "client_modules": ["crate::money"],
        "roots": [{ "kind": "text_binding", "node": 1 }],
        "emitted": {
            "wrapper": true,
            "seed_script": true,
            "manifest_registration": true,
            "js_asset": true,
            "component_fragment_asset": true,
            "uses_queries": false,
            "uses_mutations": false
        }
    });
    std::fs::write(dir.join("price_tag_1234.json"), value.to_string()).unwrap();

    let diagnostics = lint_file_with_options(
        Path::new("sample.rs"),
        source,
        LintOptions {
            rsjs_strategy_cache: Some(RsjsStrategyCacheOptions {
                root: tmp.path().to_path_buf(),
                require_fresh: false,
            }),
            ..Default::default()
        },
    );
    let diag = diagnostics
        .iter()
        .find(|diag| diag.code == "rsjs/unused-client-module")
        .expect("unused declared client module should be reported from strategy sidecar");
    assert_eq!(diag.severity, Severity::Info);
    assert!(diag.message.contains("crate::dates"));
    assert!(!diag.message.contains("crate::money,"));
    assert!(diag.help.as_deref().is_some_and(|help| {
        help.contains("#[rsjs::client]")
            && help.contains("#[rsjs::client_module]")
            && !help.contains("#[rsjs(clients(...))]")
    }));
}

#[test]
fn lint_strategy_cache_require_fresh_reports_missing_sidecar() {
    let source = r#"
#[rsjs(component, client)]
impl<B: WriteBuf> WebRenderSync<::proa_core::NoLoader, B> for MissingStrategyCachePanel {
    fn render(self, cx: &mut WebContext<::proa_core::NoLoader, B>) -> Result<(), WriteError> {
        html_sync! {
            <section>
                <p>"Ready"</p>
            </section>
        }.render(cx)
    }
}
"#;
    let tmp = tempfile::tempdir().unwrap();
    let diagnostics = lint_file_with_options(
        Path::new("sample.rs"),
        source,
        LintOptions {
            rsjs_strategy_cache: Some(RsjsStrategyCacheOptions {
                root: tmp.path().to_path_buf(),
                require_fresh: true,
            }),
            ..Default::default()
        },
    );
    let diag = diagnostics
        .iter()
        .find(|diag| diag.code == "rsjs/strategy-cache-missing")
        .expect("strict strategy cache mode should report missing sidecars");
    assert_eq!(diag.severity, Severity::Error);
    assert!(
        !diagnostics
            .iter()
            .any(|diag| diag.code == "rsjs/unnecessary-island"),
        "strict cache error should own this impl instead of falling back: {diagnostics:#?}"
    );
}

#[test]
fn lint_strategy_cache_require_fresh_reports_stale_fingerprint() {
    let source = r#"
#[rsjs(component, client)]
impl<B: WriteBuf> WebRenderSync<::proa_core::NoLoader, B> for StaticPanel {
    fn render(self, cx: &mut WebContext<::proa_core::NoLoader, B>) -> Result<(), WriteError> {
        html_sync! {
            <section>
                <p>"Ready"</p>
            </section>
        }.render(cx)
    }
}
"#;
    let tmp = tempfile::tempdir().unwrap();
    write_strategy_sidecar_with_fingerprint(
        tmp.path(),
        "static_panel_1234",
        "StaticPanel",
        "StaticPanel",
        "server_only",
        Some("sha256:stale"),
        None,
    );

    let diagnostics = lint_file_with_options(
        Path::new("sample.rs"),
        source,
        LintOptions {
            rsjs_strategy_cache: Some(RsjsStrategyCacheOptions {
                root: tmp.path().to_path_buf(),
                require_fresh: true,
            }),
            ..Default::default()
        },
    );
    let diag = diagnostics
        .iter()
        .find(|diag| diag.code == "rsjs/strategy-cache-stale")
        .expect("strict strategy cache mode should report stale fingerprints");
    assert_eq!(diag.severity, Severity::Error);
    assert!(
        !diagnostics
            .iter()
            .any(|diag| diag.code == "rsjs/unnecessary-island"),
        "strict stale-cache error should own this impl instead of falling back: {diagnostics:#?}"
    );
}

#[test]
fn lint_scans_annotated_client_helpers_for_browser_globals() {
    let source = r#"
#[rsjs::client_module]
pub mod helpers {
    #[rsjs::client]
    pub fn mount(selector: String) {
        document::query_selector(selector);
        element.add_event_listener("click", handler);
    }
}
"#;
    let diagnostics = lint_file(Path::new("sample.rs"), source);
    let codes: Vec<_> = diagnostics.iter().map(|diag| diag.code).collect();
    assert!(codes.contains(&"rsjs/window-unchecked"));
    assert!(codes.contains(&"rsjs/nonstatic-selector"));
    assert!(codes.contains(&"rsjs/effect-leak"));
    let unchecked = diagnostics
        .iter()
        .find(|diag| diag.code == "rsjs/window-unchecked")
        .expect("raw browser global should have a focused diagnostic");
    assert!(unchecked.help.as_deref().is_some_and(|help| {
        help.contains("origin-free #[rsjs::client]")
            && !help.contains("move arbitrary DOM access to a client module")
    }));
}

#[test]
fn lint_scans_client_class_methods_for_browser_globals() {
    let source = r#"
#[rsjs::client(class)]
pub mod controller {
    pub struct Controller {
        selector: String,
    }

    impl Controller {
        pub fn new(selector: String) -> Self {
            Self { selector }
        }

        pub fn mount(&self) {
            document::query_selector(self.selector.clone());
            element.add_event_listener("click", handler);
        }
    }
}
"#;
    let diagnostics = lint_file(Path::new("sample.rs"), source);
    let codes: Vec<_> = diagnostics.iter().map(|diag| diag.code).collect();
    assert!(codes.contains(&"rsjs/window-unchecked"));
    assert!(codes.contains(&"rsjs/nonstatic-selector"));
    assert!(codes.contains(&"rsjs/effect-leak"));
}

/// Shared-state arithmetic used to be checked at compile time, badly: the
/// widths came from an on-disk descriptor keyed on the unqualified struct name.
/// That check is gone (see `Analyzer::shared_field_client_type`), so the lint
/// makes the remaining runtime resolution discoverable instead of invisible.
#[test]
fn lint_reports_shared_state_arithmetic_as_runtime_resolved() {
    let source = r#"
#[rsjs(component, client)]
impl<B: WriteBuf> WebRenderSync<::proa_core::NoLoader, B> for CartPanel {
    fn render(self, cx: &mut WebContext<::proa_core::NoLoader, B>) -> Result<(), WriteError> {
        let cart = rsjs::shared::<CartState>();
        let count = cart.count();
        let local = rsjs::signal(0);
        let next = rsjs::memo(|| count.get() + 1);
        let local_next = rsjs::memo(|| local.get() + 1);
        html_sync! {
            <output>{next.get()}</output>
            <span>{local_next.get()}</span>
        }
        .render(cx)
    }
}
"#;
    let diagnostics = lint_file(Path::new("sample.rs"), source);
    let shared: Vec<_> = diagnostics
        .iter()
        .filter(|diag| diag.code == "rsjs/shared-width-dynamic")
        .collect();
    assert_eq!(
        shared.len(),
        1,
        "only the shared-field operand is runtime resolved; island-local signals \
         keep their compile-time width proof: {diagnostics:?}"
    );
    assert!(shared[0].message.contains("count"), "{:?}", shared[0]);
    assert_eq!(shared[0].severity, Severity::Info, "{:?}", shared[0]);
}

#[test]
fn lint_allows_rsjs_browser_helpers() {
    let source = r#"
#[rsjs(component, client)]
impl<B: WriteBuf> WebRenderSync<::proa_core::NoLoader, B> for BrowserEffects {
    fn render(self, cx: &mut WebContext<::proa_core::NoLoader, B>) -> Result<(), WriteError> {
        rsjs::window::scroll_to(0_i64, 10_i64);
        rsjs::document::dispatch_event("ready");
        rsjs::location::assign("/next");
        html_sync! { <button>"Open"</button> }.render(cx)
    }
}
"#;
    let diagnostics = lint_file(Path::new("sample.rs"), source);
    assert!(!diagnostics
        .iter()
        .any(|diag| diag.code.starts_with("rsjs/")));
}

#[test]
fn lint_reports_rsjs_manual_storage_json_parse() {
    let source = r#"
#[rsjs::client]
pub fn restore() -> bool {
    serde_json::from_str(rsjs::local_storage::get_item("enabled").as_str()).unwrap_or(false)
}
"#;
    let diagnostics = lint_file(Path::new("sample.rs"), source);
    assert!(diagnostics
        .iter()
        .any(|diag| diag.code == "rsjs/localstorage-json"));
}

#[test]
fn lint_reports_rsjs_invalid_slot_grammar() {
    let source = r#"
#[rsjs(component, client)]
impl<B: WriteBuf> WebRenderSync<::proa_core::NoLoader, B> for Counter {
    fn render(self, cx: &mut WebContext<::proa_core::NoLoader, B>) -> Result<(), WriteError> {
        let count = signal(0_i64);
        html_sync! {
            <div>
                <button on_click={|event, extra| count.set(extra)}>"Save"</button>
                <input bind_value={count.get()} />
                <span>{match count.get() { 0 => "zero", _ => "many" }}</span>
            </div>
        }.render(cx)
    }
}
"#;
    let diagnostics = lint_file(Path::new("sample.rs"), source);
    let count = diagnostics
        .iter()
        .filter(|diag| diag.code == "slot/invalid-expression")
        .count();
    assert!(
        count >= 3,
        "expected slot grammar diagnostics: {diagnostics:#?}"
    );
}

#[test]
fn lint_defers_reactive_helper_identity_to_the_semantic_linker() {
    let source = r#"
#[rsjs(component, client)]
impl<B: WriteBuf> WebRenderSync<::proa_core::NoLoader, B> for CartTotal {
    fn render(self, cx: &mut WebContext<::proa_core::NoLoader, B>) -> Result<(), WriteError> {
        let count = signal(0_i64);
        html_sync! {
            <span>{pricing::subtotal(count.get(), 2)}</span>
        }.render(cx)
    }
}
"#;
    let diagnostics = lint_file(Path::new("sample.rs"), source);
    assert!(
        !diagnostics
            .iter()
            .any(|diag| diag.code == "slot/invalid-expression"),
        "source lint cannot resolve imports/aliases and must defer helper identity to the \
         relocation-backed linker: {diagnostics:#?}"
    );
}

#[test]
fn lint_allows_one_annotation_rsjs_reactive_helper_call() {
    let source = r#"
#[rsjs(component, client)]
impl<B: WriteBuf> WebRenderSync<::proa_core::NoLoader, B> for CartTotal {
    fn render(self, cx: &mut WebContext<::proa_core::NoLoader, B>) -> Result<(), WriteError> {
        let cents = signal(self.cents);
        html_sync! {
            <span>{pricing::format_cents(cents.get())}</span>
        }.render(cx)
    }
}
"#;
    let diagnostics = lint_file(Path::new("sample.rs"), source);
    assert!(
        !diagnostics
            .iter()
            .any(|diag| diag.code == "slot/invalid-expression"),
        "client helper calls should be accepted provisionally by source lint: {diagnostics:#?}"
    );
    assert!(
        !diagnostics
            .iter()
            .any(|diag| diag.code == "rsjs/unnecessary-island"),
        "client helper calls should keep the source heuristic hydrated: {diagnostics:#?}"
    );
}

#[test]
fn lint_understands_explicit_component_with_linked_client_helper() {
    let source = r#"
#[rsjs(component, client)]
impl<B: WriteBuf> WebRenderSync<::proa_core::NoLoader, B> for CartTotal {
    fn render(self, cx: &mut WebContext<::proa_core::NoLoader, B>) -> Result<(), WriteError> {
        let cents = signal(self.cents);
        html_sync! {
            <span>{pricing::format_cents(cents.get())}</span>
        }.render(cx)
    }
}
"#;
    let diagnostics = lint_file(Path::new("sample.rs"), source);
    assert!(
        !diagnostics.iter().any(|diag| {
            diag.code == "slot/invalid-expression" || diag.code == "rsjs/unnecessary-island"
        }),
        "explicit components should accept statically spelled linked helpers in source lint: {diagnostics:#?}"
    );
}

#[test]
fn lint_applies_render_perf_rules_to_explicit_components() {
    let source = r#"
#[rsjs(component, client)]
impl<B: WriteBuf> WebRenderSync<::proa_core::NoLoader, B> for Counter {
    fn render(self, cx: &mut WebContext<::proa_core::NoLoader, B>) -> Result<(), WriteError> {
        let label = format!("count {}", self.initial);
        let count = signal(self.initial);
        html_sync! {
            <button on_click={|_| count.update(|value| value + 1)}>{label}</button>
        }.render(cx)
    }
}
"#;
    let diagnostics = lint_file(Path::new("sample.rs"), source);
    assert!(
        diagnostics.iter().any(|diag| diag.code == "perf/format"),
        "explicit components must stay under render-path performance lint: {diagnostics:#?}"
    );
}

#[test]
fn lint_reports_self_field_in_linked_rsjs_reactive_helper_call() {
    let source = r#"
#[rsjs(component, client)]
impl<B: WriteBuf> WebRenderSync<::proa_core::NoLoader, B> for CartTotal {
    fn render(self, cx: &mut WebContext<::proa_core::NoLoader, B>) -> Result<(), WriteError> {
        html_sync! {
            <span>{pricing::format_cents(self.cents + 1)}</span>
        }.render(cx)
    }
}
"#;
    let diagnostics = lint_file(Path::new("sample.rs"), source);
    let diag = diagnostics
        .iter()
        .find(|diag| {
            diag.code == "slot/invalid-expression"
                && diag.message.contains("`self` data is server-owned")
        })
        .expect("self-rooted args to linked client helpers should be reported");
    assert_eq!(diag.severity, Severity::Error);
    assert!(diag.help.as_deref().is_some_and(
        |help| help.contains("signal(self.field)") && help.contains("rsjs::server_only")
    ));
}

#[test]
fn lint_defers_handler_helper_identity_to_the_semantic_linker() {
    let source = r#"
#[rsjs(component, client)]
impl<B: WriteBuf> WebRenderSync<::proa_core::NoLoader, B> for CartTotal {
    fn render(self, cx: &mut WebContext<::proa_core::NoLoader, B>) -> Result<(), WriteError> {
        let count = signal(0_i64);
        html_sync! {
            <button on_click={|_| count.set(pricing::subtotal(count.get(), 2))}>"Save"</button>
        }.render(cx)
    }
}
"#;
    let diagnostics = lint_file(Path::new("sample.rs"), source);
    assert!(
        !diagnostics
            .iter()
            .any(|diag| diag.code == "slot/invalid-expression"),
        "source lint must defer handler helper identity to the relocation-backed linker: \
         {diagnostics:#?}"
    );
}

#[test]
fn lint_allows_one_annotation_rsjs_handler_helper_call() {
    let source = r#"
#[rsjs(component, client)]
impl<B: WriteBuf> WebRenderSync<::proa_core::NoLoader, B> for CartTotal {
    fn render(self, cx: &mut WebContext<::proa_core::NoLoader, B>) -> Result<(), WriteError> {
        let count = signal(0_i64);
        html_sync! {
            <button on_click={|_| count.set(pricing::subtotal(count.get(), 2))}>"Save"</button>
        }.render(cx)
    }
}
"#;
    let diagnostics = lint_file(Path::new("sample.rs"), source);
    assert!(
            !diagnostics
                .iter()
                .any(|diag| diag.code == "slot/invalid-expression"),
            "client helper calls inside event handlers should be accepted by source lint: {diagnostics:#?}"
        );
    assert!(
            !diagnostics
                .iter()
                .any(|diag| diag.code == "rsjs/unnecessary-island"),
            "client helper calls inside event handlers should keep the source heuristic hydrated: {diagnostics:#?}"
        );
}

#[test]
fn lint_reports_self_field_in_linked_rsjs_handler_helper_call() {
    let source = r#"
#[rsjs(component, client)]
impl<B: WriteBuf> WebRenderSync<::proa_core::NoLoader, B> for CartTotal {
    fn render(self, cx: &mut WebContext<::proa_core::NoLoader, B>) -> Result<(), WriteError> {
        let count = signal(0_i64);
        html_sync! {
            <button on_click={|_| count.set(pricing::subtotal(self.cents + 1))}>"Save"</button>
        }.render(cx)
    }
}
"#;
    let diagnostics = lint_file(Path::new("sample.rs"), source);
    let diag = diagnostics
        .iter()
        .find(|diag| {
            diag.code == "slot/invalid-expression"
                && diag.message.contains("`self` data is server-owned")
        })
        .expect("self-rooted helper args inside event handlers should be reported");
    assert_eq!(diag.severity, Severity::Error);
    assert!(diag.help.as_deref().is_some_and(
        |help| help.contains("signal(self.field)") && help.contains("rsjs::server_only")
    ));
}

#[test]
fn lint_allows_rsjs_static_template_loops() {
    let source = r#"
#[rsjs(component, client)]
impl<B: WriteBuf> WebRenderSync<::proa_core::NoLoader, B> for Menu {
    fn render(self, cx: &mut WebContext<::proa_core::NoLoader, B>) -> Result<(), WriteError> {
        html_sync! {
            <ul>
                {for item in self.items.iter() {
                    html_sync! { <li>{item.label}</li> }
                }}
            </ul>
        }
        .render(cx)
    }
}
"#;
    let diagnostics = lint_file(Path::new("sample.rs"), source);
    assert!(
        !diagnostics
            .iter()
            .any(|diag| diag.code == "slot/invalid-expression"),
        "static template loops must not be reported as invalid RSJS slots: {diagnostics:#?}"
    );
}

#[test]
fn lint_reports_rsjs_unsupported_reactive_capture() {
    let source = r#"
#[rsjs(component, client)]
impl<B: WriteBuf> WebRenderSync<::proa_core::NoLoader, B> for Counter {
    fn render(self, cx: &mut WebContext<::proa_core::NoLoader, B>) -> Result<(), WriteError> {
        let count = signal(0_i64);
        let on_save = rsjs::handler(|_| count.set(1_i64));
        html_sync! {
            <div>
                {rsjs::server_only(count.get())}
                <span data-save={rsjs::server_attr(on_save)}>"Save"</span>
            </div>
        }.render(cx)
    }
}
"#;
    let diagnostics = lint_file(Path::new("sample.rs"), source);
    let count = diagnostics
        .iter()
        .filter(|diag| diag.code == "rsjs/unsupported-capture")
        .count();
    assert_eq!(count, 2, "expected capture diagnostics: {diagnostics:#?}");
}

#[test]
fn lint_reports_rsjs_unsupported_client_alias_capture() {
    let source = r#"
#[rsjs(component, client)]
impl<B: WriteBuf> WebRenderSync<::proa_core::NoLoader, B> for Counter {
    fn render(self, cx: &mut WebContext<::proa_core::NoLoader, B>) -> Result<(), WriteError> {
        let count = signal(0_i64);
        let display = count.get();
        let current = display;
        html_sync! {
            <div>
                {rsjs::server_only(display)}
                <span data-current={rsjs::server_attr(current)}>"Current"</span>
            </div>
        }.render(cx)
    }
}
"#;
    let diagnostics = lint_file(Path::new("sample.rs"), source);
    let captures: Vec<_> = diagnostics
        .iter()
        .filter(|diag| diag.code == "rsjs/unsupported-capture")
        .collect();
    assert_eq!(
        captures.len(),
        2,
        "expected alias capture diagnostics: {diagnostics:#?}"
    );
    assert!(captures.iter().any(|diag| diag.message.contains("display")));
    assert!(captures.iter().any(|diag| diag.message.contains("current")));
}

#[test]
fn lint_reports_rsjs_unsupported_client_alias_in_prelude_macro() {
    let source = r#"
#[rsjs(component, client)]
impl<B: WriteBuf> WebRenderSync<::proa_core::NoLoader, B> for Counter {
    fn render(self, cx: &mut WebContext<::proa_core::NoLoader, B>) -> Result<(), WriteError> {
        let count = signal(0_i64);
        // proa-perf-ok: lint fixture intentionally uses unsupported allocation syntax
        let display = format!("{}", count.get());
        html_sync! {
            <span>{display}</span>
        }.render(cx)
    }
}
"#;
    let diagnostics = lint_file(Path::new("sample.rs"), source);
    let diag = diagnostics
        .iter()
        .find(|diag| diag.code == "rsjs/unsupported-client-alias")
        .expect("unsupported client-derived alias should be reported");
    assert!(diag.message.contains("display"));
    assert!(diag.message.contains("count"));
}

#[test]
fn lint_reports_rsjs_unsupported_transitive_client_alias_in_prelude_macro() {
    let source = r#"
#[rsjs(component, client)]
impl<B: WriteBuf> WebRenderSync<::proa_core::NoLoader, B> for Counter {
    fn render(self, cx: &mut WebContext<::proa_core::NoLoader, B>) -> Result<(), WriteError> {
        let count = signal(0_i64);
        let current = count.get();
        // proa-perf-ok: lint fixture intentionally uses unsupported allocation syntax
        let display = format!("{}", current);
        html_sync! {
            <span>{display}</span>
        }.render(cx)
    }
}
"#;
    let diagnostics = lint_file(Path::new("sample.rs"), source);
    let diag = diagnostics
        .iter()
        .find(|diag| diag.code == "rsjs/unsupported-client-alias")
        .expect("unsupported transitive client-derived alias should be reported");
    assert!(diag.message.contains("display"));
    assert!(diag.message.contains("current"));
}

#[test]
fn lint_allows_unsupported_client_alias_when_only_used_as_signal_seed() {
    let source = r#"
#[rsjs(component, client)]
impl<B: WriteBuf> WebRenderSync<::proa_core::NoLoader, B> for Counter {
    fn render(self, cx: &mut WebContext<::proa_core::NoLoader, B>) -> Result<(), WriteError> {
        let count = signal(0_i64);
        // proa-perf-ok: lint fixture intentionally uses unsupported allocation syntax
        let display = format!("{}", count.get());
        // proa-perf-ok: rsjs signal owns hydrated client-side string state
        let selected = signal(display.to_string());
        html_sync! {
            <span>{selected.get()}</span>
        }.render(cx)
    }
}
"#;
    let diagnostics = lint_file(Path::new("sample.rs"), source);
    assert!(
        diagnostics
            .iter()
            .all(|diag| diag.code != "rsjs/unsupported-client-alias"),
        "signal seed should stay server-computed until read by Client IR: {diagnostics:#?}"
    );
}

#[test]
fn lint_reports_rsjs_unsupported_client_primitives_in_server_boundary() {
    let source = r#"
#[rsjs(component, client)]
impl<B: WriteBuf> WebRenderSync<::proa_core::NoLoader, B> for Counter {
    fn render(self, cx: &mut WebContext<::proa_core::NoLoader, B>) -> Result<(), WriteError> {
        html_sync! {
            <div>
                {rsjs::server_only(rsjs::memo(|| 1_i64))}
                <span data-ref={rsjs::server_attr(rsjs::node_ref::<rsjs::elem::Input>())}>"Name"</span>
            </div>
        }.render(cx)
    }
}
"#;
    let diagnostics = lint_file(Path::new("sample.rs"), source);
    let captures: Vec<_> = diagnostics
        .iter()
        .filter(|diag| diag.code == "rsjs/unsupported-capture")
        .collect();
    assert_eq!(
        captures.len(),
        2,
        "expected client primitive capture diagnostics: {diagnostics:#?}"
    );
    assert!(captures.iter().any(|diag| diag.message.contains("memo")));
    assert!(captures
        .iter()
        .any(|diag| diag.message.contains("node_ref")));
}

#[test]
fn lint_reports_rsjs_unsupported_browser_helper_in_server_boundary() {
    let source = r#"
#[rsjs(component, client)]
impl<B: WriteBuf> WebRenderSync<::proa_core::NoLoader, B> for BrowserPanel {
    fn render(self, cx: &mut WebContext<::proa_core::NoLoader, B>) -> Result<(), WriteError> {
        html_sync! {
            <div>
                {rsjs::server_only(rsjs::window::inner_width())}
                <span data-path={rsjs::server_attr(rsjs::location::pathname())}>"Path"</span>
                <span data-locale={rsjs::server_attr(rsjs::navigator::language())}>"Locale"</span>
            </div>
        }.render(cx)
    }
}
"#;
    let diagnostics = lint_file(Path::new("sample.rs"), source);
    let captures: Vec<_> = diagnostics
        .iter()
        .filter(|diag| diag.code == "rsjs/unsupported-capture")
        .collect();
    assert_eq!(
        captures.len(),
        3,
        "expected browser helper capture diagnostics: {diagnostics:#?}"
    );
    assert!(captures
        .iter()
        .any(|diag| diag.message.contains("rsjs::window")));
    assert!(captures
        .iter()
        .any(|diag| diag.message.contains("rsjs::location")));
    assert!(captures
        .iter()
        .any(|diag| diag.message.contains("rsjs::navigator")));
}

#[test]
fn lint_reports_rsjs_unsupported_client_work_in_explicit_island_boundary() {
    let source = r#"
#[rsjs(component, client)]
impl<B: WriteBuf> WebRenderSync<::proa_core::NoLoader, B> for BrowserPanel {
    fn render(self, cx: &mut WebContext<::proa_core::NoLoader, B>) -> Result<(), WriteError> {
        html_sync! {
            <div>
                {rsjs::island(BoundaryChild { value: rsjs::memo(|| 1_i64) })}
                {rsjs::island(BoundaryChild { value: rsjs::window::inner_width() })}
            </div>
        }.render(cx)
    }
}
"#;
    let diagnostics = lint_file(Path::new("sample.rs"), source);
    let captures: Vec<_> = diagnostics
        .iter()
        .filter(|diag| diag.code == "rsjs/unsupported-capture")
        .collect();
    assert_eq!(
        captures.len(),
        2,
        "expected explicit island boundary capture diagnostics: {diagnostics:#?}"
    );
    assert!(captures.iter().all(|diag| diag
        .message
        .contains("explicit RSJS island boundary captures parent client-side")));
    assert!(captures.iter().all(|diag| diag
        .help
        .as_deref()
        .is_some_and(|help| help.contains("rsjs::island"))));
    assert!(captures.iter().all(|diag| !diag
        .help
        .as_deref()
        .is_some_and(|help| help.contains("server_only/server_attr"))));
    assert!(captures.iter().any(|diag| diag.message.contains("memo")));
    assert!(captures
        .iter()
        .any(|diag| diag.message.contains("rsjs::window")));
}

#[test]
fn lint_allows_noop_handler_local_in_opaque_server_boundary() {
    let source = r#"
#[rsjs(component)]
impl<B: WriteBuf> WebRenderSync<::proa_core::NoLoader, B> for StaticPanel {
    fn render(self, cx: &mut WebContext<::proa_core::NoLoader, B>) -> Result<(), WriteError> {
        let on_change = rsjs::noop_handler();
        html_sync! {
            <div>
                {rsjs::server_only(on_change)}
            </div>
        }.render(cx)
    }
}
"#;
    let diagnostics = lint_file(Path::new("sample.rs"), source);
    assert!(
        !diagnostics
            .iter()
            .any(|diag| diag.code == "rsjs/unsupported-capture"),
        "noop_handler locals are inert and should not count as reactive captures: {diagnostics:#?}"
    );
    assert!(
        !diagnostics
            .iter()
            .any(|diag| diag.code == "rsjs/unnecessary-island"),
        "an inert participant may still be composed by a live parent: {diagnostics:#?}"
    );
}

#[test]
fn lint_reports_unused_rsjs_handler_local() {
    let source = r#"
#[rsjs(component, client)]
impl<B: WriteBuf> WebRenderSync<::proa_core::NoLoader, B> for Counter {
    fn render(self, cx: &mut WebContext<::proa_core::NoLoader, B>) -> Result<(), WriteError> {
        let on_save = rsjs::event_handler(|_| {});
        html_sync! {
            <button type="button">"Save"</button>
        }.render(cx)
    }
}
"#;
    let diagnostics = lint_file(Path::new("sample.rs"), source);
    let diag = diagnostics
        .iter()
        .find(|diag| diag.code == "rsjs/dead-client-primitive")
        .expect("unused handler local should be reported");
    assert_eq!(diag.severity, Severity::Info);
    assert!(diag.message.contains("`on_save` event_handler"));
}

#[test]
fn lint_reports_unused_rsjs_signal_local() {
    let source = r#"
#[rsjs(component, client)]
impl<B: WriteBuf> WebRenderSync<::proa_core::NoLoader, B> for Counter {
    fn render(self, cx: &mut WebContext<::proa_core::NoLoader, B>) -> Result<(), WriteError> {
        let count = signal(0_i64);
        html_sync! {
            <p>"Static"</p>
        }.render(cx)
    }
}
"#;
    let diagnostics = lint_file(Path::new("sample.rs"), source);
    let diag = diagnostics
        .iter()
        .find(|diag| diag.code == "rsjs/dead-client-primitive")
        .expect("unused signal local should be reported");
    assert_eq!(diag.severity, Severity::Info);
    assert!(diag.message.contains("`count` signal"));
}

#[test]
fn lint_reports_unused_prop_seeded_signal_without_forcing_hydration() {
    let source = r#"
#[rsjs(component, client)]
impl<B: WriteBuf> WebRenderSync<::proa_core::NoLoader, B> for ProductChip {
    fn render(self, cx: &mut WebContext<::proa_core::NoLoader, B>) -> Result<(), WriteError> {
        let draft = signal(self.label);
        html_sync! {
            <span>"Server label only"</span>
        }.render(cx)
    }
}
"#;
    let diagnostics = lint_file(Path::new("sample.rs"), source);
    assert!(
        !diagnostics
            .iter()
            .any(|diag| diag.code == "rsjs/unnecessary-island"),
        "a dead origin is reported directly without claiming the component ABI is unnecessary: {diagnostics:#?}"
    );
    let diag = diagnostics
        .iter()
        .find(|diag| diag.code == "rsjs/dead-client-primitive")
        .expect("unused prop-seeded signal should be reported");
    assert_eq!(diag.severity, Severity::Info);
    assert!(diag.message.contains("`draft` signal"));
    assert!(diag.help.as_deref().is_some_and(|help| help.contains(
        "signal seeds, including prop-derived seeds, are serialized only when reachable"
    )));
}

#[test]
fn lint_reports_signal_used_only_by_server_prelude_as_dead() {
    let source = r#"
#[rsjs(component, client)]
impl<B: WriteBuf> WebRenderSync<::proa_core::NoLoader, B> for Counter {
    fn render(self, cx: &mut WebContext<::proa_core::NoLoader, B>) -> Result<(), WriteError> {
        let count = signal(0_i64);
        let alias = count;
        let _server_value = alias.get();
        html_sync! {
            <p>"Static"</p>
        }.render(cx)
    }
}
"#;
    let diagnostics = lint_file(Path::new("sample.rs"), source);
    assert!(
        !diagnostics
            .iter()
            .any(|diag| diag.code == "rsjs/unnecessary-island"),
        "a dead origin is reported directly without claiming the component ABI is unnecessary: {diagnostics:#?}"
    );
    let diag = diagnostics
        .iter()
        .find(|diag| diag.code == "rsjs/dead-client-primitive")
        .expect("prelude-only signal should still be reported as dead");
    assert_eq!(diag.severity, Severity::Info);
    assert!(diag.message.contains("`count` signal"));
}

#[test]
fn lint_signal_reachability_follows_client_derived_template_local() {
    let source = r#"
#[rsjs(component, client)]
impl<B: WriteBuf> WebRenderSync<::proa_core::NoLoader, B> for Counter {
    fn render(self, cx: &mut WebContext<::proa_core::NoLoader, B>) -> Result<(), WriteError> {
        let count = signal(0_i64);
        let display = count.get();
        html_sync! {
            <span>{display}</span>
        }.render(cx)
    }
}
"#;
    let diagnostics = lint_file(Path::new("sample.rs"), source);
    assert!(
        !diagnostics
            .iter()
            .any(|diag| diag.code == "rsjs/unnecessary-island"),
        "template-used client-derived local should require hydration: {diagnostics:#?}"
    );
    assert!(
        !diagnostics
            .iter()
            .any(|diag| diag.code == "rsjs/dead-client-primitive"),
        "client-derived template local should keep the source signal reachable: {diagnostics:#?}"
    );
}

#[test]
fn lint_signal_reachability_follows_transitive_client_derived_template_local() {
    let source = r#"
#[rsjs(component, client)]
impl<B: WriteBuf> WebRenderSync<::proa_core::NoLoader, B> for Counter {
    fn render(self, cx: &mut WebContext<::proa_core::NoLoader, B>) -> Result<(), WriteError> {
        let count = signal(0_i64);
        let current = count.get();
        let display = current;
        html_sync! {
            <span>{display}</span>
        }.render(cx)
    }
}
"#;
    let diagnostics = lint_file(Path::new("sample.rs"), source);
    assert!(
        !diagnostics
            .iter()
            .any(|diag| diag.code == "rsjs/unnecessary-island"),
        "transitively client-derived template local should require hydration: {diagnostics:#?}"
    );
    assert!(
            !diagnostics
                .iter()
                .any(|diag| diag.code == "rsjs/dead-client-primitive"),
            "transitively client-derived template local should keep the source signal reachable: {diagnostics:#?}"
        );
}

#[test]
fn lint_browser_read_reachability_follows_client_derived_template_local() {
    let source = r#"
#[rsjs(component, client)]
impl<B: WriteBuf> WebRenderSync<::proa_core::NoLoader, B> for LocationBanner {
    fn render(self, cx: &mut WebContext<::proa_core::NoLoader, B>) -> Result<(), WriteError> {
        let path = rsjs::location::pathname();
        html_sync! {
            <span>{path}</span>
        }.render(cx)
    }
}
"#;
    let diagnostics = lint_file(Path::new("sample.rs"), source);
    assert!(
        !diagnostics
            .iter()
            .any(|diag| diag.code == "rsjs/unnecessary-island"),
        "template-used browser-read local should require hydration: {diagnostics:#?}"
    );
}

#[test]
fn lint_allows_rsjs_signal_local_used_from_template() {
    let source = r#"
#[rsjs(component, client)]
impl<B: WriteBuf> WebRenderSync<::proa_core::NoLoader, B> for Counter {
    fn render(self, cx: &mut WebContext<::proa_core::NoLoader, B>) -> Result<(), WriteError> {
        let count = signal(0_i64);
        html_sync! {
            <span>{count.get()}</span>
        }.render(cx)
    }
}
"#;
    let diagnostics = lint_file(Path::new("sample.rs"), source);
    assert!(
        !diagnostics
            .iter()
            .any(|diag| diag.code == "rsjs/dead-client-primitive"),
        "template-used signal local should not be reported: {diagnostics:#?}"
    );
}

#[test]
fn lint_signal_reachability_follows_reachable_handler() {
    let source = r#"
#[rsjs(component, client)]
impl<B: WriteBuf> WebRenderSync<::proa_core::NoLoader, B> for Counter {
    fn render(self, cx: &mut WebContext<::proa_core::NoLoader, B>) -> Result<(), WriteError> {
        let count = signal(0_i64);
        let increment = rsjs::event_handler(|_| count.set(count.get() + 1));
        html_sync! {
            <button type="button" on_click={increment()}>"+"</button>
        }.render(cx)
    }
}
"#;
    let diagnostics = lint_file(Path::new("sample.rs"), source);
    assert!(
        !diagnostics
            .iter()
            .any(|diag| diag.code == "rsjs/dead-client-primitive"),
        "handler-used signal local should not be reported: {diagnostics:#?}"
    );
}

#[test]
fn lint_reports_unused_rsjs_memo_local() {
    let source = r#"
#[rsjs(component, client)]
impl<B: WriteBuf> WebRenderSync<::proa_core::NoLoader, B> for CartTotal {
    fn render(self, cx: &mut WebContext<::proa_core::NoLoader, B>) -> Result<(), WriteError> {
        let price = signal(10_i64);
        let qty = signal(2_i64);
        let total = rsjs::memo(|| price.get() * qty.get());
        html_sync! {
            <p>"Static"</p>
        }.render(cx)
    }
}
"#;
    let diagnostics = lint_file(Path::new("sample.rs"), source);
    let diag = diagnostics
        .iter()
        .find(|diag| {
            diag.code == "rsjs/dead-client-primitive" && diag.message.contains("`total` memo")
        })
        .expect("unused memo local should be reported");
    assert_eq!(diag.severity, Severity::Info);
}

#[test]
fn lint_memo_reachability_keeps_signal_deps_live() {
    let source = r#"
#[rsjs(component, client)]
impl<B: WriteBuf> WebRenderSync<::proa_core::NoLoader, B> for CartTotal {
    fn render(self, cx: &mut WebContext<::proa_core::NoLoader, B>) -> Result<(), WriteError> {
        let price = signal(10_i64);
        let qty = signal(2_i64);
        let total = rsjs::memo(|| price.get() * qty.get());
        html_sync! {
            <span>{total.get()}</span>
        }.render(cx)
    }
}
"#;
    let diagnostics = lint_file(Path::new("sample.rs"), source);
    assert!(
        !diagnostics
            .iter()
            .any(|diag| diag.code == "rsjs/dead-client-primitive"),
        "template-used memo local and its signal deps should not be reported: {diagnostics:#?}"
    );
}

#[test]
fn lint_reports_unused_rsjs_query_local() {
    let source = r#"
#[rsjs(component, client)]
impl<B: WriteBuf> WebRender<::proa_core::NoLoader, B> for PriceTag {
    fn render(
        self,
        cx: &mut WebContext<::proa_core::NoLoader, B>,
    ) -> proa_core::RenderOutcome<
        impl std::future::Future<Output = Result<(), WriteError>>,
    >
    {
        proa_core::RenderOutcome::pending(async move {
            let price = rsjs::query_with_initial(
                rsjs::QueryKey::new(("stock", "AAPL")),
                42_i64,
                rsjs::FetchRequest {
                    url: "/api/stocks/AAPL",
                    ..Default::default()
                },
                rsjs::QueryOptions::default(),
            );
            html! {
                <p>"Static"</p>
            }.render(cx).resolve().await
        })
    }
}
"#;
    let diagnostics = lint_file(Path::new("sample.rs"), source);
    let diag = diagnostics
        .iter()
        .find(|diag| diag.code == "rsjs/dead-client-primitive")
        .expect("unused query local should be reported");
    assert_eq!(diag.severity, Severity::Info);
    assert!(diag.message.contains("`price` query"));
}

#[test]
fn lint_reports_unused_rsjs_query_with_options_local() {
    let source = r#"
#[rsjs(component, client)]
impl<B: WriteBuf> WebRender<::proa_core::NoLoader, B> for PriceTag {
    fn render(
        self,
        cx: &mut WebContext<::proa_core::NoLoader, B>,
    ) -> proa_core::RenderOutcome<
        impl std::future::Future<Output = Result<(), WriteError>>,
    >
    {
        proa_core::RenderOutcome::pending(async move {
            let price = rsjs::query_with_options(
                rsjs::QueryKey::new(("stock", "AAPL")),
                42_i64,
                rsjs::FetchRequest {
                    url: "/api/stocks/AAPL",
                    ..Default::default()
                },
                rsjs::QueryOptions::default(),
            );
            html! {
                <p>"Static"</p>
            }.render(cx).resolve().await
        })
    }
}
"#;
    let diagnostics = lint_file(Path::new("sample.rs"), source);
    let diag = diagnostics
        .iter()
        .find(|diag| diag.code == "rsjs/dead-client-primitive")
        .expect("unused query_with_options local should be reported");
    assert_eq!(diag.severity, Severity::Info);
    assert!(diag.message.contains("`price` query"));
}

#[test]
fn lint_allows_rsjs_query_local_used_from_template() {
    let source = r#"
#[rsjs(component, client)]
impl<B: WriteBuf> WebRender<::proa_core::NoLoader, B> for PriceTag {
    fn render(
        self,
        cx: &mut WebContext<::proa_core::NoLoader, B>,
    ) -> proa_core::RenderOutcome<
        impl std::future::Future<Output = Result<(), WriteError>>,
    >
    {
        proa_core::RenderOutcome::pending(async move {
            let price = rsjs::query_with_initial(
                rsjs::QueryKey::new(("stock", "AAPL")),
                42_i64,
                rsjs::FetchRequest {
                    url: "/api/stocks/AAPL",
                    ..Default::default()
                },
                rsjs::QueryOptions::default(),
            );
            html! {
                <span>{price.data()}</span>
            }.render(cx).resolve().await
        })
    }
}
"#;
    let diagnostics = lint_file(Path::new("sample.rs"), source);
    assert!(
        !diagnostics
            .iter()
            .any(|diag| diag.code == "rsjs/dead-client-primitive"),
        "template-used query local should not be reported: {diagnostics:#?}"
    );
}

#[test]
fn lint_query_reachability_follows_reachable_handler() {
    let source = r#"
#[rsjs(component, client)]
impl<B: WriteBuf> WebRender<::proa_core::NoLoader, B> for PriceTag {
    fn render(
        self,
        cx: &mut WebContext<::proa_core::NoLoader, B>,
    ) -> proa_core::RenderOutcome<
        impl std::future::Future<Output = Result<(), WriteError>>,
    >
    {
        proa_core::RenderOutcome::pending(async move {
            let price = rsjs::query_with_initial(
                rsjs::QueryKey::new(("stock", "AAPL")),
                42_i64,
                rsjs::FetchRequest {
                    url: "/api/stocks/AAPL",
                    ..Default::default()
                },
                rsjs::QueryOptions::default(),
            );
            let refresh = rsjs::event_handler(|_| price.refetch());
            html! {
                <button type="button" on_click={refresh()}>"Refresh"</button>
            }.render(cx).resolve().await
        })
    }
}
"#;
    let diagnostics = lint_file(Path::new("sample.rs"), source);
    assert!(
        !diagnostics
            .iter()
            .any(|diag| diag.code == "rsjs/dead-client-primitive"),
        "handler-used query local should not be reported: {diagnostics:#?}"
    );
}

#[test]
fn lint_reports_unused_rsjs_mutation_local() {
    let source = r#"
#[rsjs(component, client)]
impl<B: WriteBuf> WebRender<::proa_core::NoLoader, B> for CartButton {
    fn render(
        self,
        cx: &mut WebContext<::proa_core::NoLoader, B>,
    ) -> proa_core::RenderOutcome<
        impl std::future::Future<Output = Result<(), WriteError>>,
    >
    {
        proa_core::RenderOutcome::pending(async move {
            let save = rsjs::mutation::<(), ()>(
                rsjs::QueryKey::new(("cart", "AAPL")),
                rsjs::FetchRequest {
                    url: "/api/cart",
                    method: Some(rsjs::HttpMethod::Post),
                    ..Default::default()
                },
                &[rsjs::QueryKey::new(("cart",))],
            );
            html! {
                <button type="button">"Add"</button>
            }.render(cx).resolve().await
        })
    }
}
"#;
    let diagnostics = lint_file(Path::new("sample.rs"), source);
    let diag = diagnostics
        .iter()
        .find(|diag| diag.code == "rsjs/dead-client-primitive")
        .expect("unused mutation local should be reported");
    assert_eq!(diag.severity, Severity::Info);
    assert!(diag.message.contains("`save` mutation"));
}

#[test]
fn lint_reports_unused_rsjs_mutation_with_options_local() {
    let source = r#"
#[rsjs(component, client)]
impl<B: WriteBuf> WebRender<::proa_core::NoLoader, B> for CartButton {
    fn render(
        self,
        cx: &mut WebContext<::proa_core::NoLoader, B>,
    ) -> proa_core::RenderOutcome<
        impl std::future::Future<Output = Result<(), WriteError>>,
    >
    {
        proa_core::RenderOutcome::pending(async move {
            let save = rsjs::mutation_with_options::<(), ()>(
                rsjs::QueryKey::new(("cart", "AAPL")),
                rsjs::FetchRequest {
                    url: "/api/cart",
                    method: Some(rsjs::HttpMethod::Post),
                    ..Default::default()
                },
                &[rsjs::QueryKey::new(("cart",))],
                rsjs::MutationOptions::default(),
            );
            html! {
                <button type="button">"Add"</button>
            }.render(cx).resolve().await
        })
    }
}
"#;
    let diagnostics = lint_file(Path::new("sample.rs"), source);
    let diag = diagnostics
        .iter()
        .find(|diag| diag.code == "rsjs/dead-client-primitive")
        .expect("unused mutation_with_options local should be reported");
    assert_eq!(diag.severity, Severity::Info);
    assert!(diag.message.contains("`save` mutation"));
}

#[test]
fn lint_allows_rsjs_mutation_local_used_from_template() {
    let source = r#"
#[rsjs(component, client)]
impl<B: WriteBuf> WebRender<::proa_core::NoLoader, B> for CartButton {
    fn render(
        self,
        cx: &mut WebContext<::proa_core::NoLoader, B>,
    ) -> proa_core::RenderOutcome<
        impl std::future::Future<Output = Result<(), WriteError>>,
    >
    {
        proa_core::RenderOutcome::pending(async move {
            let save = rsjs::mutation::<(), ()>(
                rsjs::QueryKey::new(("cart", "AAPL")),
                rsjs::FetchRequest {
                    url: "/api/cart",
                    method: Some(rsjs::HttpMethod::Post),
                    ..Default::default()
                },
                &[rsjs::QueryKey::new(("cart",))],
            );
            html! {
                <button type="button" disabled={save.is_pending()}>"Add"</button>
            }.render(cx).resolve().await
        })
    }
}
"#;
    let diagnostics = lint_file(Path::new("sample.rs"), source);
    assert!(
        !diagnostics
            .iter()
            .any(|diag| diag.code == "rsjs/dead-client-primitive"),
        "template-used mutation local should not be reported: {diagnostics:#?}"
    );
}

#[test]
fn lint_mutation_reachability_follows_reachable_handler() {
    let source = r#"
#[rsjs(component, client)]
impl<B: WriteBuf> WebRender<::proa_core::NoLoader, B> for CartButton {
    fn render(
        self,
        cx: &mut WebContext<::proa_core::NoLoader, B>,
    ) -> proa_core::RenderOutcome<
        impl std::future::Future<Output = Result<(), WriteError>>,
    >
    {
        proa_core::RenderOutcome::pending(async move {
            let save = rsjs::mutation::<(), ()>(
                rsjs::QueryKey::new(("cart", "AAPL")),
                rsjs::FetchRequest {
                    url: "/api/cart",
                    method: Some(rsjs::HttpMethod::Post),
                    ..Default::default()
                },
                &[rsjs::QueryKey::new(("cart",))],
            );
            let add = rsjs::event_handler(|_| save.run(()));
            html! {
                <button type="button" on_click={add()}>"Add"</button>
            }.render(cx).resolve().await
        })
    }
}
"#;
    let diagnostics = lint_file(Path::new("sample.rs"), source);
    assert!(
        !diagnostics
            .iter()
            .any(|diag| diag.code == "rsjs/dead-client-primitive"),
        "handler-used mutation local should not be reported: {diagnostics:#?}"
    );
}

#[test]
fn lint_allows_rsjs_handler_local_used_from_template() {
    let source = r#"
#[rsjs(component, client)]
impl<B: WriteBuf> WebRenderSync<::proa_core::NoLoader, B> for Counter {
    fn render(self, cx: &mut WebContext<::proa_core::NoLoader, B>) -> Result<(), WriteError> {
        let count = signal(0_i64);
        let on_save = rsjs::event_handler(|_| count.set(1_i64));
        html_sync! {
            <button type="button" on_click={on_save()}>"Save"</button>
        }.render(cx)
    }
}
"#;
    let diagnostics = lint_file(Path::new("sample.rs"), source);
    assert!(
        !diagnostics
            .iter()
            .any(|diag| diag.code == "rsjs/dead-client-primitive"),
        "used handler local should not be reported: {diagnostics:#?}"
    );
}

#[test]
fn lint_handler_local_reachability_follows_handler_calls() {
    let source = r#"
#[rsjs(component, client)]
impl<B: WriteBuf> WebRenderSync<::proa_core::NoLoader, B> for Counter {
    fn render(self, cx: &mut WebContext<::proa_core::NoLoader, B>) -> Result<(), WriteError> {
        let count = signal(0_i64);
        let inner = rsjs::handler(|_| count.set(1_i64));
        let outer = rsjs::event_handler(|_| inner());
        html_sync! {
            <button type="button" on_click={outer()}>"Save"</button>
        }.render(cx)
    }
}
"#;
    let diagnostics = lint_file(Path::new("sample.rs"), source);
    assert!(
        !diagnostics
            .iter()
            .any(|diag| diag.code == "rsjs/dead-client-primitive"),
        "transitively used handler locals should not be reported: {diagnostics:#?}"
    );
}

#[test]
fn lint_reports_unused_rsjs_node_ref_local() {
    let source = r#"
#[rsjs(component, client)]
impl<B: WriteBuf> WebRenderSync<::proa_core::NoLoader, B> for SearchBox {
    fn render(self, cx: &mut WebContext<::proa_core::NoLoader, B>) -> Result<(), WriteError> {
        let input_ref = rsjs::node_ref::<rsjs::elem::Input>();
        html_sync! {
            <input type="search" />
        }.render(cx)
    }
}
"#;
    let diagnostics = lint_file(Path::new("sample.rs"), source);
    let diag = diagnostics
        .iter()
        .find(|diag| diag.code == "rsjs/dead-client-primitive")
        .expect("unused node_ref local should be reported");
    assert_eq!(diag.severity, Severity::Info);
    assert!(diag.message.contains("`input_ref` node_ref"));
}

#[test]
fn lint_allows_rsjs_node_ref_local_used_from_template() {
    let source = r#"
#[rsjs(component, client)]
impl<B: WriteBuf> WebRenderSync<::proa_core::NoLoader, B> for SearchBox {
    fn render(self, cx: &mut WebContext<::proa_core::NoLoader, B>) -> Result<(), WriteError> {
        let input_ref = rsjs::node_ref::<rsjs::elem::Input>();
        html_sync! {
            <input type="search" ref={input_ref} />
        }.render(cx)
    }
}
"#;
    let diagnostics = lint_file(Path::new("sample.rs"), source);
    assert!(
        !diagnostics
            .iter()
            .any(|diag| diag.code == "rsjs/dead-client-primitive"),
        "template-bound node_ref local should not be reported: {diagnostics:#?}"
    );
}

#[test]
fn lint_node_ref_reachability_follows_reachable_handler() {
    let source = r#"
#[rsjs(component, client)]
impl<B: WriteBuf> WebRenderSync<::proa_core::NoLoader, B> for SearchBox {
    fn render(self, cx: &mut WebContext<::proa_core::NoLoader, B>) -> Result<(), WriteError> {
        let input_ref = rsjs::node_ref::<rsjs::elem::Input>();
        let focus = rsjs::event_handler(|_| input_ref.focus());
        html_sync! {
            <button type="button" on_click={focus()}>"Focus"</button>
        }.render(cx)
    }
}
"#;
    let diagnostics = lint_file(Path::new("sample.rs"), source);
    assert!(
        !diagnostics
            .iter()
            .any(|diag| diag.code == "rsjs/dead-client-primitive"),
        "handler-used node_ref local should not be reported: {diagnostics:#?}"
    );
}

#[test]
fn lint_does_not_scan_unmarked_client_like_functions() {
    let source = r#"
pub fn mount(selector: String) {
    document::query_selector(selector);
}
"#;
    let diagnostics = lint_file(Path::new("sample.rs"), source);
    assert!(diagnostics.is_empty());
}

#[test]
fn lint_reports_indentation_warning() {
    let source = r#"
fn render() {
    html_sync! {
        <div>
        <span>"bad"</span>
        </div>
    };
}
"#;
    let diagnostics = lint_file(Path::new("sample.rs"), source);
    assert!(diagnostics
        .iter()
        .any(|diag| diag.code == "format/indentation"));
}

#[test]
fn lint_reports_braced_string_literal() {
    let source = r#"
fn render() {
    html_sync! {
        <span>{"literal"}</span>
    };
}
"#;
    let diagnostics = lint_file(Path::new("sample.rs"), source);
    let diag = diagnostics
        .iter()
        .find(|diag| diag.code == "perf/braced-string-literal")
        .expect("string literal in a slot should be reported");
    assert!(diag.message.contains("runtime escaped"));
}

#[test]
fn lint_reports_collect_in_render_path() {
    let source = r#"
fn render() {
    html_sync! {
        <ul>{self.names.iter().collect::<Vec<_>>()}</ul>
    };
}
"#;
    let diagnostics = lint_file(Path::new("sample.rs"), source);
    assert!(diagnostics.iter().any(|diag| diag.code == "perf/collect"));
}

#[test]
fn lint_reports_join_in_render_path() {
    let source = r#"
fn render() {
    html_sync! {
        <div>{self.parts.join(", ")}</div>
    };
}
"#;
    let diagnostics = lint_file(Path::new("sample.rs"), source);
    assert!(diagnostics.iter().any(|diag| diag.code == "perf/join"));
}

#[test]
fn lint_reports_manual_escape_call_in_render_path() {
    let source = r#"
fn render() {
    html_sync! {
        <div>{html_escape::encode_text(&self.label)}</div>
    };
}
"#;
    let diagnostics = lint_file(Path::new("sample.rs"), source);
    assert!(diagnostics
        .iter()
        .any(|diag| diag.code == "perf/manual-escape"));
}

#[test]
fn lint_reports_string_add_in_render_path() {
    let source = r#"
fn render() {
    html_sync! {
        <div>{self.name + "!"}</div>
    };
}
"#;
    let diagnostics = lint_file(Path::new("sample.rs"), source);
    assert!(diagnostics
        .iter()
        .any(|diag| diag.code == "perf/string-add"));
}

#[test]
fn lint_reports_string_push_in_render_path() {
    let source = r#"
impl<B: WriteBuf> WebRenderSync<::proa_core::NoLoader, B> for Tags {
    fn render(self, cx: &mut WebContext<::proa_core::NoLoader, B>) -> Result<(), WriteError> {
        let mut out = String::new();
        out.push('<');
        html_sync! { <div>{out}</div> }.render(cx)
    }
}
"#;
    let diagnostics = lint_file(Path::new("sample.rs"), source);
    assert!(diagnostics
        .iter()
        .any(|diag| diag.code == "perf/string-push"));
}

#[test]
fn lint_reports_to_owned_in_render_path() {
    let source = r#"
fn render() {
    html_sync! {
        <div>{self.name.to_owned()}</div>
    };
}
"#;
    let diagnostics = lint_file(Path::new("sample.rs"), source);
    assert!(diagnostics.iter().any(|diag| diag.code == "perf/to-owned"));
}

#[test]
fn lint_reports_vec_new_in_render_path() {
    let source = r#"
fn render() {
    html_sync! {
        <div>{Vec::new()}</div>
    };
}
"#;
    let diagnostics = lint_file(Path::new("sample.rs"), source);
    assert!(diagnostics.iter().any(|diag| diag.code == "perf/vec-new"));
}

#[test]
fn lint_reports_vec_macro_in_render_path() {
    let source = r#"
fn render() {
    html_sync! {
        <div>{vec![1, 2, 3]}</div>
    };
}
"#;
    let diagnostics = lint_file(Path::new("sample.rs"), source);
    assert!(diagnostics.iter().any(|diag| diag.code == "perf/vec"));
}

#[test]
fn lint_strategy_cache_require_fresh_reports_ambiguous_sidecars() {
    let source = r#"
#[rsjs(component, client)]
impl<B: WriteBuf> WebRenderSync<::proa_core::NoLoader, B> for AmbiguousPanel {
    fn render(self, cx: &mut WebContext<::proa_core::NoLoader, B>) -> Result<(), WriteError> {
        html_sync! {
            <section>
                <p>"Ready"</p>
            </section>
        }.render(cx)
    }
}
"#;
    let tmp = tempfile::tempdir().unwrap();
    write_strategy_sidecar_with_fingerprint(
        tmp.path(),
        "ambiguous_panel_a",
        "AmbiguousPanel",
        "AmbiguousPanel",
        "server_only",
        None,
        None,
    );
    write_strategy_sidecar_with_fingerprint(
        tmp.path(),
        "ambiguous_panel_b",
        "AmbiguousPanel",
        "AmbiguousPanel",
        "server_only",
        None,
        None,
    );

    let diagnostics = lint_file_with_options(
        Path::new("sample.rs"),
        source,
        LintOptions {
            rsjs_strategy_cache: Some(RsjsStrategyCacheOptions {
                root: tmp.path().to_path_buf(),
                require_fresh: true,
            }),
            ..Default::default()
        },
    );
    let diag = diagnostics
        .iter()
        .find(|diag| diag.code == "rsjs/strategy-cache-ambiguous")
        .expect("strict strategy cache mode should report ambiguous sidecar matches");
    assert_eq!(diag.severity, Severity::Error);
    assert!(diag
        .help
        .as_deref()
        .is_some_and(|help| help.contains("found 2 matching sidecars")));
    assert!(
            !diagnostics
                .iter()
                .any(|diag| diag.code == "rsjs/unnecessary-island"),
            "strict ambiguous-cache error should own this impl instead of falling back: {diagnostics:#?}"
        );
}

#[test]
fn shared_context_does_not_imply_html_escaping() {
    let source = r###"
fn render_md_example<L: DataLoader>(cx: &mut WebContext<L>) -> Result<(), WriteError> {
    let caption = format!("{}", 42);
    cx.write_md_inline_code("<section>example</section>")?;
    cx.out.extend_static(b"<section>example</section>")?;
    escape_html(&caption);
    Ok(())
}
fn render_shared<L: DataLoader>(cx: &mut WebContext<L>) -> Result<(), WriteError> {
    let text = format!("{}", 1);
    cx.out.extend(text.as_bytes())
}
fn render_html_example<L: DataLoader>(cx: &mut WebContext<L>) -> Result<(), WriteError> {
    cx.out.extend_static(b"<section>html</section>")
}
"###;
    let diagnostics = lint_file(Path::new("sample.rs"), source);
    assert_eq!(
        diagnostics
            .iter()
            .filter(|d| d.code == "perf/raw-write-html")
            .count(),
        1
    );
    assert_eq!(
        diagnostics
            .iter()
            .filter(|d| d.code == "perf/format")
            .count(),
        2
    );
    assert!(!diagnostics
        .iter()
        .any(|d| matches!(d.code, "perf/manual-escape" | "perf/manual-escape-helper")));
}

#[test]
fn standalone_markdown_evidence_disables_only_html_checks() {
    for (name, operation) in [
        ("render_md_example", ""),
        ("render_markdown_example", ""),
        ("render_fragment", r#"md_sync! { "sample" };"#),
        ("render_fragment", r#"let _ = md_sync! { "sample" };"#),
        (
            "render_fragment",
            r#"let _ = proa_macros::md! { "sample" };"#,
        ),
        ("render_fragment", "cx.write_md_text(\"sample\")?;"),
        ("render_fragment", "cx.md_block_boundary()?;"),
        ("render_fragment", "child.render_md(cx)?;"),
        (
            "render_fragment",
            "WebContext::write_md_text(cx, \"sample\")?;",
        ),
        ("render_fragment", "MdRenderSync::render_md(child, cx)?;"),
    ] {
        // proa-perf-ok: lint fixtures assemble Rust source containing intentional raw HTML
        let source = format!(
            r#"
fn {name}<L: DataLoader>(cx: &mut WebContext<L>) -> Result<(), WriteError> {{
    {operation}
    let caption = format!("{{}}", 42);
    cx.out.extend_static(b"<section>code sample</section>")?;
    escape_html(&caption);
    Ok(())
}}
"#
        );
        let diagnostics = lint_file(Path::new("sample.rs"), &source);
        assert!(
            diagnostics.iter().any(|d| d.code == "perf/format"),
            "{name}: {operation}: {diagnostics:#?}"
        );
        assert!(
            !diagnostics.iter().any(|d| matches!(
                d.code,
                "perf/raw-write-html" | "perf/manual-escape" | "perf/manual-escape-helper"
            )),
            "{name}: {operation}: {diagnostics:#?}"
        );
    }
}

#[test]
fn markdown_trait_lints_follow_shared_context_and_renamed_helpers() {
    let source = r###"
impl<L: DataLoader> MdRenderSync<L> for Markdown {
    fn render_md(self, cx: &mut WebContext<L>) -> Result<(), WriteError> {
        cx.write_md_prepared_static(b"# Handwritten\n")?;
        cx.write_md_inline_code("<section>literal</section>")
    }
}

"###;
    let diagnostics = lint_file(Path::new("sample.rs"), source);
    assert!(diagnostics.iter().any(|d| d.code == "md/manual-write"));
    assert!(!diagnostics.iter().any(|d| d.code == "perf/raw-write-html"));
}

#[test]
fn reactive_text_arguments_keep_named_client_locals_reachable() {
    let source = r#"
#[rsjs(component, client)]
impl<L: DataLoader> WebRenderSync<L> for Counter {
    fn render(self, cx: &mut WebContext<L>) -> Result<(), WriteError> {
        let count = signal(1_u32);
        let subtotal = computed(|| count.get() * 105_u32);
        let label = proa_macros::text!("Subtotal {}", subtotal.get());
        html_sync! {
            <div title={::proa_macros::text!("Count {}", count.get())}>
                {label}
                <p>{text!("Count {}", count.get())}</p>
            </div>
        }.render(cx)
    }
}
"#;
    let diagnostics = lint_file(Path::new("sample.rs"), source);
    assert!(
        !diagnostics
            .iter()
            .any(|diag| diag.code == "rsjs/dead-client-primitive"),
        "{diagnostics:#?}"
    );
}

#[test]
fn markdown_syntax_errors_are_reported_without_a_cargo_build() {
    for body in [
        "{if false {A} elsse {B}}",
        "{for x in xs {X} ignored}",
        "{if true {unclosed}",
    ] {
        let source = format!(r####"fn f() {{ md_sync! {{ r###"{body}"### }}; }}"####);
        let diagnostics = lint_file(Path::new("sample.rs"), &source);
        assert!(
            diagnostics.iter().any(|d| d.code == "parse/template"),
            "{body}: {diagnostics:?}"
        );
    }
}

#[test]
fn markdown_named_arguments_participate_in_layout_lint() {
    let source = r####"fn f() {
    md_sync! { r###"
            {if show {
                title
            }}
            "###, show = true, title = self.title
    };
}"####;
    let diagnostics = lint_file(Path::new("sample.rs"), source);
    assert!(
        diagnostics
            .iter()
            .any(|d| d.code == "format/markdown-layout"),
        "{diagnostics:?}"
    );
    assert!(
        !diagnostics.iter().any(|d| d.code == "parse/template"),
        "{diagnostics:?}"
    );
}

#[test]
fn lint_flags_misleading_public_url_calls_in_local_bindings() {
    let source = r#"
impl<L: DataLoader> WebRenderSync<L> for Hero {
    fn render(self, cx: &mut WebContext<L>) -> Result<(), WriteError> {
        let logo = cx.url("/public/logo.svg");
        let banner = cx.url("./public/banner.png");
        let generated = cx.url("/_proa/immutable/logo.1234abcd5e6f7a8b.svg");
        let valid = cx.url("public/ok.svg");
        html_sync! {
            <header>
                <img src={logo} />
                <img src={banner} />
                <img src={generated} />
                <img src={valid} />
            </header>
        }
        .render(cx)
    }
}
"#;
    let diagnostics = lint_file(Path::new("hero.rs"), source);
    let logical_count = diagnostics
        .iter()
        .filter(|d| d.code == "asset/logical-path")
        .count();
    let generated_count = diagnostics
        .iter()
        .filter(|d| d.code == "asset/generated-url")
        .count();
    assert_eq!(logical_count, 2, "{diagnostics:#?}");
    assert_eq!(generated_count, 1, "{diagnostics:#?}");
}

#[test]
fn asset_url_lint_infers_arbitrary_struct_fields_constructors_and_data_slices() {
    let source = r#"
pub struct Avatar {
    pub photo: &'static str,
}
impl WebRenderSync for Avatar {
    fn render(self, cx: &mut WebContext) -> Result<(), WriteError> {
        html_sync! { <img src={self.photo} alt="Avatar" /> }.render(cx)
    }
}

pub struct TeamCard {
    pub lead_photo: &'static str,
}
impl TeamCard {
    pub const fn with_photo(photo: &'static str) -> Self {
        Self { lead_photo: photo }
    }
}
impl WebRenderSync for TeamCard {
    fn render(self, cx: &mut WebContext) -> Result<(), WriteError> {
        html_sync! { <div>{Avatar { photo: self.lead_photo }}</div> }.render(cx)
    }
}

pub struct SafeImage {
    pub src: &'static str,
}
impl WebRenderSync for SafeImage {
    fn render(self, cx: &mut WebContext) -> Result<(), WriteError> {
        html_sync! { <img src={cx.url(self.src)} alt="Safe" /> }.render(cx)
    }
}

pub struct Project {
    pub thumb: &'static str,
}
const PROJECTS: &[Project] = &[Project {
    thumb: "public/images/brand.svg",
}];

const RAW_LOGO: &str = "public/images/raw_logo.svg";

pub struct ProjectsPage;
impl WebRenderSync for ProjectsPage {
    fn render(self, cx: &mut WebContext) -> Result<(), WriteError> {
        let local_icon = "public/images/local_icon.svg";
        for project in PROJECTS {
            html_sync! { <img src={project.thumb} alt="Thumb" /> }.render(cx)?;
        }
        html_sync! {
            <main>
                <img src={RAW_LOGO} alt="Const" />
                <img src={local_icon} alt="Local" />
                <img src={cx.url(RAW_LOGO)} alt="Resolved Const" />
                {SafeImage { src: "public/images/resolved_inside.svg" }}
            </main>
        }
        .render(cx)
    }
}

pub fn handler() -> (TeamCard, TeamCard, SafeImage) {
    (
        TeamCard {
            lead_photo: "public/images/alice.png",
        },
        TeamCard::with_photo("public/images/bob.png"),
        SafeImage {
            src: "public/images/safe_in_handler.svg",
        },
    )
}
"#;
    let hits: Vec<_> = lint_file(Path::new("src/team.rs"), source)
        .into_iter()
        .filter(|d| d.code.starts_with("asset/"))
        .map(|d| (d.line, d.code))
        .collect();
    assert_eq!(
        hits,
        vec![
            (38, "asset/public-url"), // const PROJECTS Project { thumb: "public/images/brand.svg" }
            (52, "asset/public-url"), // <img src={RAW_LOGO} />
            (53, "asset/public-url"), // <img src={local_icon} />
            (65, "asset/public-url"), // TeamCard { lead_photo: "public/images/alice.png" } in handler
            (67, "asset/public-url"), // TeamCard::with_photo("public/images/bob.png") in handler
        ],
        "unexpected hits: {hits:#?}"
    );
}

#[test]
fn asset_url_lint_checks_markdown_templates_and_render_head() {
    let source = r####"
pub struct ArticlePage;
impl WebRenderSync for ArticlePage {
    const ROLE: ComponentRole = ComponentRole::PAGE;
    fn render_head(&self, cx: &mut WebContext) -> Result<(), WriteError> {
        let _media = HeadMedia::new("public/images/og.png");
        let _icon = HeadIcon::new("public/favicon.ico");
        html_sync! {
            <link rel="stylesheet" href="public/styles.css" />
            <meta property="og:image" content="public/images/share.png" />
            <meta name="description" content="public/not-a-url" />
        }
        .render(cx)
    }
    fn render(self, cx: &mut WebContext) -> Result<(), WriteError> {
        html_sync! { <main>"ok"</main> }.render(cx)
    }
}

impl MdRenderSync for ArticlePage {
    fn render_md(self, cx: &mut WebContext) -> Result<(), WriteError> {
        let raw = "public/images/slot.svg";
        md_sync! { r###"
            ![Static](public/images/static.svg)

            ![Slot]({raw})

            ![Safe]({cx.url("public/images/ok.svg")})

            ![BadPrefix]({cx.url("/public/images/bad.svg")})

            `![InlineCode](public/images/code.svg)`

            ```md
            ![FencedCode](public/images/fence.svg)
            ```
            "###
        }
        .render_md(cx)
    }
}
"####;
    let mut hits: Vec<_> = lint_file(Path::new("src/article.rs"), source)
        .into_iter()
        .filter(|d| d.code.starts_with("asset/"))
        .map(|d| (d.line, d.code))
        .collect();
    hits.sort_unstable();
    assert_eq!(
        hits,
        vec![
            (6, "asset/public-url"),    // HeadMedia::new("public/images/og.png")
            (7, "asset/public-url"),    // HeadIcon::new("public/favicon.ico")
            (9, "asset/public-url"),    // <link href="public/styles.css" />
            (10, "asset/public-url"), // <meta property="og:image" content="public/images/share.png" />
            (24, "asset/public-url"), // ![Static](public/images/static.svg)
            (26, "asset/public-url"), // ![Slot]({raw})
            (30, "asset/logical-path"), // cx.url("/public/images/bad.svg") in md_sync!
        ],
        "unexpected hits: {hits:#?}"
    );
}

#[test]
fn asset_url_lint_works_cross_file_and_detects_public_catalog_files() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    std::fs::create_dir_all(root.join("src/components")).unwrap();
    std::fs::create_dir_all(root.join("src/pages")).unwrap();
    std::fs::create_dir_all(root.join("public/images")).unwrap();
    std::fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();
    std::fs::write(
        root.join("proa.config.rs"),
        r#"use proa::config::{Config, ProjectPaths, Result};
pub async fn config() -> Result<Config> {
    Ok(Config {
        paths: ProjectPaths { public: Some("static"), ..Default::default() },
        ..Config::default()
    })
}
"#,
    )
    .unwrap();
    std::fs::create_dir_all(root.join("static/images")).unwrap();
    std::fs::write(root.join("static/images/brand.svg"), "<svg/>").unwrap();
    std::fs::write(
        root.join("src/components/card.rs"),
        r#"pub struct BrandCard {
    pub artwork: &'static str,
}
impl WebRenderSync for BrandCard {
    fn render(self, cx: &mut WebContext) -> Result<(), WriteError> {
        html_sync! { <img src={self.artwork} alt="Brand" /> }.render(cx)
    }
}
"#,
    )
    .unwrap();
    std::fs::write(
        root.join("src/pages/home.rs"),
        r#"use crate::components::card::BrandCard;

pub fn make_card() -> BrandCard {
    BrandCard {
        artwork: "/images/brand.svg",
    }
}

pub struct HomePage;
impl WebRenderSync for HomePage {
    fn render(self, cx: &mut WebContext) -> Result<(), WriteError> {
        html_sync! {
            <main>
                <img src="/images/brand.svg" alt="Root Relative" />
                <a href="/projects">"Route"</a>
                <img src={cx.url("public/images/brand.svg")} alt="Resolved" />
            </main>
        }
        .render(cx)
    }
}
"#,
    )
    .unwrap();

    let diagnostics = lint_paths(
        &[root.join("src/pages/home.rs"), root.join("proa.config.rs")],
        LintOptions::default(),
    )
    .unwrap();
    let asset_diags: Vec<_> = diagnostics
        .into_iter()
        .filter(|d| d.code.starts_with("asset/"))
        .collect();
    let mut hits: Vec<_> = asset_diags
        .iter()
        .map(|d| {
            (
                d.path.file_name().unwrap().to_str().unwrap().to_string(),
                d.line,
                d.code,
            )
        })
        .collect();
    hits.sort();
    assert_eq!(
        hits,
        vec![
            ("home.rs".to_string(), 5, "asset/public-url"), // cross-file BrandCard { artwork: "/images/brand.svg" }
            ("home.rs".to_string(), 14, "asset/public-url"), // <img src="/images/brand.svg" />
        ],
        "unexpected asset diagnostics: {asset_diags:#?}"
    );
    assert!(asset_diags.iter().all(|d| d
        .help
        .as_deref()
        .unwrap_or_default()
        .contains("cx.url(\"public/images/brand.svg\")")));

    // Disabling `paths.public = None` disables root-relative `public/` file matching.
    std::fs::write(
        root.join("proa.config.rs"),
        r#"use proa::config::{Config, ProjectPaths, Result};
pub async fn config() -> Result<Config> {
    Ok(Config {
        paths: ProjectPaths { public: None, ..Default::default() },
        ..Config::default()
    })
}
"#,
    )
    .unwrap();
    let disabled_hits: Vec<_> =
        lint_paths(&[root.join("src/pages/home.rs")], LintOptions::default())
            .unwrap()
            .into_iter()
            .filter(|d| d.code.starts_with("asset/"))
            .collect();
    assert!(disabled_hits.is_empty(), "{disabled_hits:#?}");
}

#[test]
fn asset_url_lint_respects_block_scope_foreign_macros_named_md_args_and_mixed_sinks() {
    let source = r####"
pub struct MixedView {
    pub src: &'static str,
}
impl WebRenderSync for MixedView {
    fn render(self, cx: &mut WebContext) -> Result<(), WriteError> {
        html_sync! { <img src={cx.url(self.src)} alt="Safe HTML" /> }.render(cx)
    }
}
impl MdRenderSync for MixedView {
    fn render_md(self, cx: &mut WebContext) -> Result<(), WriteError> {
        md_sync! { "![Broken MD]({image})", image = self.src }.render_md(cx)
    }
}

pub struct ScopedPage;
impl WebRenderSync for ScopedPage {
    fn render(self, cx: &mut WebContext) -> Result<(), WriteError> {
        let image = "/route";
        {
            let image = "public/inner.png";
            let _ = image;
        }
        let raw = "public/named_md.svg";
        let _md = md_sync! { "![Named]({pic})", pic = raw };
        let _foreign = yew::html! { <img src="public/yew.svg" /> };
        html_sync! {
            <main>
                <img src={image} alt="Outer Route" />
                {MixedView { src: "public/mixed.svg" }}
            </main>
        }
        .render(cx)
    }
}
"####;
    let mut hits: Vec<_> = lint_file(Path::new("src/scoped.rs"), source)
        .into_iter()
        .filter(|d| d.code.starts_with("asset/"))
        .map(|d| (d.line, d.code))
        .collect();
    hits.sort_unstable();
    assert_eq!(
        hits,
        vec![
            (25, "asset/public-url"), // named md arg `pic = raw` where `raw = "public/named_md.svg"`
            (30, "asset/public-url"), // MixedView { src: "public/mixed.svg" } (unsafe MD sink wins over safe HTML sink)
        ],
        "unexpected hits: {hits:#?}"
    );
}

fn asset_hits(path: &str, source: &str) -> Vec<(usize, &'static str)> {
    lint_file(Path::new(path), source)
        .into_iter()
        .filter(|d| d.code.starts_with("asset/"))
        .map(|d| (d.line, d.code))
        .collect()
}

#[test]
fn asset_url_lint_checks_templates_nested_in_template_slots() {
    let source = r#"
pub struct Gallery {
    pub cover: &'static str,
}
impl WebRenderSync for Gallery {
    fn render(self, cx: &mut WebContext) -> Result<(), WriteError> {
        html_sync! {
            <div>
                {html_sync! { <img src="public/a.png" alt="A" /> }}
                {if true { html_sync! { <img src={self.cover} alt="Cover" /> } } else { html_sync! {} }}
                {yew::html! { <img src="public/foreign.png" /> }}
            </div>
        }
        .render(cx)
    }
}

pub fn handler() -> Gallery {
    Gallery {
        cover: "public/cover.png",
    }
}
"#;
    assert_eq!(
        asset_hits("src/gallery.rs", source),
        vec![
            (9, "asset/public-url"),  // nested <img src="public/a.png" />
            (20, "asset/public-url"), // `cover` reaches the nested <img src={self.cover} />
        ]
    );
}

#[test]
fn asset_url_lint_propagates_builtin_head_constructors_through_wrappers() {
    let source = r#"
fn media(src: &'static str) -> HeadMedia {
    HeadMedia::new(src)
}

pub fn handler() -> HeadMedia {
    media("public/share.png")
}
"#;
    assert_eq!(
        asset_hits("src/head.rs", source),
        vec![(7, "asset/public-url")]
    );
}

#[test]
fn asset_url_lint_resolves_markdown_arguments_in_the_callers_scope() {
    let source = r#"
pub struct Album {
    pub artwork: &'static str,
    pub caption: &'static str,
}
impl MdRenderSync for Album {
    fn render_md(self, cx: &mut WebContext) -> Result<(), WriteError> {
        let a = self.artwork;
        md_sync! { "{a} ![Image]({b})", a = self.caption, b = a }.render_md(cx)
    }
}

pub fn handler() -> Album {
    Album {
        artwork: "public/art.png",
        caption: "public/caption-text",
    }
}
"#;
    // `b = a` binds the outer `a` (`self.artwork`), not the `a` argument.
    assert_eq!(
        asset_hits("src/album.rs", source),
        vec![(15, "asset/public-url")]
    );
}

#[test]
fn asset_url_lint_ignores_names_defined_more_than_once_in_a_package() {
    let source = r#"
mod web {
    pub struct Image {
        pub src: &'static str,
    }
    impl WebRenderSync for Image {
        fn render(self, cx: &mut WebContext) -> Result<(), WriteError> {
            html_sync! { <img src={self.src} alt="" /> }.render(cx)
        }
    }
}

mod files {
    pub struct Image {
        pub src: &'static str,
    }
    pub fn copy() -> Image {
        Image {
            src: "public/input.png",
        }
    }
}

mod routes {
    pub const LOGO: &str = "/route";
    pub struct Nav;
    impl WebRenderSync for Nav {
        fn render(self, cx: &mut WebContext) -> Result<(), WriteError> {
            html_sync! { <a href={LOGO}>"Home"</a> }.render(cx)
        }
    }
}

mod assets {
    pub const LOGO: &str = "public/logo.svg";
}
"#;
    assert_eq!(asset_hits("src/ambiguous.rs", source), vec![]);
}

#[test]
fn asset_url_lint_points_markdown_findings_at_the_authored_text() {
    let source = r####"
pub struct Notes;
impl MdRenderSync for Notes {
    fn render_md(self, cx: &mut WebContext) -> Result<(), WriteError> {
        let raw = "public/raw.svg";
        md_sync! { r###"
            Intro text.

            ![Static](public/static.svg) and ![Slot]({raw})

            {if true { md_sync! { "![Nested](public/nested.svg)" } } else { md_sync! { "" } }}
            "###
        }
        .render_md(cx)
    }
}
"####;
    let mut hits: Vec<_> = lint_file(Path::new("src/notes.rs"), source)
        .into_iter()
        .filter(|d| d.code.starts_with("asset/"))
        .map(|d| (d.line, d.column))
        .collect();
    hits.sort_unstable();
    let line_9 = source.lines().nth(8).unwrap();
    let line_11 = source.lines().nth(10).unwrap();
    assert_eq!(
        hits,
        vec![
            (9, line_9.find("public/static.svg").unwrap() + 1),
            (9, line_9.find("raw}").unwrap() + 1),
            (11, line_11.find("public/nested.svg").unwrap() + 1),
        ]
    );
}

#[test]
fn files_declared_by_cfg_test_modules_are_test_paths() {
    let temp = tempfile::tempdir().unwrap();
    let src = temp.path().join("src");
    let write = |relative: &str, contents: &str| {
        let path = src.join(relative);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, contents).unwrap();
        path
    };
    write(
        "lib.rs",
        "mod assets;\nmod prod;\n#[cfg(test)] pub(crate) mod fixtures;\n\n#[cfg(test)]\nmod tests;\n",
    );
    let prod = write("prod.rs", "pub fn shipped() {}\n");
    let tests = write("tests.rs", "#[test]\nfn works() {}\n");
    let fixtures = write("fixtures.rs", "mod data;\n");
    let nested = write("fixtures/data.rs", "pub const DATA: &str = \"\";\n");
    write(
        "assets.rs",
        "#[cfg(test)]\n#[allow(dead_code)]\nmod tests;\n// `mod tests_support;` stays production code.\nmod tests_support;\n",
    );
    let assets_tests = write("assets/tests.rs", "");
    let assets_support = write("assets/tests_support.rs", "");
    let module_dir = write("views/mod.rs", "#[cfg(test)]\nmod snapshots;\n");
    let module_dir_tests = write("views/snapshots.rs", "");
    write("lib_root_sibling.rs", "");

    for path in [&tests, &fixtures, &nested, &assets_tests, &module_dir_tests] {
        assert!(is_test_path(path), "{} should be test code", path.display());
    }
    for path in [&prod, &assets_support, &module_dir, &src.join("lib.rs")] {
        assert!(
            !is_test_path(path),
            "{} should be production code",
            path.display()
        );
    }

    // End to end: a raw `public/` path in a test fixture module is not linted.
    let fixture = "impl WebRenderSync for Fixture {\n    fn render(self, cx: &mut WebContext) -> Result<(), WriteError> {\n        html_sync! { <img src=\"public/image.png\" alt=\"\" /> }.render(cx)\n    }\n}\n";
    std::fs::write(&assets_tests, fixture).unwrap();
    assert!(
        lint_file(&assets_tests, fixture)
            .iter()
            .all(|d| !d.code.starts_with("asset/")),
        "test module fixtures are test code"
    );
    std::fs::write(&assets_support, fixture).unwrap();
    assert!(
        lint_file(&assets_support, fixture)
            .iter()
            .any(|d| d.code == "asset/public-url"),
        "production modules are still linted"
    );
}

#[test]
fn asset_url_lint_ignores_reassigned_locals() {
    let source = r#"
pub struct Logo;
impl WebRenderSync for Logo {
    fn render(self, cx: &mut WebContext) -> Result<(), WriteError> {
        let mut stale = "public/logo.svg";
        stale = "/route";
        let mut current = "/route";
        current += "";
        let fixed = "public/fixed.svg";
        html_sync! {
            <img src={stale} alt="" />
            <img src={current} alt="" />
            <img src={fixed} alt="" />
        }
        .render(cx)
    }
}
"#;
    // Only the never-reassigned binding is trusted.
    assert_eq!(
        asset_hits("src/logo.rs", source),
        vec![(13, "asset/public-url")]
    );
}

#[test]
fn asset_url_lint_checks_static_text_before_markdown_slots() {
    let source = r####"
pub struct Notes;
impl MdRenderSync for Notes {
    fn render_md(self, cx: &mut WebContext) -> Result<(), WriteError> {
        md_sync! { "![Logo](public/logo.svg?v={version}) [see (public/x.svg)](public/x.svg) [ok](/docs/{page})", version = "2", page = "a" }
            .render_md(cx)
    }
}
"####;
    let line = source.lines().nth(4).unwrap();
    let hits: Vec<_> = lint_file(Path::new("src/notes.rs"), source)
        .into_iter()
        .filter(|d| d.code.starts_with("asset/"))
        .map(|d| (d.line, d.column))
        .collect();
    assert_eq!(
        hits,
        vec![
            (5, line.find("public/logo.svg?v=").unwrap() + 1),
            // The destination, not the copy inside the label.
            (5, line.rfind("public/x.svg").unwrap() + 1),
        ]
    );
}

#[test]
fn shared_or_mentioned_module_files_stay_production_code() {
    let temp = tempfile::tempdir().unwrap();
    let src = temp.path().join("src");
    let write = |relative: &str, contents: &str| {
        let path = src.join(relative);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, contents).unwrap();
        path
    };
    // The library ships `shared`; only the binary's tests use `helpers`.
    write("lib.rs", "mod shared;\nconst EXAMPLE: &str = r\"#[cfg(test)]\nmod quoted;\";\nmod outer {\n    #[cfg(test)]\n    mod inner_only;\n}\n");
    write(
        "main.rs",
        "#[cfg(test)]\nmod shared;\n#[cfg(test)]\nmod helpers;\n",
    );
    let shared = write("shared.rs", "");
    let helpers = write("helpers.rs", "");
    let quoted = write("quoted.rs", "");
    let inner_only = write("inner_only.rs", "");
    assert!(!is_test_path(&shared), "a production owner ships the file");
    assert!(is_test_path(&helpers));
    assert!(
        !is_test_path(&quoted),
        "text inside a string is not a declaration"
    );
    assert!(
        !is_test_path(&inner_only),
        "a declaration in an inline module owns another directory"
    );
}

#[test]
fn cfg_test_path_attribute_modules_are_test_paths() {
    let temp = tempfile::tempdir().unwrap();
    let src = temp.path().join("src");
    let write = |relative: &str, contents: &str| {
        let path = src.join(relative);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, contents).unwrap();
        path
    };
    write("lib.rs", "mod route;\n");
    write("route.rs", "mod response;\n");
    write(
        "route/response.rs",
        "#[cfg(test)]\n#[path = \"response_tests.rs\"]\nmod response_tests;\n#[path = \"shipped_impl.rs\"]\nmod shipped;\n",
    );
    let fixture = write("route/response_tests.rs", "");
    let shipped = write("route/shipped_impl.rs", "");
    let orphan = write("route/orphan.rs", "");
    assert!(is_test_path(&fixture));
    assert!(
        !is_test_path(&shipped),
        "a production #[path] owner ships the file"
    );
    assert!(!is_test_path(&orphan));
}

#[test]
fn production_path_owners_ship_files_that_a_test_module_also_declares() {
    let temp = tempfile::tempdir().unwrap();
    let src = temp.path().join("src");
    let write = |relative: &str, contents: &str| {
        let path = src.join(relative);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, contents).unwrap();
        path
    };
    write(
        "lib.rs",
        "#[cfg(test)]\nmod shared;\n#[path = \"shared.rs\"]\nmod shipped;\n#[cfg(test)]\nmod helpers;\n#[cfg(test)]\n#[path = \"helpers.rs\"]\nmod helpers_again;\n",
    );
    let shared = write("shared.rs", "");
    let helpers = write("helpers.rs", "");
    assert!(
        !is_test_path(&shared),
        "the #[path] declaration ships shared.rs"
    );
    assert!(
        is_test_path(&helpers),
        "every owner of helpers.rs is a test"
    );
}

#[test]
fn path_attribute_cycles_do_not_recurse_forever() {
    let temp = tempfile::tempdir().unwrap();
    let src = temp.path().join("src");
    std::fs::create_dir_all(&src).unwrap();
    let a = src.join("a.rs");
    let b = src.join("b.rs");
    std::fs::write(&a, "#[path = \"b.rs\"]\nmod x;\n").unwrap();
    std::fs::write(&b, "#[path = \"a.rs\"]\nmod y;\n").unwrap();
    assert!(!is_test_path(&a));
    assert!(!is_test_path(&b));
}

#[test]
fn unsaved_parent_declarations_decide_test_modules() {
    let temp = tempfile::tempdir().unwrap();
    let src = temp.path().join("src");
    std::fs::create_dir_all(&src).unwrap();
    let lib = src.join("lib.rs");
    std::fs::write(&lib, "#[cfg(test)]\nmod fixtures;\n").unwrap();
    let fixtures = src.join("fixtures.rs");
    std::fs::write(&fixtures, "").unwrap();
    assert!(is_test_path(&fixtures));
    {
        let _overrides = SourceOverrides::install([(lib.clone(), "mod fixtures;\n".to_string())]);
        assert!(!is_test_path(&fixtures), "the editor buffer wins");
    }
    assert!(
        is_test_path(&fixtures),
        "disk applies again after the snapshot"
    );
}

#[test]
fn standalone_sibling_files_keep_separate_asset_facts() {
    let temp = tempfile::tempdir().unwrap();
    let snippet = |name: &str| {
        let path = temp.path().join(name);
        std::fs::write(
            &path,
            "pub struct Avatar {\n    pub photo: &'static str,\n}\nimpl WebRenderSync for Avatar {\n    fn render(self, cx: &mut WebContext) -> Result<(), WriteError> {\n        html_sync! { <img src={self.photo} alt=\"\" /> }.render(cx)\n    }\n}\npub fn handler() -> Avatar {\n    Avatar {\n        photo: \"public/a.png\",\n    }\n}\n",
        )
        .unwrap();
        path
    };
    let first = snippet("first.rs");
    let second = snippet("second.rs");
    let hits: Vec<_> = lint_paths(&[first, second], LintOptions::default())
        .unwrap()
        .into_iter()
        .filter(|d| d.code == "asset/public-url")
        .map(|d| d.path.file_name().unwrap().to_owned())
        .collect();
    assert_eq!(hits, ["first.rs", "second.rs"]);
}

#[test]
fn asset_url_lint_tracks_reassignment_per_binding() {
    let source = r#"
pub struct Logo;
impl WebRenderSync for Logo {
    fn render(self, cx: &mut WebContext) -> Result<(), WriteError> {
        let image = "public/logo.svg";
        {
            let mut image = "/a";
            image = "/b";
        }
        let mut changed = "public/changed.svg";
        if true {
            changed = "/route";
        }
        html_sync! {
            <img src={image} alt="" />
            <img src={changed} alt="" />
        }
        .render(cx)
    }
}
"#;
    // The inner `image` is a different binding; the outer one is unchanged.
    assert_eq!(
        asset_hits("src/logo.rs", source),
        vec![(15, "asset/public-url")]
    );
}

#[test]
fn asset_url_lint_ignores_asset_text_in_markdown_query_strings() {
    let source = r####"
pub struct Links;
impl MdRenderSync for Links {
    fn render_md(self, cx: &mut WebContext) -> Result<(), WriteError> {
        md_sync! { "[Open](/search?next=/_proa/mutable/{name}) ![Raw](/_proa/mutable/{file})", name = "logo.svg", file = "x.svg" }
            .render_md(cx)
    }
}
"####;
    let line = source.lines().nth(4).unwrap();
    let hits: Vec<_> = lint_file(Path::new("src/links.rs"), source)
        .into_iter()
        .filter(|d| d.code.starts_with("asset/"))
        .map(|d| (d.line, d.column, d.code))
        .collect();
    assert_eq!(
        hits,
        vec![(
            5,
            line.rfind("/_proa/mutable/").unwrap() + 1,
            "asset/generated-url"
        )]
    );
}

#[test]
fn asset_url_lint_hides_names_shadowed_by_patterns() {
    let source = r#"
pub struct Logo;
impl WebRenderSync for Logo {
    fn render(self, cx: &mut WebContext) -> Result<(), WriteError> {
        let image = "public/unused.svg";
        let draw = |mut image: &str| {
            image = "/route";
            html_sync! { <img src={image} alt="" /> }
        };
        for image in ["/a"] {
            html_sync! { <img src={image} alt="" /> }.render(cx)?;
        }
        match Some("/b") {
            Some(image) => html_sync! { <img src={image} alt="" /> }.render(cx)?,
            None => {}
        }
        if let Some(image) = Some("/c") {
            html_sync! { <img src={image} alt="" /> }.render(cx)?;
        }
        let (image, _) = ("/d", 1);
        html_sync! { <img src={image} alt="" /> }.render(cx)
    }
}
"#;
    assert_eq!(asset_hits("src/logo.rs", source), vec![]);
}

#[test]
fn asset_url_lint_keeps_outer_bindings_after_pattern_scopes() {
    let source = r#"
pub struct Logo;
impl WebRenderSync for Logo {
    fn render(self, cx: &mut WebContext) -> Result<(), WriteError> {
        let image = "public/logo.svg";
        for image in ["/a"] {
            let _ = image;
        }
        let _ = |image: &str| image;
        html_sync! { <img src={image} alt="" /> }.render(cx)
    }
}
"#;
    assert_eq!(
        asset_hits("src/logo.rs", source),
        vec![(10, "asset/public-url")]
    );
}

#[test]
fn asset_url_lint_scales_linearly_with_function_length() {
    let body = |locals: usize| {
        let mut source = String::from(
            "pub struct Big;\nimpl WebRenderSync for Big {\n    fn render(self, cx: &mut WebContext) -> Result<(), WriteError> {\n",
        );
        for index in 0..locals {
            source.push_str(&format!("        let value_{index} = \"/route\";\n"));
        }
        source.push_str("        html_sync! { <main></main> }.render(cx)\n    }\n}\n");
        source
    };
    let time = |locals: usize| {
        let source = body(locals);
        let start = std::time::Instant::now();
        lint_file(Path::new("src/big.rs"), &source);
        start.elapsed()
    };
    time(250);
    let small = time(1_000);
    let large = time(8_000);
    // Eight times the locals: linear work is about 8x, quadratic about 64x.
    assert!(
        large < small * 24,
        "lint time grew from {small:?} to {large:?} for 8x the locals"
    );
}

#[test]
fn asset_url_lint_resolves_reassignment_inside_template_slots() {
    let source = r#"
pub struct Card {
    pub photo: &'static str,
}
impl WebRenderSync for Card {
    fn render(self, cx: &mut WebContext) -> Result<(), WriteError> {
        html_sync! {
            <div>{{ let mut image = self.photo; image = "/route"; html_sync! { <img src={image} alt="" /> } }}</div>
        }
        .render(cx)
    }
}

pub struct Badge {
    pub icon: &'static str,
}
impl WebRenderSync for Badge {
    fn render(self, cx: &mut WebContext) -> Result<(), WriteError> {
        html_sync! {
            <div>{{ let image = self.icon; html_sync! { <img src={image} alt="" /> } }}</div>
        }
        .render(cx)
    }
}

pub fn handler() -> (Card, Badge) {
    (
        Card {
            photo: "public/unused.svg",
        },
        Badge {
            icon: "public/icon.svg",
        },
    )
}
"#;
    // Only `Badge.icon` reaches the image; `Card`'s alias is reassigned.
    assert_eq!(
        asset_hits("src/cards.rs", source),
        vec![(32, "asset/public-url")]
    );
}

#[test]
fn asset_url_lint_catches_head_writer_setters_and_missing_public_files() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    std::fs::create_dir_all(root.join("src/pages")).unwrap();
    std::fs::create_dir_all(root.join("public/images")).unwrap();
    std::fs::create_dir_all(root.join("public/.well-known")).unwrap();
    std::fs::create_dir_all(root.join("styles")).unwrap();
    std::fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();
    std::fs::write(
        root.join("proa.config.rs"),
        r#"use proa::config::{Config, Result};
pub async fn config() -> Result<Config> {
    Ok(Config::default())
}
"#,
    )
    .unwrap();
    std::fs::write(root.join("styles/input.css"), "@import \"tailwindcss\";\n").unwrap();
    std::fs::write(root.join("public/favicon.svg"), "<svg/>").unwrap();
    std::fs::write(root.join("public/images/og.png"), b"png").unwrap();
    std::fs::write(
        root.join("public/.well-known/security.txt"),
        "Contact: hi\n",
    )
    .unwrap();

    // Note: `public/styles.css` is intentionally NOT on disk yet; because
    // `styles/input.css` exists and `build.assets` is default (`None`), the CLI
    // infers `public/styles.css` and the lint must not false-positive.
    std::fs::write(
        root.join("src/pages/home.rs"),
        r#"pub struct HomePage;
impl WebRenderSync for HomePage {
    fn render_head(&self, cx: &mut WebContext) -> Result<(), WriteError> {
        let _ = HeadIcon::new("/favicon.svg");
        let _ = HeadMedia::from_url("public/images/og.png");
        let _ = HeadIcon::from_url("/favicon.svg");
        let _ = cx.head_mut().og_image("/images/og.png");
        let _ = cx.head_mut().og_image("public/images/og.png");
        let _ = cx.head_mut().icon("/favicon.svg");
        let _ = cx.head_mut().stylesheet("app", "/favicon.svg");
        let _ = cx.head_mut().module("main", "public/images/og.png");
        // Valid resolved calls + dot-prefixed asset + inferred Tailwind output + unrelated `.icon(...)`
        let og = cx.url("public/images/og.png");
        let _ = cx.head_mut().og_image(og);
        let css = cx.url("public/styles.css");
        let _ = cx.head_mut().stylesheet("app", css);
        let _ = cx.url("public/.well-known/security.txt");
        let _ = HeadMedia::from_url(cx.url("public/images/og.png"));
        let _ = my_builder.icon("/favicon.svg");
        // Missing files under public/
        let _ = cx.url("public/images/typo.png");
        let _ = cx.url("public/images/absent.png?v=1#top");
        Ok(())
    }

    fn render(self, cx: &mut WebContext) -> Result<(), WriteError> {
        html_sync! { <main>"ok"</main> }.render(cx)
    }
}
"#,
    )
    .unwrap();

    let diagnostics =
        lint_paths(&[root.join("src/pages/home.rs")], LintOptions::default()).unwrap();
    let mut hits: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.code.starts_with("asset/"))
        .map(|d| (d.line, d.code))
        .collect();
    hits.sort();
    assert_eq!(
        hits,
        vec![
            (4, "asset/public-url"),           // HeadIcon::new("/favicon.svg")
            (5, "asset/public-url"),           // HeadMedia::from_url("public/images/og.png")
            (6, "asset/public-url"),           // HeadIcon::from_url("/favicon.svg")
            (7, "asset/public-url"),           // cx.head_mut().og_image("/images/og.png")
            (8, "asset/public-url"),           // cx.head_mut().og_image("public/images/og.png")
            (9, "asset/public-url"),           // cx.head_mut().icon("/favicon.svg")
            (10, "asset/public-url"),          // cx.head_mut().stylesheet("app", "/favicon.svg")
            (11, "asset/public-url"), // cx.head_mut().module("main", "public/images/og.png")
            (21, "asset/missing-public-file"), // cx.url("public/images/typo.png")
            (22, "asset/missing-public-file"), // cx.url("public/images/absent.png?v=1#top")
        ],
        "unexpected asset diagnostics: {diagnostics:#?}"
    );

    // Removing `styles/input.css` disables Tailwind inference, so missing `public/styles.css` now warns.
    std::fs::remove_file(root.join("styles/input.css")).unwrap();
    let without_input =
        lint_paths(&[root.join("src/pages/home.rs")], LintOptions::default()).unwrap();
    assert!(
        without_input
            .iter()
            .any(|d| d.line == 15 && d.code == "asset/missing-public-file"),
        "expected missing-public-file for public/styles.css without styles/input.css: {without_input:#?}"
    );

    // Explicit `build.assets: Some(vec![AssetConfig { output: ... }])` inside `vec!`
    // exempts the declared output (`public/styles.css`) even before the build runs,
    // while another unbuilt output (`public/other.css`) still warns.
    std::fs::write(
        root.join("proa.config.rs"),
        r#"use proa::config::{AssetConfig, BuildConfig, Config, Result};
pub async fn config() -> Result<Config> {
    Ok(Config {
        build: BuildConfig {
            assets: Some(vec![AssetConfig {
                kind: "tailwind",
                command: vec![],
                input: "styles/main.css",
                output: "public/styles.css",
                config: None,
            }]),
            ..Default::default()
        },
        ..Config::default()
    })
}
"#,
    )
    .unwrap();
    let with_declared =
        lint_paths(&[root.join("src/pages/home.rs")], LintOptions::default()).unwrap();
    assert!(
        !with_declared
            .iter()
            .any(|d| d.line == 15 && d.code == "asset/missing-public-file"),
        "declared `vec![AssetConfig {{ output: \"public/styles.css\" }}]` must not warn: {with_declared:#?}"
    );
}

#[test]
fn static_mounts_outside_the_build_output_are_reported() {
    let source = r#"
use include_dir::{include_dir, Dir};
use tower_http::services::{ServeDir, ServeFile};

static PUBLIC: Dir = include_dir!("$CARGO_MANIFEST_DIR/public");
static MIGRATIONS: Dir = include_dir!("$CARGO_MANIFEST_DIR/migrations");
static KEYS: Dir = include_dir!("$CARGO_MANIFEST_DIR/public_keys");
static RELATIVE: Dir = include_dir!("public");

pub fn router() -> Router {
    Router::new()
        .nest_service("/static", ServeDir::new("public"))
        .route_service("/logo.svg", tower_http::services::ServeFile::new_with_mime("public/logo.svg", &mime))
        .nest_service("/uploads", ServeDir::new(uploads_directory())) // proa-perf-ok: uploads exist only at run time
        .route("/health", get(health))
}

#[cfg(test)]
mod tests {
    fn fixture() -> ServeDir {
        ServeDir::new("fixtures")
    }
}
"#;
    let hits: Vec<_> = lint_file(Path::new("src/router.rs"), source)
        .into_iter()
        .filter(|d| d.code == "asset/unmanaged-static")
        .map(|d| (d.line, d.message))
        .collect();
    assert_eq!(
        hits,
        vec![
            (
                5,
                "`include_dir!` serves files the build output does not contain".to_string()
            ),
            (
                8,
                "`include_dir!` serves files the build output does not contain".to_string()
            ),
            (
                12,
                "`ServeDir` serves files the build output does not contain".to_string()
            ),
            (
                13,
                "`ServeFile` serves files the build output does not contain".to_string()
            ),
        ]
    );
}

fn route_path_diagnostics(path: &Path, source: &str) -> Vec<Diagnostic> {
    lint_file(path, source)
        .into_iter()
        .filter(|d| d.code == "route/express-path-syntax")
        .collect()
}

/// The text a diagnostic's primary label underlines.
fn underlined(source: &str, diagnostic: &Diagnostic) -> String {
    let range = &diagnostic.labels[0].range;
    source
        .lines()
        .nth(range.line - 1)
        .unwrap()
        .chars()
        .skip(range.column - 1)
        .take(range.end_column - range.column)
        .collect()
}

#[test]
fn express_route_syntax_is_an_error_in_every_router_path_position() {
    let source = r#"use axum::{routing::get, Router};
use proa::axum::{router_from_spec, FrameworkBuilder, Route};

pub fn router(state: AppState) -> Router {
    let (router, metadata) = router_from_spec(vec![
        Route::endpoint("/", get(pages::home::handler)),
    ]);
    router
        .route("/healthz", get(health))
        .route("/api/issues/:id", get(api::issue).delete(api::delete_issue))
        .route("/orgs/:org/repos/:repo", get(api::repo))
        .route_service("/media/*file", media())
        .nest("/api/:version", api::router())
        .nest_service("/static/*path", static_assets::service())
        .with_state(state)
}
"#;
    let diagnostics = route_path_diagnostics(Path::new("src/routes.rs"), source);
    let found: Vec<_> = diagnostics
        .iter()
        .map(|d| {
            (
                d.line,
                underlined(source, d),
                d.suggestions[0].replacement.clone(),
            )
        })
        .collect();
    let expect = |line, segment: &str, fixed: &str| (line, segment.to_owned(), fixed.to_owned());
    assert_eq!(
        found,
        [
            expect(10, ":id", r#""/api/issues/{id}""#),
            expect(11, ":org", r#""/orgs/{org}/repos/{repo}""#),
            expect(12, "*file", r#""/media/{*file}""#),
            expect(13, ":version", r#""/api/{version}""#),
            expect(14, "*path", r#""/static""#),
        ],
        "{diagnostics:#?}"
    );
    for diagnostic in &diagnostics {
        assert_eq!(diagnostic.severity, Severity::Error);
        assert_eq!(diagnostic.column, diagnostic.labels[0].range.column);
        assert!(diagnostic.notes[0].contains("panics while the router is built"));
        // The fix replaces the whole literal and nothing else.
        let suggestion = &diagnostic.suggestions[0];
        assert_eq!(
            suggestion.applicability,
            crate::diagnostic::SuggestionApplicability::MachineApplicable
        );
        assert_eq!(suggestion.range.line, diagnostic.line);
    }
    assert_eq!(
        diagnostics[0].to_string(),
        r#"error[route/express-path-syntax]: route path `/api/issues/:id` uses the Express-style capture `:id`; Axum 0.8 writes captures as `{id}`
  --> src/routes.rs:10:29
    |
  9 |         .route("/healthz", get(health))
 10 |         .route("/api/issues/:id", get(api::issue).delete(api::delete_issue))
    |                             ^^^ Axum 0.8 rejects a segment that starts with `:`
 11 |         .route("/orgs/:org/repos/:repo", get(api::repo))
 12 |         .route_service("/media/*file", media())
    |
   = note: Axum 0.8 panics while the router is built ("Path segments must not start with `:`"), so the server exits at startup instead of serving requests
   = note: for more information, see https://proa.so/docs/framework/routing#register-a-route
   = help: write the path as
           "/api/issues/{id}"
"#
    );
    assert!(diagnostics[4]
        .message
        .starts_with("nest prefix `/static/*path` ends in the Express-style wildcard `*path`"));

    // The corrected file is clean.
    let fixed = source
        .replace(":id", "{id}")
        .replace(":org", "{org}")
        .replace(":repo", "{repo}")
        .replace("*file", "{*file}")
        .replace(":version", "{version}")
        .replace("/static/*path", "/static");
    assert!(route_path_diagnostics(Path::new("src/routes.rs"), &fixed).is_empty());
}

#[test]
fn express_route_syntax_stays_quiet_without_proof_of_an_axum_0_8_panic() {
    let proa = "use axum::{routing::get, Router};\nuse proa::axum::{router_from_spec, FrameworkBuilder, Route};\n";
    for (why, source) in [
        (
            "an unrelated receiver in an Axum application",
            format!("{proa}fn menu(nav: Nav) -> Nav {{ nav.route(\"/issues/:id\", item()) }}"),
        ),
        (
            "no Axum in the file",
            "fn menu(nav: Nav) -> Nav { nav.route(\"/issues/:id\", item()) }".to_owned(),
        ),
        (
            "Axum of an unknown version, outside a Proa package",
            "use axum::{routing::get, Router};\nfn app() -> Router { Router::new().route(\"/issues/:id\", get(show)) }".to_owned(),
        ),
        (
            "the author opted out of the check",
            format!("{proa}fn app() -> Router {{ Router::new().without_v07_checks().route(\"/:literal\", get(show)) }}"),
        ),
        (
            // `proa_framework_core::route_spec` rewrites both spellings to
            // braces, so these routes are served.
            "Proa route specs accept the old spelling",
            format!("{proa}fn specs() -> Vec<Route<()>> {{ vec![Route::endpoint(\"/issues/:id\", get(show)), Route::page(\"/files/*path\", \"Files\", get(show)), proa::axum::Route::layout(\"/docs/:section\", DOCS, vec![])] }}"),
        ),
        (
            "a path that is not a literal argument",
            format!("{proa}const ISSUE: &str = \"/issues/:id\";\nfn app() -> Router {{ Router::new().route(ISSUE, get(show)).route(&format!(\"/{{}}/:id\", base()), get(show)) }}"),
        ),
        (
            "a literal that is not the path argument",
            format!("{proa}fn app() -> Router {{ Router::new().route(path(), label(\"/issues/:id\")).fallback(\"/missing/:id\") }}"),
        ),
        (
            "colons and stars inside a segment",
            format!("{proa}fn app() -> Router {{ Router::new().route(\"/time/12:30/a*b\", get(show)).route(\"/issues/{{id}}\", get(show)).route(\"/files/{{*path}}\", get(show)) }}"),
        ),
        (
            "a relative path, which Axum rejects for a different reason",
            format!("{proa}fn app() -> Router {{ Router::new().route(\":id\", get(show)) }}"),
        ),
        (
            "test code",
            format!("{proa}#[cfg(test)]\nmod tests {{\n    #[test]\n    fn old_syntax_panics() {{ let _ = Router::<()>::new().route(\"/issues/:id\", get(show)); }}\n}}"),
        ),
    ] {
        let diagnostics = route_path_diagnostics(Path::new("/nowhere/src/routes.rs"), &source);
        assert!(diagnostics.is_empty(), "{why}: {diagnostics:#?}");
    }
    let test_file =
        format!("{proa}fn app() -> Router {{ Router::new().route(\"/issues/:id\", get(show)) }}");
    assert!(route_path_diagnostics(Path::new("tests/routes.rs"), &test_file).is_empty());
    assert_eq!(
        route_path_diagnostics(Path::new("/nowhere/src/routes.rs"), &test_file).len(),
        1
    );
}

#[test]
fn express_route_fix_preserves_quoted_path_segments_and_router_aliases() {
    let source = r##"use axum::Router as Routes;
use proa::axum::FrameworkBuilder;
fn router() -> Routes { Routes::new().route(r#"/say/"hello"/:id"#, get(show)) }
"##;
    let diagnostics = route_path_diagnostics(Path::new("src/routes.rs"), source);
    assert_eq!(diagnostics.len(), 1);
    let fixed: syn::LitStr = syn::parse_str(&diagnostics[0].suggestions[0].replacement).unwrap();
    assert_eq!(fixed.value(), r#"/say/"hello"/{id}"#);
}

#[test]
fn express_route_syntax_uses_the_package_to_prove_axum_0_8() {
    let source = "use axum::{routing::get, Router};\npub fn api() -> Router { Router::new().route(\"/api/issues/:id\", get(show)) }\n";
    let lint_in = |files: &[(&str, &str)]| {
        let tmp = tempfile::tempdir().unwrap();
        for (name, contents) in files {
            std::fs::write(tmp.path().join(name), contents).unwrap();
        }
        std::fs::create_dir(tmp.path().join("src")).unwrap();
        let path = tmp.path().join("src/api.rs");
        std::fs::write(&path, source).unwrap();
        route_path_diagnostics(&path, source).len()
    };
    let proa_app = "[package]\nname = \"app\"\n\n[dependencies]\naxum = \"0.8.9\"\nproa = { version = \"0.4.2\", features = [\"axum\"] }\n";
    assert_eq!(lint_in(&[("Cargo.toml", proa_app)]), 1);
    let legacy = "[package]\nname = \"legacy\"\n\n[dependencies]\naxum = \"0.7\"\n";
    assert_eq!(lint_in(&[("Cargo.toml", legacy)]), 0);
    assert_eq!(
        lint_in(&[("Cargo.toml", legacy), ("proa.config.rs", "")]),
        1
    );
    assert_eq!(lint_in(&[]), 0);
}

#[test]
fn express_route_syntax_without_a_name_is_not_machine_fixable() {
    let source = "use proa::axum::FrameworkBuilder;\nfn mount(app: axum::Router) -> axum::Router {\n    app.route_service(r\"/files/*\", files())\n        .nest_service(\"/*path\", files())\n}\n";
    let diagnostics = route_path_diagnostics(Path::new("src/routes.rs"), source);
    assert_eq!(diagnostics.len(), 2, "{diagnostics:#?}");
    // A raw literal is underlined whole.
    assert_eq!(underlined(source, &diagnostics[0]), "r\"/files/*\"");
    assert_eq!(
        diagnostics[0].suggestions[0].replacement,
        "\"/files/{*rest}\""
    );
    assert_eq!(
        diagnostics[0].suggestions[0].applicability,
        crate::diagnostic::SuggestionApplicability::MaybeIncorrect
    );
    assert!(diagnostics[1].suggestions.is_empty());
    assert!(diagnostics[1]
        .help
        .as_deref()
        .unwrap()
        .contains("cannot be nested at `/`"));
}

#[test]
fn documented_rules_end_with_one_link_to_their_guide() {
    let source = r####"use axum::Router;
use proa::axum::RouteResponse;
use proa::{html_sync, md_sync, MdRenderSync, WebContext, WebRender, WebRenderSync, WriteError};
use proa::stream::Suspense;

struct Product { title: String, price: u32 }
impl WebRenderSync for Product {
    fn render(self, cx: &mut WebContext) -> Result<(), WriteError> {
        html_sync! { <h1>{self.title}</h1><p>{self.price}</p> }.render(cx)
    }
}
impl MdRenderSync for Product {
    fn render_md(self, cx: &mut WebContext) -> Result<(), WriteError> {
        md_sync! { "# {self.title}" }.render_md(cx)
    }
}
struct Home;
impl WebRenderSync for Home {
    const ROLE: proa::ComponentRole = proa::ComponentRole::PAGE;
    fn render(self, cx: &mut WebContext) -> Result<(), WriteError> {
        cx.head_mut().title("Home")?;
        html_sync! { <main>{Suspense::new(Home, ())}{Home}</main> }.render(cx)
    }
}
struct Untitled;
impl WebRenderSync for Untitled {
    fn render_head(&self, cx: &mut WebContext) -> impl proa::SyncHeadOperation { proa::NoHead }
    fn render(self, cx: &mut WebContext) -> Result<(), WriteError> {
        let label = String::new();
        html_sync! { <main>{label}</main> }.render(cx)
    }
}
fn api() -> Router { Router::new().route("/issues/:id", axum::routing::get(show)) }
"####;
    let guides = [
        (
            "render/head-role-required",
            "https://proa.so/docs/framework/layouts-and-pages#roles-are-explicit",
        ),
        (
            "render/head-mutation-in-body",
            "https://proa.so/docs/framework/layouts-and-pages#roles-are-explicit",
        ),
        (
            "render/head-role-in-suspense",
            "https://proa.so/docs/framework/layouts-and-pages#roles-are-explicit",
        ),
        (
            "render/head-role-in-page",
            "https://proa.so/docs/framework/layouts-and-pages#roles-are-explicit",
        ),
        (
            "render/content-coverage",
            "https://proa.so/docs/cli/format-lint-check#content-coverage",
        ),
        (
            "route/express-path-syntax",
            "https://proa.so/docs/framework/routing#register-a-route",
        ),
    ];
    let tmp = tempfile::tempdir().unwrap();
    std::fs::create_dir(tmp.path().join("src")).unwrap();
    let path = tmp.path().join("src/lib.rs");
    std::fs::write(&path, source).unwrap();
    let from_paths = lint_paths(std::slice::from_ref(&path), LintOptions::default()).unwrap();
    for diagnostics in [lint_file(&path, source), from_paths] {
        for (code, guide) in guides {
            let diagnostic = diagnostics
                .iter()
                .find(|d| d.code == code)
                .unwrap_or_else(|| panic!("{code}: {diagnostics:#?}"));
            let links: Vec<_> = diagnostic
                .notes
                .iter()
                .filter(|note| note.contains("https://"))
                .collect();
            assert_eq!(
                links,
                [&format!("for more information, see {guide}")],
                "{code}"
            );
            assert_eq!(diagnostic.notes.last(), Some(links[0]), "{code}");
        }
        // A rule without a guide of its own carries no link.
        let unlinked = diagnostics
            .iter()
            .find(|d| d.code == "perf/string-new")
            .unwrap_or_else(|| panic!("{diagnostics:#?}"));
        assert!(unlinked.notes.is_empty(), "{:?}", unlinked.notes);
    }
}

#[test]
fn a_managed_layout_requires_a_page_in_its_sole_body_slot() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    std::fs::create_dir_all(root.join("src/layouts")).unwrap();
    std::fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"fixture\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();
    std::fs::write(
        root.join("src/layouts/mod.rs"),
        "mod root; pub use root::Shell;",
    )
    .unwrap();
    std::fs::write(root.join("src/layouts/root.rs"), r#"
pub struct Shell<C> { pub content: C }
impl<C: WebRender> WebRender for Shell<C> {
    const ROLE: ComponentRole = ComponentRole::LAYOUT;
    fn render(self, cx: &mut WebContext) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>> {
        RenderOutcome::pending(async move {
            let path = cx.url("public/logo.svg");
            html! { <head>{cx.head()}</head><body><img src={path}>{self.content}</body> }.render(cx).resolve().await
        })
    }
}
"#).unwrap();
    let path = root.join("src/pages.rs");
    std::fs::write(&path, r#"
use crate::layouts::Shell;
struct Leaf;
impl WebRenderSync for Leaf {
    fn render(self, cx: &mut WebContext) -> Result<(), WriteError> { html_sync! { <p>"body"</p> }.render(cx) }
}
struct Page;
impl WebRenderSync for Page {
    const ROLE: ComponentRole = ComponentRole::PAGE;
    fn render(self, cx: &mut WebContext) -> Result<(), WriteError> { html_sync! { <p/> }.render(cx) }
}
fn routes() {
    RouteResponse::ssr(Shell { content: Leaf });
    RouteResponse::ssr(Shell { content: Page });
    RouteResponse::ssr(Leaf);
}
"#).unwrap();
    let diagnostics = lint_paths(&[path], LintOptions::default()).unwrap();
    assert_eq!(
        codes_at(&diagnostics, "render/head-page-missing"),
        [13],
        "{diagnostics:#?}"
    );
}

#[test]
fn rsjs_requests_explain_origin_paths_and_dynamic_cache_keys() {
    let source = r#"
struct Counter { id: String }
#[rsjs(component, client)]
impl WebRenderSync for Counter {
    fn render(self, cx: &mut WebContext) -> Result<(), WriteError> {
        let a = rsjs::query_with_initial(QueryKey::new(("issue", self.id)), 0, rsjs::FetchRequest { url: "/api/issues", ..Default::default() }, Default::default());
        let b = rsjs::query_with_initial(QueryKey::new("all"), 0, rsjs::FetchRequest { url: "/api/all", apply_base_path: false, ..Default::default() }, Default::default());
        let c = rsjs::mutation(QueryKey::new("update"), rsjs::FetchRequest { url: "https://other.example/", ..Default::default() }, &[]);
        html_sync! { <main/> }.render(cx)
    }
}
"#;
    let diagnostics = lint_file(Path::new("src/components.rs"), source);
    for code in ["rsjs/fetch-base-path", "rsjs/query-key-static-url"] {
        assert_eq!(codes_at(&diagnostics, code), [6], "{diagnostics:#?}");
        assert!(diagnostics
            .iter()
            .filter(|d| d.code == code)
            .all(|d| d.severity == Severity::Info));
    }
}
