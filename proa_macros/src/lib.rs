// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Proa Macros
//!
//! Provides `html!` / `html_sync!` and `md!` / `md_sync!` for
//! high-performance HTML and Markdown generation in Proa.
//!
//! The async-capable `html!` macro consolidates static HTML portions at compile
//! time while driving child expressions through `WebRender`. The sync-only
//! `html_sync!` macro keeps the original zero-overhead `WebRenderSync` fast
//! path for pre-existing sync templates.
//!
//! Markdown follows the same contract: `md!` composes `MdRender` children and
//! `md_sync!` preserves the `MdRenderSync` fast path.
//!
use proc_macro::TokenStream;
use syn::parse_macro_input;
use syn::LitStr;

mod action_derive;
mod component_derive;
mod const_recipe;
mod content;
mod form_schema_derive;
mod island_attr;
mod md;
mod parse;
mod static_consolidation;
mod text_macro;
mod tree_codegen;
mod tsconfig;
mod tsx_parser;
mod tsx_resolve;
mod variant_derive;

use parse::{HtmlRoot, XmlRoot};

/// Describe a render view's content-coverage policy for `proa lint`.
///
/// `#[content(skip)]` exempts a view on its impl or render method. Pair a
/// separate representation with `#[content(source = HtmlProduct)]`. An optional
/// `reason = "..."` documents intent. This attribute preserves the renderer;
/// content coverage is checked by the linter, not macro expansion.
#[proc_macro_attribute]
pub fn content(args: TokenStream, input: TokenStream) -> TokenStream {
    content::policy(args.into(), input.into())
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

/// Acknowledge one intentionally omitted content contribution.
///
/// Put this as a Rust statement inside the render method that deliberately
/// omits the contribution, before its template. For a local, place it after
/// the declaration and before the value is moved.
///
/// Use `omit!(self.image)`, `omit!(&notice, "reason")`,
/// `omit!(crate::RestockNotice)`, or
/// `omit!("restock")`. Optional `from = "html"` restricts the
/// source comparison. Value paths are type-checked without being evaluated
/// or moved. Bare paths select child types; borrow a local with `&notice` to
/// select its value. Child types are checked without constructing them, and
/// ordinary Rust generic arguments apply, e.g. `omit!(RestockNotice<'_>)`.
#[proc_macro]
pub fn omit(input: TokenStream) -> TokenStream {
    content::omit(input.into())
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

/// Give a rendered value a shared semantic identity across representations.
///
/// `content_part!("restock", RestockNotice::new(data))` returns the supplied
/// expression unchanged. The name lets `proa lint` recognize corresponding
/// content even when targets use different wrappers or helper functions.
#[proc_macro]
pub fn content_part(input: TokenStream) -> TokenStream {
    content::part(input.into())
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

/// The `html!` macro for generating async-capable HTML fragments.
///
/// This macro analyzes the HTML tree at compile time and consolidates static
/// portions into single byte literals, dramatically reducing runtime overhead.
/// Returns an `AsyncHtml` that implements `WebRender`.
///
/// ## Syntax
///
/// ```rust,ignore
/// html! {
///     <div class="container">
///         <h1>"Hello"</h1>
///         {some_component}
///     </div>
/// }.render(cx);
/// ```
///
/// ## Features
///
/// - HTML elements: `<div>`, `<span>`, etc.
/// - Attributes: `class="value"` or `class={expr}`
/// - Text nodes: `"Hello world"`
/// - Rust expressions: `{expr}` (must implement `WebRender`)
/// - Void elements: `<img />`, `<br />`
/// - Static consolidation: consecutive static elements merged into byte literals
/// - Compile-time escaping: text literals escaped at macro expansion time
///
/// ## Performance
///
/// Static HTML portions are merged into single byte array writes, making this
/// 10-20x faster than traditional approaches for pages with mostly static content.
#[proc_macro]
pub fn html(input: TokenStream) -> TokenStream {
    let root = parse_macro_input!(input as HtmlRoot);

    // Compile-time security, a11y, and performance validation.
    // Hard errors (XSS, invalid ARIA) abort compilation.
    // Soft warnings (a11y hints, perf hints) emit #[deprecated] structs
    // that show as rustc warnings with file/line info.
    let warnings = match static_consolidation::validate_security(&root) {
        Err(errors) => return TokenStream::from(errors),
        Ok(w) => w,
    };

    let expanded = static_consolidation::generate_optimized_async(&root);

    TokenStream::from(quote::quote! { { #warnings #expanded } })
}

/// The send-capable HTML macro for `WebRenderSend` component trees.
///
/// Its syntax and compile-time validation match [`html`], but interpolated
/// children are driven through `WebRenderSend` and the generated continuation
/// carries a compiler-checked `Send` bound.
#[proc_macro]
pub fn html_send(input: TokenStream) -> TokenStream {
    let root = parse_macro_input!(input as HtmlRoot);

    let warnings = match static_consolidation::validate_security(&root) {
        Err(errors) => return TokenStream::from(errors),
        Ok(warnings) => warnings,
    };
    let expanded = static_consolidation::generate_optimized_send_async(&root);
    TokenStream::from(quote::quote! { { #warnings #expanded } })
}

/// The `html_sync!` macro for generating sync-only HTML fragments.
///
/// This preserves Proa's original zero-overhead `WebRenderSync` codegen. Use
/// it for existing sync components and for hot paths that do not need async
/// child composition.
#[proc_macro]
pub fn html_sync(input: TokenStream) -> TokenStream {
    let root = parse_macro_input!(input as HtmlRoot);

    let warnings = match static_consolidation::validate_security(&root) {
        Err(errors) => return TokenStream::from(errors),
        Ok(w) => w,
    };

    let expanded = static_consolidation::generate_optimized(&root);

    TokenStream::from(quote::quote! { { #warnings #expanded } })
}

/// The `text!` macro for mixed static, Proa-rendered, and Rust-formatted text.
///
/// Bare `{}` placeholders use Proa rendering in text position and `AttrValue`
/// rendering in attribute position. Formatted `{:...}` placeholders use Rust
/// formatting semantics with Proa escaping. Primitive integers select an
/// allocation-free fast path for decimal width/alignment/sign formatting,
/// binary/octal/hex (including alternate prefixes), and integer scientific
/// notation. Exact fixed-decimal formats such as `{:.2}` select the equivalent
/// primitive `f32` / `f64` fast path. Other value types retain their normal Rust
/// formatting-trait behavior.
/// `{{` / `}}` write literal braces as static text, matching `format!`.
///
/// A direct `text!(...)` expression inside `html!` / `html_sync!` is an HTML
/// compiler intrinsic: its static pieces are folded into the surrounding
/// template and it does not need to be imported separately. Recognition is
/// intentionally narrow: `text!(...)`, `proa_macros::text!(...)`, and
/// `::proa_macros::text!(...)` are accepted; another namespace's `text!`
/// remains an ordinary Rust macro. Standalone `text!` values still require
/// this proc macro to be in scope. A direct placeholder-free URL attribute,
/// such as `href={text!("/docs")}`, receives static dangerous-scheme
/// validation and folds into the template. Placeholder-bearing URL values are
/// rejected because their complete value cannot be validated safely, and a
/// stored `text!` value is rejected at runtime. Build a validated URL value
/// first or use a typed URL helper for composite URLs.
/// Inside an RSJS-analyzed template, direct `text!` calls can track reactive
/// arguments in text and ordinary value attributes. Client formatting supports
/// integer display/binary/octal/hex and string display with literal format
/// plans; other Rust formatting traits remain server-only.
#[proc_macro]
pub fn text(input: TokenStream) -> TokenStream {
    text_macro::expand(input)
}

/// The `xml!` macro for generating async-capable XML fragments.
///
/// This shares the `html!` syntax and optimized byte consolidation, but uses
/// XML escaping rules: U+00A0 passes through as UTF-8 and U+0000 is rejected.
/// HTML-only validation is intentionally not applied because XML vocabularies
/// are application-defined.
#[proc_macro]
pub fn xml(input: TokenStream) -> TokenStream {
    let root = parse_macro_input!(input as XmlRoot).into_inner();

    let warnings = match static_consolidation::validate_xml(&root) {
        Err(errors) => return TokenStream::from(errors),
        Ok(w) => w,
    };

    let expanded = static_consolidation::generate_optimized_async_xml(&root);

    TokenStream::from(quote::quote! { { #warnings #expanded } })
}

/// The `xml_sync!` macro for generating sync-only XML fragments.
///
/// Use this for non-async XML render paths. It preserves static consolidation
/// and switches dynamic text/attribute escaping to XML while the fragment
/// renders.
#[proc_macro]
pub fn xml_sync(input: TokenStream) -> TokenStream {
    let root = parse_macro_input!(input as XmlRoot).into_inner();

    let warnings = match static_consolidation::validate_xml(&root) {
        Err(errors) => return TokenStream::from(errors),
        Ok(w) => w,
    };

    let expanded = static_consolidation::generate_optimized_xml(&root);

    TokenStream::from(quote::quote! { { #warnings #expanded } })
}

/// The `md!` macro for generating async-capable Markdown fragments.
///
/// ## Raw Markdown template mode (preferred)
///
/// ```rust,ignore
/// use proa_macros::md;
///
/// let title = "World";
/// let body = proa_core::to_md_async("Async-capable body");
/// let output = md! { r###"
///     # Hello {title}
///
///     {body}
///     "###
/// };
/// ```
///
/// - The raw string is trusted, compile-time Markdown
/// - `{expr}` composes an async-capable `MdRender` child
/// - Source newlines are rendered directly, without escaped `"\n"` separators
/// - Common Rust indentation is stripped while relative Markdown indentation is
///   preserved
/// - Braces in fenced code blocks and inline code spans stay literal
/// - Outside code, `{{` and `}}` render literal braces
/// - `{for ...}` and `{if ...}` provide template control flow
/// - Wrap synchronous custom children with `proa_core::to_md_async(...)`
///
/// ## Editor support
///
/// The [Proa extension](https://github.com/Proa-Labs/proa/tree/main/editors/vscode)
/// adds Rust highlighting and navigation inside raw-template expressions using
/// the existing rust-analyzer server. See its documentation for supported
/// template forms and editor limitations. It is not required for compilation.
///
/// ## Named arguments and native IDE support
///
/// `format!`-style `name = expr` arguments may follow the template. They offer
/// an alternative to in-string expressions like `{stats.min}` when using
/// stock rust-analyzer without the Proa extension: the expression is written
/// as real macro tokens, so go-to-definition, rename, and type diagnostics
/// work on it.
///
/// ```rust,ignore
/// md! { r###"
///     | Points | Min (ms) |
///     | --- | --- |
///     | {count} | {min} |
///     "###,
///     min = stats.min,
/// }
/// ```
///
/// When every placeholder is a bare identifier and the template has no
/// control flow, rust-analyzer additionally resolves the placeholders inside
/// the string itself — go-to-definition and rename on `{count}` work exactly
/// as they do in `format!`. That support type-checks placeholders against
/// `Display` inside the IDE; for a value that only implements the Markdown
/// render traits (a nested fragment, say), write the placeholder as
/// `{ ident }` with spaces — rendering is identical and the IDE leaves it
/// alone.
///
/// Use three hashes by default. Increase the delimiter only if the Markdown
/// contains the exact closing sequence `"###`. The expression-visible token
/// form remains supported for generated templates and token-aware tooling:
///
/// ```rust,ignore
/// md! { "# Hello " { title } }
/// ```
#[proc_macro]
pub fn md(input: TokenStream) -> TokenStream {
    expand_md(input, md::EmitMode::Async)
}

/// The `md_sync!` macro for synchronous Markdown fragments.
///
/// This is the zero-overhead `MdRenderSync` counterpart to async-capable
/// [`md!`]. Its syntax is identical — including `format!`-style named
/// arguments and the rust-analyzer placeholder support described there — but
/// every interpolated `{expr}` must implement `MdRenderSync`.
/// See [`md!`]'s editor-support documentation for the Proa extension and
/// native IDE support through named arguments.
///
/// ```rust,ignore
/// use proa_macros::md_sync;
///
/// let title = "World";
/// let body = "Synchronous body";
/// let output = md_sync! { r###"
///     # Hello {title}
///
///     {body}
///     "###
/// };
/// ```
///
/// Raw templates may align with the surrounding Rust without adding that
/// alignment to the output:
///
/// ```rust,ignore
/// let frontmatter = md_sync! { r###"
///     ---
///     title: Hello
///     ---
///     "###
/// };
/// ```
#[proc_macro]
pub fn md_sync(input: TokenStream) -> TokenStream {
    expand_md(input, md::EmitMode::Sync)
}

fn expand_md(input: TokenStream, mode: md::EmitMode) -> TokenStream {
    // Preferred template mode: one multiline raw literal, optionally followed
    // by `format!`-style `name = expr` arguments. Token mode's first token is
    // also a literal, but it is never followed by a comma, so the parse
    // failing is what routes it to the fallback below.
    let input2 = proc_macro2::TokenStream::from(input.clone());
    if let Ok(tpl) = syn::parse2::<md::MdTemplateInput>(input2) {
        return match md::generate_template(&tpl, mode) {
            Ok(tokens) => TokenStream::from(tokens),
            Err(err) => TokenStream::from(err.to_compile_error()),
        };
    }

    // Expression-visible token mode fallback.
    let root = parse_macro_input!(input as md::MdRoot);
    TokenStream::from(md::generate_tokens(&root, mode))
}

/// Embed a CSS file into the binary at compile time.
///
/// The path is resolved from the caller crate's `CARGO_MANIFEST_DIR`.
///
/// ## Example
///
/// ```rust,ignore
/// const SHARED_CSS: &str = proa_macros::include_css!("src/styles/shared.css");
/// ```
#[proc_macro]
pub fn include_css(input: TokenStream) -> TokenStream {
    let path = parse_macro_input!(input as LitStr);
    let path_value = path.value();

    let expanded = quote::quote! {{
        const _: &[u8] = include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/", #path_value));
        include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/", #path_value))
    }};

    TokenStream::from(expanded)
}

/// The `html_with_tree!` macro for generating both optimized HTML and a ComponentTree.
///
/// This macro is an experimental sync-only companion to `html_sync!` that also
/// generates a `ComponentTree` structure for TypeScript code generation. It
/// returns a tuple of `(OptimizedHtml, ComponentTree)`.
///
/// ## Example
///
/// ```rust,ignore
/// use proa_macros::html_with_tree;
///
/// let (html, tree) = html_with_tree! {
///     <button class={classes} disabled={self.disabled}>
///         {self.loading.then(|| html! { <Spinner class="animate-spin" /> })}
///         {children}
///     </button>
/// };
///
/// // Use html for SSR
/// html.render(cx);
///
/// // Use tree for TypeScript codegen analysis
/// let deps = tree.component_dependencies(); // ["Spinner"]
/// ```
///
/// ## Features
///
/// - Returns `(OptimizedHtml, ComponentTree)` tuple
/// - ComponentTree captures the structure for TS/JSX generation
/// - Automatically detects HTML elements vs components (PascalCase = component)
/// - Recognizes conditional rendering patterns like `.then(|| ...)`
/// - Identifies children slots
#[proc_macro]
pub fn html_with_tree(input: TokenStream) -> TokenStream {
    let root = parse_macro_input!(input as HtmlRoot);

    // Compile-time validation — same as html!
    let warnings = match static_consolidation::validate_security(&root) {
        Err(errors) => return TokenStream::from(errors),
        Ok(w) => w,
    };

    // Generate both outputs
    let html_expanded = static_consolidation::generate_optimized(&root);
    let tree_expanded = tree_codegen::generate_tree(&root);

    // Return a tuple
    let expanded = quote::quote! {
        { #warnings (#html_expanded, #tree_expanded) }
    };

    TokenStream::from(expanded)
}

/// The `html_tree!` macro for generating only a ComponentTree.
///
/// This is a simpler version of `html_with_tree!` that only generates the tree structure,
/// without the optimized HTML. Useful for testing and analysis.
///
/// ## Example
///
/// ```rust,ignore
/// use proa_macros::html_tree;
/// // The generated tree types are provided by proa_ir 0.1.2 or newer.
///
/// let tree = html_tree! {
///     <button class={classes}>
///         <Spinner />
///         {children}
///     </button>
/// };
///
/// // Analyze the tree
/// let deps = tree.component_dependencies(); // ["Spinner"]
/// ```
#[proc_macro]
pub fn html_tree(input: TokenStream) -> TokenStream {
    let root = parse_macro_input!(input as HtmlRoot);

    let tree_expanded = tree_codegen::generate_tree(&root);

    TokenStream::from(tree_expanded)
}

/// Derive macro for creating zero-cost const recipe implementations.
///
/// Automatically generates:
/// - `pub const CLASS_CAPACITY: usize` - Maximum number of class strings
/// - `pub const fn classes(...) -> ([&str; CLASS_CAPACITY], usize)` - Compile-time class generation
///
/// ## Requirements
///
/// Each variant enum must implement a `pub const fn classes(self) -> &'static str` method.
/// The variant types and their `classes()` methods are application-defined;
/// the derive does not depend on a particular component library.
///
/// ## Example
///
/// ```rust,ignore
/// // Each enum implements its own classes() method
/// impl ButtonSize {
///     pub const fn classes(self) -> &'static str {
///         match self {
///             ButtonSize::Xs => BUTTON_SIZE_XS,
///             ButtonSize::Sm => BUTTON_SIZE_SM,
///             // ...
///         }
///     }
/// }
///
/// // The macro generates CLASS_CAPACITY and classes() for the recipe
/// #[derive(ProaRecipe)]
/// pub struct ButtonRecipe {
///     base: &'static str,
///     size_default: ButtonSize,
///     color_default: ButtonColor,
/// }
/// ```
///
/// This eliminates the boilerplate of writing manual CLASS_CAPACITY and classes() method!
#[proc_macro_derive(ProaRecipe)]
pub fn derive_proa_recipe(input: TokenStream) -> TokenStream {
    const_recipe::derive_const_recipe(input)
}

/// Derive macro for automatically generating portable component render implementations.
///
/// The generated implementation is generic over both `WriteBuf` and
/// `DataLoader`, so ordinary component authors do not need to write those
/// parameters. Generic children are supported without boxing. Add
/// `async_render` to the struct-level attribute when this sync component
/// should also compose directly inside `html!` without `to_async(...)`. Add
/// `children_builder` when a direct `Option<C>` child should get an inferred,
/// allocation-free `with_children(...)` method. Add `constructor` to generate
/// `new(...)`; fields can then explicitly opt into defaults or named optional
/// builders.
///
/// ## Requirements
///
/// The component provides a `classes` method returning its static class
/// array and the number of occupied entries:
///
/// ```rust,ignore
/// impl<C> Card<C> {
///     fn classes(&self) -> ([&'static str; 1], usize) {
///         (["card"], 1)
///     }
/// }
///
/// let card = Card::new()
///     .with_id("summary")
///     .with_children("Statically dispatched child");
/// ```
///
/// ## Attributes
///
/// - `#[component(tag = "...")]` - HTML tag name (required, struct-level)
/// - `#[component(tag = "...", async_render)]` - also generate an immediate
///   `WebRender` implementation for direct use in `html!`
/// - `#[component(tag = "...", children_builder)]` - generate
///   `with_children<NewChild>(self, child)` for a direct `Option<C>` child
/// - `#[component(tag = "...", constructor)]` - generate `new(...)` from
///   required fields in declaration order
/// - `#[component(attr, attr_name = "name")]` - optional HTML attribute
/// - `#[component(bool_attr, attr_name = "name")]` - boolean HTML attribute
/// - `#[component(class)]` - Custom class field
/// - `#[component(action, attr_name = "name")]` - action data field
/// - `#[component(children)]` - optional generic child
/// - `#[component(default)]` - omit this field from `new(...)` and initialize
///   it through `Default`
/// - `#[component(default = <expr>)]` - omit this field from `new(...)` and
///   use a component-specific initializer
/// - `#[component(optional_builder = "with_name")]` - for an `Option<T>`
///   field, generate the explicitly named consuming builder that accepts `T`
///   and stores `Some(value)`
///
/// ## Example
///
/// ```rust,ignore
/// use proa_macros::ProaComponent;
///
/// #[derive(ProaComponent)]
/// #[component(
///     tag = "section",
///     async_render,
///     constructor,
///     children_builder
/// )]
/// pub struct Card<C = ()> {
///     #[component(
///         attr,
///         attr_name = "id",
///         optional_builder = "with_id"
///     )]
///     pub id: Option<&'static str>,
///     #[component(class, optional_builder = "with_class")]
///     pub class: Option<&'static str>,
///     #[component(children)]
///     pub children: Option<C>,
/// }
///
/// impl<C> Card<C> {
///     fn classes(&self) -> ([&'static str; 1], usize) {
///         (["card"], 1)
///     }
/// }
/// ```
///
/// This generates zero-allocation `WebRenderSync<L, B>` code. With
/// `async_render`, it also generates `WebRender<L, B>` as an immediate
/// `RenderOutcome::Done`; no future or child is boxed. `children_builder`
/// requires exactly one direct `#[component(children)]` field shaped as
/// `Option<C>`. `C` must be an unconstrained type generic used only by that
/// field, so replacing it preserves every other field and generic argument.
/// With both `constructor` and `children_builder`, `new(...)` is generated for
/// the zero-child specialization, making
/// `Card::new().with_children(child)` deterministic even when `C` has no
/// default. Optional builders only add inherent methods: they do not change
/// component layout, render dispatch, or allocation behavior.
#[proc_macro_derive(ProaComponent, attributes(component))]
pub fn derive_proa_component(input: TokenStream) -> TokenStream {
    component_derive::derive_proa_component(input)
}

/// Derive macro for action enums that generates TypeScript bindings and collision detection.
///
/// ## Requirements
///
/// - Must be applied to an enum
/// - Requires `#[action(scope = "...")]` attribute for namespacing
///
/// ## Attributes
///
/// - `#[action(scope = "unique_scope")]` - Required scope for collision prevention
///
/// ## Example
///
/// ```rust,ignore
/// use proa_macros::Action;
///
/// #[derive(Action)]
/// #[action(scope = "home_page")]
/// pub enum ButtonAction {
///     HandleClick,
///     HandleSave { autosave: bool, notify_user: bool },
/// }
///
/// // Generated constants:
/// // ButtonAction::HANDLE_CLICK = "home_page.ButtonAction.HandleClick"
/// // ButtonAction::HANDLE_SAVE = "home_page.ButtonAction.HandleSave"
/// ```
///
/// The macro generates:
/// - Action name constants (HANDLE_CLICK, etc.)
/// - Parameter structs for variants with fields
/// - `ActionHandler` trait implementation
/// - Registry entry for build-time collision detection
#[proc_macro_derive(Action, attributes(action))]
pub fn derive_action(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as syn::DeriveInput);
    action_derive::derive_action(input)
}

/// Derive macro for variant enums to implement VariantSpec.
///
/// This macro makes variant enums the single source of truth by automatically
/// generating metadata for TypeScript code generation.
///
/// ## Attributes
///
/// - `#[variant(name = "...")]` - Required on the enum. Specifies the variant dimension name (e.g., "size", "color")
/// - `path = "..."` - Optional in the enum's `variant` attribute. Overrides the `variant_spec` module path for source-installed components
/// - `#[variant(label = "...")]` - Optional on variants. Overrides the display label
/// - `#[variant(value = "...")]` - Optional on variants. Overrides the TypeScript enum value
///
/// ## Example
///
/// ```rust,ignore
/// use proa_macros::ProaVariant;
///
/// #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, ProaVariant)]
/// #[variant(name = "size")]
/// pub enum ButtonSize {
///     Sm,
///     #[default]
///     Default,
///     Lg,
/// }
///
/// impl ButtonSize {
///     pub const fn classes(self) -> &'static str {
///         match self {
///             ButtonSize::Sm => "h-8",
///             ButtonSize::Md => "h-9",
///             ButtonSize::Lg => "h-10",
///         }
///     }
/// }
/// ```
///
/// This generates an implementation of `VariantSpec` that provides:
/// - Variant name ("size")
/// - TypeScript type name ("ButtonSize")
/// - All variant options with labels, values, and classes
/// - Default variant
#[proc_macro_derive(ProaVariant, attributes(variant))]
pub fn derive_proa_variant(input: TokenStream) -> TokenStream {
    variant_derive::derive_proa_variant(input)
}

/// Deprecated derive macro for the historical `proa_ir::FormSchema` trait.
///
/// Existing callers can keep compiling during migration. New applications
/// should use native forms and validate input in their handler or service
/// layer. Proa does not require or bundle a schema library; typed schema
/// integrations can remain downstream.
///
/// ## Replacement
///
/// ```rust,ignore
/// async fn submit(axum::Form(input): axum::Form<Signup>) -> Response {
///     let input = validate_signup(input)?;
///     save_signup(input).await?;
///     Redirect::to("/welcome").into_response()
/// }
/// ```
#[deprecated(
    since = "0.1.2",
    note = "validate native form input in the application handler or service layer"
)]
#[proc_macro_derive(FormSchema, attributes(form, validate))]
pub fn derive_form_schema(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as syn::DeriveInput);
    form_schema_derive::derive_form_schema(input)
}

/// Attribute macro for declaring typed React islands.
///
/// Reads a TSX file at compile time, extracts the component's props interface,
/// and generates a typed Rust props struct + `IslandDef` static.
///
/// # Basic Usage
///
/// ```rust,ignore
/// use proa_macros::island;
///
/// #[island]
/// pub struct Counter;
/// // expands to:
/// //   #[derive(serde::Serialize)]
/// //   pub struct CounterProps { pub initial_count: f64, pub label: String, ... }
/// //   pub static Counter: IslandDef<CounterProps> = IslandDef::new("Counter");
/// ```
///
/// # Attributes
///
/// - `module = "..."` — Resolve via tsconfig path aliases (e.g. `"@components/Counter"`)
/// - `export = "..."` — Export name if different from the struct name
/// - `path = "..."` — Explicit relative path to the TSX file
/// - `strategy = "..."` — Default hydration strategy (`"immediate"`, `"idle"`, `"visible"`)
///
/// # Example with explicit path
///
/// ```rust,ignore
/// #[island(path = "client/Counter.tsx")]
/// pub struct Counter;
/// ```
///
/// # Example with module
///
/// ```rust,ignore
/// #[island(module = "@components/Forms", export = "TextInput")]
/// pub struct TextInput;
/// ```
#[proc_macro_attribute]
pub fn island(args: TokenStream, input: TokenStream) -> TokenStream {
    match island_attr::island_attr(args.into(), input.into()) {
        Ok(tokens) => tokens.into(),
        Err(err) => err.to_compile_error().into(),
    }
}

/// Experimental explicitly identified sync template for `proa dev --hot-static`.
/// Static text and attribute strings can update without Cargo; dynamic Rust and
/// markup structure retain the ordinary compile/restart path. Without the dev
/// environment this emits the normal consolidated literal fast path.
#[proc_macro]
pub fn html_hot_sync(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as proa_dsl::dev_hot::HotInput);
    let warnings = match static_consolidation::validate_security(&input.root) {
        Err(errors) => return errors.into(),
        Ok(warnings) => warnings,
    };
    let expanded = match std::env::var("PROA_DEV_HOT_STATIC") {
        Ok(path) if std::path::Path::new(&path).is_absolute() => {
            match proa_dsl::dev_hot::compile(&input) {
                Ok(template) => static_consolidation::generate_optimized_hot(
                    &input.root,
                    &path,
                    &input.id.value(),
                    &template.shape,
                    template.slots.len(),
                ),
                Err(error) => {
                    return syn::Error::new(input.id.span(), error)
                        .to_compile_error()
                        .into()
                }
            }
        }
        _ => static_consolidation::generate_optimized(&input.root),
    };
    quote::quote! {{ const _: Option<&str> = ::core::option_env!("PROA_DEV_HOT_STATIC"); #warnings #expanded }}.into()
}
