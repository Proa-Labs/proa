// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! The linked RSJS modules, published under content-hashed names.
//!
//! The runtime and every generated module are prepared once and served from
//! `/_proa/immutable/rsjs/`, cached for as long as a browser will keep them.
//! Their bytes are the same in every deployment: modules import each other by
//! relative path, and the base path is read from the page. A module's name
//! holds a hash of those bytes, which include the names of the modules it
//! imports, so a change anywhere renames everything that reaches it.

use std::{
    borrow::Cow,
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

use axum::{
    http::{header, HeaderMap, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    routing::get,
    Router,
};
use bytes::Bytes;
use oxc_allocator::Allocator;
use oxc_codegen::{Codegen, CodegenOptions, LegalComment};
use oxc_minifier::{CompressOptions, Minifier, MinifierOptions};
use oxc_parser::Parser;
use oxc_span::SourceType;
use proa_assets::{CachePolicy, FINGERPRINT_LENGTH, PUBLIC_ROUTE};
use proa_core::{FrameworkAsset, FrameworkAssets};

use crate::asset_urls::{portable_js, PortableJs};

#[derive(Debug, thiserror::Error)]
#[error("could not prepare RSJS JavaScript `{asset}`: {reason}")]
pub struct RsjsAssetsError {
    asset: String,
    reason: String,
}

/// The setting `cargo proa` compiled this executable with.
fn minify() -> bool {
    matches!(option_env!("PROA_RSJS_MINIFY"), Some("1"))
}

/// Every linked module's source, by canonical path.
struct Sources(BTreeMap<String, Cow<'static, str>>);

impl Sources {
    fn linked(manifest: &rsjs::Manifest) -> Result<Self, RsjsAssetsError> {
        let mut sources = BTreeMap::new();

        let mut add = |logical: String, source: Cow<'static, str>| {
            if sources.insert(logical.clone(), source).is_some() {
                return Err(RsjsAssetsError {
                    asset: logical,
                    reason: "two linked modules share this file name".into(),
                });
            }
            Ok(())
        };
        add(
            rsjs::manifest::RUNTIME_PATH.into(),
            Cow::Borrowed(rsjs::runtime_assets::CORE_JS),
        )?;
        add(
            rsjs::manifest::QUERY_RUNTIME_PATH.into(),
            Cow::Borrowed(rsjs::runtime_assets::QUERY_JS),
        )?;
        for entry in &manifest.islands {
            add(
                rsjs::RsjsAssetRef::Island {
                    asset_id: entry.asset_id,
                }
                .canonical_path(),
                Cow::Borrowed(entry.js_source),
            )?;
        }
        for entry in &manifest.client_modules {
            add(
                rsjs::RsjsAssetRef::ClientModule {
                    module_path: entry.module_path,
                }
                .canonical_path(),
                Cow::Owned(rsjs::client_module_js_source(entry)),
            )?;
        }
        for entry in &manifest.client_classes {
            add(
                rsjs::RsjsAssetRef::ClientClass {
                    module_path: entry.module_path,
                }
                .canonical_path(),
                Cow::Borrowed(entry.js_source),
            )?;
        }
        for entry in &manifest.component_fragments {
            add(
                rsjs::RsjsAssetRef::ComponentFragment {
                    asset_id: entry.asset_id,
                }
                .canonical_path(),
                Cow::Borrowed(entry.js_source),
            )?;
        }

        Ok(Self(sources))
    }

    /// Identifies what would be published from these sources. A bundle
    /// records it, so an executable recognizes the modules exported from it.
    fn digest(&self, minify: bool) -> String {
        let mut input = Vec::new();
        input.extend_from_slice(if minify { b"minified\0" } else { b"verbatim\0" });
        for (name, source) in &self.0 {
            input.extend_from_slice(name.as_bytes());
            input.push(0);
            input.extend_from_slice(source.as_bytes());
            input.push(0);
        }
        proa_assets::sha256(&input)
    }
}

/// Directory of the published modules, below the immutable directory.
const DIRECTORY: &str = "rsjs";

/// The RSJS modules linked into this executable, ready to serve.
///
/// Merge [`router`](Self::router) into a custom router and hand the same
/// value to [`RouteContextLayer::with_rsjs_assets`](crate::RouteContextLayer::with_rsjs_assets),
/// so pages link the names this router serves. `FrameworkBuilder::with_rsjs_assets`
/// does both.
#[derive(Clone)]
pub struct RsjsAssets {
    files: Arc<Vec<RsjsFile>>,
    published: FrameworkAssets,
    /// The files were read from the bundle beside the executable, so they
    /// are also wherever that bundle's `public/` directory was uploaded.
    bundled: bool,
}

/// One published module.
pub struct RsjsFile {
    /// Path from the origin root, with no leading slash, such as
    /// `_proa/immutable/rsjs/runtime.<hash>.js`.
    pub path: String,
    pub bytes: Bytes,
    pub sha256: String,
    /// Subresource Integrity value for `bytes`.
    pub integrity: String,
}

impl RsjsAssets {
    /// Prepare the linked modules using the minification setting selected by
    /// `cargo proa`.
    pub fn prepare() -> Result<Self, RsjsAssetsError> {
        Self::build(&rsjs::manifest(), minify())
    }

    /// The modules `proa build` stored in `bundle`, when they were exported
    /// from this executable. They are read as they are: nothing is parsed,
    /// minified, or hashed at startup. Any other bundle, and a store, gives
    /// the same result as [`prepare`](Self::prepare).
    pub fn load(bundle: Option<&crate::PublicAssets>) -> Result<Self, RsjsAssetsError> {
        let manifest = rsjs::manifest();
        let sources = Sources::linked(&manifest)?;
        if let Some(assets) =
            bundle.and_then(|bundle| Self::from_bundle(bundle.catalog(), &sources.digest(minify())))
        {
            return Ok(assets);
        }
        Self::publish(sources, minify())
    }

    fn from_bundle(catalog: &proa_assets::AssetCatalog, source: &str) -> Option<Self> {
        let manifest = catalog.manifest();
        if manifest.framework.is_empty() {
            return None;
        }
        if manifest.framework_source.as_deref() != Some(source) {
            tracing::warn!(
                "the bundled RSJS modules were exported from another build; preparing this executable's own"
            );
            return None;
        }
        let route = PUBLIC_ROUTE.trim_start_matches('/');
        let mut files = Vec::with_capacity(manifest.framework.len());
        let mut published = BTreeMap::new();
        for (name, entry) in &manifest.framework {
            // The catalog verified these bytes against the manifest.
            let bytes = std::fs::read(catalog.framework_path(name)?).ok()?;
            let path = format!("{route}/{}", entry.path);
            published.insert(
                name.clone(),
                FrameworkAsset {
                    path: Arc::from(path.as_str()),
                    integrity: Arc::from(entry.integrity.as_str()),
                },
            );
            files.push(RsjsFile {
                path,
                bytes: Bytes::from(bytes),
                sha256: entry.sha256.clone(),
                integrity: entry.integrity.clone(),
            });
        }
        Some(Self {
            files: Arc::new(files),
            published: FrameworkAssets::new(published).hosted(true),
            bundled: true,
        })
    }

    fn build(manifest: &rsjs::Manifest, minify: bool) -> Result<Self, RsjsAssetsError> {
        Self::publish(Sources::linked(manifest)?, minify)
    }

    fn publish(sources: Sources, minify: bool) -> Result<Self, RsjsAssetsError> {
        let sources = sources.0;

        let logical: Vec<&str> = sources.keys().map(String::as_str).collect();
        let runtime = logical
            .binary_search(&rsjs::manifest::RUNTIME_PATH)
            .expect("the runtime is always published");
        let mut modules = Vec::with_capacity(sources.len());
        for (index, (name, source)) in sources.iter().enumerate() {
            let fail = |reason: String| RsjsAssetsError {
                asset: name.clone(),
                reason,
            };
            let resolve = |specifier: &str| {
                let target = imported_module(name, specifier)?;
                logical.binary_search(&target.as_str()).ok()
            };
            let portable = portable_js(source, &resolve).map_err(fail)?;
            let mut imports: BTreeSet<usize> =
                portable.imports.iter().map(|(_, module)| *module).collect();
            // The helper is the runtime's `app_url`, so a generated module
            // that needs it imports the runtime.
            let is_runtime = matches!(
                name.as_str(),
                rsjs::manifest::RUNTIME_PATH | rsjs::manifest::QUERY_RUNTIME_PATH
            );
            if portable.helper.is_some() && !is_runtime {
                imports.insert(runtime);
            }
            let imports_itself = imports.remove(&index);
            modules.push(Module {
                logical: name,
                source,
                portable,
                is_runtime,
                imports_itself,
                imports: imports.into_iter().collect(),
            });
        }

        // Tarjan's algorithm yields each group of modules that import one
        // another after every group it imports, which is the order names
        // have to be settled in.
        let mut names: Vec<Option<String>> = vec![None; modules.len()];
        let mut bytes: Vec<Option<String>> = vec![None; modules.len()];
        for group in import_groups(&modules) {
            let render = |module: usize,
                          names: &[Option<String>],
                          group_name: &dyn Fn(usize) -> String| {
                // A module keeps its directory whatever its digest turns out
                // to be, so its own name is not needed to address another.
                let from = published_name(modules[module].logical, "");
                let specifier = |target: usize| {
                    let to = match &names[target] {
                        Some(name) => name.clone(),
                        None => group_name(target),
                    };
                    relative_specifier(&from, &to)
                };
                let item = &modules[module];
                let mut text = item.portable.render(item.source, specifier);
                if let Some(helper) = &item.portable.helper {
                    if item.is_runtime {
                        // Both runtime modules define `app_url` themselves.
                        text.push_str(&format!("\nfunction {helper}(p){{return app_url(p);}}\n"));
                    } else {
                        text.push_str(&format!(
                            "\nimport {{ app_url as {helper} }} from {};\n",
                            serde_json::to_string(&specifier(runtime))
                                .expect("serialize import specifier")
                        ));
                    }
                }
                if minify {
                    minify_module(&text).map_err(|reason| RsjsAssetsError {
                        asset: item.logical.clone(),
                        reason,
                    })
                } else {
                    Ok(text)
                }
            };
            let cyclic = group.len() > 1 || modules[group[0]].imports_itself;
            if !cyclic {
                // The common case: every import already has its final name,
                // so the module's own bytes decide its name.
                let module = group[0];
                let text = render(module, &names, &|_| unreachable!("no cyclic import"))?;
                names[module] = Some(published_name(
                    modules[module].logical,
                    &proa_assets::sha256(text.as_bytes()),
                ));
                bytes[module] = Some(text);
                continue;
            }
            // Modules that import each other cannot each be named after bytes
            // that hold the others' names. They share one digest, taken over
            // the group with its internal imports written canonically.
            let canonical = |module: usize| published_name(modules[module].logical, "");
            let mut digest = String::new();
            for &module in &group {
                digest.push_str(modules[module].logical);
                digest.push('\0');
                digest.push_str(&render(module, &names, &canonical)?);
                digest.push('\0');
            }
            let digest = proa_assets::sha256(digest.as_bytes());
            for &module in &group {
                names[module] = Some(published_name(modules[module].logical, &digest));
            }
            for &module in &group {
                bytes[module] = Some(render(module, &names, &|_| unreachable!("named"))?);
            }
        }

        let mut files = Vec::with_capacity(modules.len());
        let mut published = BTreeMap::new();
        for ((module, name), text) in modules.iter().zip(names).zip(bytes) {
            let text = text.expect("every module was rendered");
            let path = format!(
                "{}/{}/{}",
                PUBLIC_ROUTE.trim_start_matches('/'),
                CachePolicy::Immutable.directory(),
                name.expect("every module was named")
            );
            let integrity = proa_assets::integrity(text.as_bytes());
            published.insert(
                module.logical.clone(),
                FrameworkAsset {
                    path: Arc::from(path.as_str()),
                    integrity: Arc::from(integrity.as_str()),
                },
            );
            files.push(RsjsFile {
                path,
                sha256: proa_assets::sha256(text.as_bytes()),
                integrity,
                bytes: Bytes::from(text),
            });
        }
        Ok(Self {
            files: Arc::new(files),
            published: FrameworkAssets::new(published),
            bundled: false,
        })
    }

    /// Published names by canonical path. Pages rendered through a
    /// [`RouteContextLayer`](crate::RouteContextLayer) given this value link
    /// these names.
    pub fn published(&self) -> &FrameworkAssets {
        &self.published
    }

    /// Whether the modules came from the bundle beside the executable.
    pub fn bundled(&self) -> bool {
        self.bundled
    }

    /// Every published module, for a host that stores them elsewhere.
    pub fn files(&self) -> &[RsjsFile] {
        &self.files
    }

    /// One exact route per module. Incoming paths carry no base path: a
    /// reverse proxy strips it before forwarding.
    pub fn router(&self) -> Router {
        let mut router = Router::new();
        for (index, file) in self.files.iter().enumerate() {
            let files = self.files.clone();
            router = router.route(
                &format!("/{}", file.path),
                get(move |headers: HeaderMap| async move { serve(&files[index], &headers) }),
            );
        }
        router
    }
}

/// Answer the build tool's request for this executable's modules: write each
/// one at its published path, so the build can store them with the public
/// files. See [`rsjs::export`].
#[rsjs::linkme::distributed_slice(rsjs::export::EXPORTERS)]
#[linkme(crate = rsjs::linkme)]
static EXPORT_MODULES: rsjs::export::Exporter = export_modules;

fn export_modules(directory: &std::path::Path) -> Result<rsjs::export::Exported, String> {
    let sources = Sources::linked(&rsjs::manifest()).map_err(|error| error.to_string())?;
    let source = sources.digest(minify());
    let assets = RsjsAssets::publish(sources, minify()).map_err(|error| error.to_string())?;
    let mut index = Vec::with_capacity(assets.files.len());
    for (name, published) in assets.published.iter() {
        let file = assets
            .files
            .iter()
            .find(|file| *file.path == *published.path)
            .expect("every published name has a file");
        let destination = directory.join(&file.path);
        let io = |error: std::io::Error| format!("{}: {error}", destination.display());
        std::fs::create_dir_all(destination.parent().expect("module directory")).map_err(io)?;
        std::fs::write(&destination, &file.bytes).map_err(io)?;
        index.push((name.to_owned(), file.path.clone()));
    }
    Ok(rsjs::export::Exported {
        source,
        files: index,
    })
}

struct Module<'a> {
    logical: &'a String,
    source: &'a str,
    portable: PortableJs,
    is_runtime: bool,
    imports_itself: bool,
    imports: Vec<usize>,
}

/// The canonical path an import specifier in the module at `from` names.
/// Generated modules import the two runtime files from the origin root and
/// each other by relative path.
fn imported_module(from: &str, specifier: &str) -> Option<String> {
    if let Some(path) = specifier.strip_prefix('/') {
        return (!path.starts_with('/')).then(|| path.to_owned());
    }
    if !specifier.starts_with("./") && !specifier.starts_with("../") {
        return None;
    }
    let mut parts: Vec<&str> = from.split('/').collect();
    parts.pop();
    for part in specifier.split('/') {
        match part {
            "." | "" => {}
            ".." => {
                parts.pop()?;
            }
            part => parts.push(part),
        }
    }
    Some(parts.join("/"))
}

/// Where a module is published, below the immutable directory. An empty
/// digest gives the name used while a group of modules is being hashed.
fn published_name(logical: &str, digest: &str) -> String {
    let stem = match logical {
        rsjs::manifest::RUNTIME_PATH => "runtime".to_owned(),
        rsjs::manifest::QUERY_RUNTIME_PATH => "query".to_owned(),
        other => {
            let file = other.rsplit('/').next().unwrap_or(other);
            format!("island/{}", file.strip_suffix(".js").unwrap_or(file))
        }
    };
    let digest = &digest[..digest.len().min(FINGERPRINT_LENGTH)];
    format!("{DIRECTORY}/{stem}.{digest}.js")
}

/// The relative specifier from one published module to another. A relative
/// specifier resolves against whatever origin served the importing module.
fn relative_specifier(from: &str, to: &str) -> String {
    let mut from: Vec<&str> = from.split('/').collect();
    from.pop();
    let to: Vec<&str> = to.split('/').collect();
    let shared = from
        .iter()
        .zip(&to[..to.len() - 1])
        .take_while(|(a, b)| a == b)
        .count();
    let up = from.len() - shared;
    let mut specifier = if up == 0 {
        "./".to_owned()
    } else {
        "../".repeat(up)
    };
    specifier.push_str(&to[shared..].join("/"));
    specifier
}

/// Groups of modules that import one another, each after every group it
/// imports. A module with no cyclic import is a group of one.
fn import_groups(modules: &[Module<'_>]) -> Vec<Vec<usize>> {
    struct State<'a, 'm> {
        modules: &'a [Module<'m>],
        next: usize,
        index: Vec<Option<usize>>,
        low: Vec<usize>,
        stack: Vec<usize>,
        on_stack: Vec<bool>,
        groups: Vec<Vec<usize>>,
    }
    fn visit(state: &mut State<'_, '_>, module: usize) {
        state.index[module] = Some(state.next);
        state.low[module] = state.next;
        state.next += 1;
        state.stack.push(module);
        state.on_stack[module] = true;
        for position in 0..state.modules[module].imports.len() {
            let target = state.modules[module].imports[position];
            match state.index[target] {
                None => {
                    visit(state, target);
                    state.low[module] = state.low[module].min(state.low[target]);
                }
                Some(index) if state.on_stack[target] => {
                    state.low[module] = state.low[module].min(index);
                }
                Some(_) => {}
            }
        }
        if Some(state.low[module]) == state.index[module] {
            let mut group = Vec::new();
            loop {
                let member = state.stack.pop().expect("group member");
                state.on_stack[member] = false;
                group.push(member);
                if member == module {
                    break;
                }
            }
            group.sort_unstable();
            state.groups.push(group);
        }
    }
    let mut state = State {
        modules,
        next: 0,
        index: vec![None; modules.len()],
        low: vec![0; modules.len()],
        stack: Vec::new(),
        on_stack: vec![false; modules.len()],
        groups: Vec::new(),
    };
    for module in 0..modules.len() {
        if state.index[module].is_none() {
            visit(&mut state, module);
        }
    }
    state.groups
}

fn minify_module(source: &str) -> Result<String, String> {
    let allocator = Allocator::default();
    let mut parsed = Parser::new(&allocator, source, SourceType::mjs()).parse();
    if parsed.panicked || !parsed.errors.is_empty() {
        return Err(parsed
            .errors
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("; "));
    }

    // Keep diagnostic names and explicit debugger/console statements. The
    // current Oxc mangler's keep-names pass also panics on runtime patterns.
    Minifier::new(MinifierOptions {
        compress: Some(CompressOptions::safest()),
        mangle: None,
    })
    .build(&allocator, &mut parsed.program);
    let codegen = CodegenOptions {
        legal_comments: LegalComment::Inline,
        ..CodegenOptions::minify()
    };
    Ok(Codegen::new()
        .with_options(codegen)
        .build(&parsed.program)
        .code)
}

fn serve(file: &RsjsFile, headers: &HeaderMap) -> Response {
    let etag = format!("\"{}\"", file.sha256);
    let unchanged = headers
        .get_all(header::IF_NONE_MATCH)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(','))
        .any(|tag| {
            let tag = tag.trim();
            tag == "*" || tag.strip_prefix("W/").unwrap_or(tag) == etag
        });
    let mut response = if unchanged {
        StatusCode::NOT_MODIFIED.into_response()
    } else {
        file.bytes.clone().into_response()
    };
    let headers = response.headers_mut();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/javascript; charset=utf-8"),
    );
    headers.insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static(CachePolicy::Immutable.cache_control()),
    );
    headers.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    headers.insert(header::ETAG, etag.parse().expect("SHA-256 header"));
    response
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        body::{to_bytes, Body},
        http::Request,
    };
    use tower::ServiceExt;

    const fn island(asset_id: &'static str, js_source: &'static str) -> rsjs::IslandManifestEntry {
        rsjs::IslandManifestEntry {
            asset_id,
            struct_name: "Fixture",
            js_source,
            is_async: false,
            hydration_strategy: rsjs::IslandHydrationStrategy::Immediate,
            client_module_imports: &[],
            component_fragment_imports: &[],
            uses_queries: false,
            uses_mutations: false,
        }
    }

    const fn fragment(
        asset_id: &'static str,
        js_source: &'static str,
    ) -> rsjs::ComponentFragmentManifestEntry {
        rsjs::ComponentFragmentManifestEntry {
            asset_id,
            struct_name: "Fixture",
            export_name: "mount",
            js_source,
        }
    }

    static COUNTER: rsjs::IslandManifestEntry = island(
        "counter",
        r#"
            import { signal } from "/rsjs-runtime.js";
            import { mount_row } from "./component_row.js";

            export function hydrate(root) {
                const currentValue = signal(root.dataset.initialValue);
                mount_row(root.querySelector("[data-rsjs-bind]"), currentValue);
                fetch("/api/count");
                fetch("https://api.example/count");
            }
        "#,
    );
    static PLAIN: rsjs::IslandManifestEntry = island(
        "plain",
        r#"import { signal } from "/rsjs-runtime.js"; export const value = signal(1);"#,
    );
    // A row renders cells and a cell renders rows.
    static ROW: rsjs::ComponentFragmentManifestEntry = fragment(
        "row",
        r#"import { mount_cell } from "./component_cell.js"; export function mount_row(node, value) { return mount_cell(node, value); }"#,
    );
    static CELL: rsjs::ComponentFragmentManifestEntry = fragment(
        "cell",
        r#"import { mount_row } from "./component_row.js"; export function mount_cell(node, value) { return value ? node : mount_row(node, value); }"#,
    );

    fn manifest() -> rsjs::Manifest {
        rsjs::Manifest {
            islands: vec![&COUNTER, &PLAIN],
            component_fragments: vec![&CELL, &ROW],
            ..rsjs::Manifest::default()
        }
    }

    fn file<'a>(assets: &'a RsjsAssets, logical: &str) -> &'a RsjsFile {
        let path = &assets.published().get(logical).expect(logical).path;
        assets
            .files()
            .iter()
            .find(|file| *file.path == **path)
            .expect(logical)
    }

    fn text(file: &RsjsFile) -> &str {
        std::str::from_utf8(&file.bytes).unwrap()
    }

    fn name(file: &RsjsFile) -> &str {
        file.path.rsplit('/').next().unwrap()
    }

    #[test]
    fn modules_are_named_after_bytes_that_import_each_other_by_relative_path() {
        let assets = RsjsAssets::build(&manifest(), false).unwrap();
        let runtime = file(&assets, "rsjs-runtime.js");
        let query = file(&assets, "rsjs-runtime-query.js");
        let counter = file(&assets, "rsjs/island/counter.js");
        let row = file(&assets, "rsjs/island/component_row.js");
        let cell = file(&assets, "rsjs/island/component_cell.js");

        for (file, shape) in [
            (runtime, "_proa/immutable/rsjs/runtime."),
            (query, "_proa/immutable/rsjs/query."),
            (counter, "_proa/immutable/rsjs/island/counter."),
            (row, "_proa/immutable/rsjs/island/component_row."),
        ] {
            let digest = file
                .path
                .strip_prefix(shape)
                .and_then(|rest| rest.strip_suffix(".js"))
                .unwrap_or_else(|| panic!("{}", file.path));
            assert_eq!(digest.len(), 16, "{}", file.path);
            assert_eq!(file.integrity, proa_assets::integrity(&file.bytes));
        }
        // A module outside any import cycle is named after its own bytes.
        for file in [runtime, query, counter] {
            assert!(file.path.contains(&file.sha256[..16]), "{}", file.path);
        }

        let source = text(counter);
        assert!(
            source.contains(&format!(r#"from "../{}""#, name(runtime))),
            "{source}"
        );
        assert!(source.contains(&format!(r#"from "./{}""#, name(row))));
        assert!(!source.contains("/rsjs-runtime.js"));
        // An application URL gains the base path in the browser, through the
        // runtime, so no deployment setting is in the file.
        let helper = source
            .split("import { app_url as ")
            .nth(1)
            .and_then(|rest| rest.split(' ').next())
            .expect("the runtime helper is imported");
        assert!(source.contains(&format!(r#"fetch({helper}("/api/count"))"#)));
        assert!(source.contains(r#"fetch("https://api.example/count")"#));

        // Modules that import each other share one digest and still reach
        // each other by their published names.
        let digest = |file: &RsjsFile| name(file).rsplit('.').nth(1).unwrap().to_owned();
        assert_eq!(digest(row), digest(cell));
        assert!(text(row).contains(&format!(r#"from "./{}""#, name(cell))));
        assert!(text(cell).contains(&format!(r#"from "./{}""#, name(row))));

        // The same input gives the same names, in this process or another.
        let again = RsjsAssets::build(&manifest(), false).unwrap();
        for (first, second) in assets.files().iter().zip(again.files()) {
            assert_eq!(first.path, second.path);
            assert_eq!(first.bytes, second.bytes);
        }
    }

    #[test]
    fn a_change_renames_every_module_that_reaches_it() {
        static CHANGED: rsjs::ComponentFragmentManifestEntry = fragment(
            "cell",
            r#"import { mount_row } from "./component_row.js"; export function mount_cell(node, value) { return value ? null : mount_row(node, value); }"#,
        );
        let before = RsjsAssets::build(&manifest(), false).unwrap();
        let mut changed = manifest();
        changed.component_fragments = vec![&CHANGED, &ROW];
        let after = RsjsAssets::build(&changed, false).unwrap();
        let path = |assets: &RsjsAssets, logical: &str| file(assets, logical).path.clone();
        for renamed in [
            "rsjs/island/component_cell.js",
            "rsjs/island/component_row.js",
            "rsjs/island/counter.js",
        ] {
            assert_ne!(path(&before, renamed), path(&after, renamed), "{renamed}");
        }
        for kept in [
            "rsjs-runtime.js",
            "rsjs-runtime-query.js",
            "rsjs/island/plain.js",
        ] {
            assert_eq!(path(&before, kept), path(&after, kept), "{kept}");
        }
    }

    #[test]
    fn minified_modules_stay_valid_and_keep_their_imports() {
        let plain = RsjsAssets::build(&manifest(), false).unwrap();
        let minified = RsjsAssets::build(&manifest(), true).unwrap();
        for logical in [
            "rsjs-runtime.js",
            "rsjs-runtime-query.js",
            "rsjs/island/counter.js",
        ] {
            let compact = text(file(&minified, logical));
            assert!(
                compact.len() < text(file(&plain, logical)).len(),
                "{logical}"
            );
            let allocator = Allocator::default();
            let parsed = Parser::new(&allocator, compact, SourceType::mjs()).parse();
            assert!(parsed.errors.is_empty(), "{logical}: {:?}", parsed.errors);
        }
        let counter = text(file(&minified, "rsjs/island/counter.js"));
        assert!(counter.contains(name(file(&minified, "rsjs-runtime.js"))));
        assert!(counter.contains(name(file(&minified, "rsjs/island/component_row.js"))));
        assert!(counter.contains("[data-rsjs-bind]"));
        assert!(text(file(&minified, "rsjs-runtime.js")).contains("export function app_url("));
    }

    #[test]
    fn modules_that_share_a_file_name_are_refused() {
        static SHADOW: rsjs::ComponentFragmentManifestEntry = fragment("x", "export {};");
        static ISLAND: rsjs::IslandManifestEntry = island("component_x", "export {};");
        let manifest = rsjs::Manifest {
            islands: vec![&ISLAND],
            component_fragments: vec![&SHADOW],
            ..rsjs::Manifest::default()
        };
        assert!(RsjsAssets::build(&manifest, false).is_err());
    }

    static HOSTED: rsjs::IslandManifestEntry = island(
        "hosted_fixture",
        r#"import { signal } from "/rsjs-runtime.js"; export const value = signal(1);"#,
    );

    #[rsjs::linkme::distributed_slice(rsjs::ISLAND_REGISTRY)]
    #[linkme(crate = rsjs::linkme)]
    static REGISTER_HOSTED: &'static rsjs::IslandManifestEntry = &HOSTED;

    /// A bundle as `proa build` writes it for this test executable.
    fn bundle(directory: &std::path::Path, source: &str) -> std::path::PathBuf {
        let public = directory.join("public");
        std::fs::create_dir_all(public.join("images")).unwrap();
        std::fs::write(public.join("images/a.png"), b"image").unwrap();
        std::fs::write(public.join("styles.css"), b"a { color: red }").unwrap();
        let linked = RsjsAssets::prepare().unwrap();
        let files: Vec<_> = linked
            .published()
            .iter()
            .map(|(name, published)| {
                let file = linked
                    .files()
                    .iter()
                    .find(|file| *file.path == *published.path)
                    .unwrap();
                proa_assets::build::FrameworkFile {
                    name,
                    path: file.path.strip_prefix("_proa/").unwrap(),
                    bytes: &file.bytes,
                }
            })
            .collect();
        let output = directory.join("app.assets");
        proa_assets::build::export_with_framework_source(
            &output,
            Some(&public),
            &files,
            Some(source),
        )
        .unwrap();
        output
    }

    #[test]
    fn modules_stored_in_the_bundle_are_used_only_by_the_executable_they_came_from() {
        let temp = tempfile::tempdir().unwrap();
        let own = Sources::linked(&rsjs::manifest()).unwrap().digest(minify());
        let base = proa_core::BasePath::default();

        let matching = crate::PublicAssets::load(bundle(temp.path(), &own), &base).unwrap();
        let loaded = RsjsAssets::load(Some(&matching)).unwrap();
        assert!(loaded.bundled());
        assert!(loaded.published().is_hosted());
        let prepared = RsjsAssets::prepare().unwrap();
        for (stored, own) in loaded.files().iter().zip(prepared.files()) {
            assert_eq!(stored.path, own.path);
            assert_eq!(stored.bytes, own.bytes);
        }
        drop(matching);

        // Files from another build are not this executable's modules.
        let other = crate::PublicAssets::load(bundle(temp.path(), "another build"), &base).unwrap();
        let fallback = RsjsAssets::load(Some(&other)).unwrap();
        assert!(!fallback.bundled());
        assert!(!fallback.published().is_hosted());
        assert!(!RsjsAssets::load(None).unwrap().bundled());
    }

    #[tokio::test]
    async fn a_page_links_hosted_files_and_its_policy_allows_their_origin() {
        use crate::{AssetPrefix, FrameworkBuilder, PublicAssets, RouteResponse};
        use proa_core::{WebContext, WebRenderSync, WriteError};
        use proa_macros::html_sync;

        struct Page;
        impl WebRenderSync for Page {
            fn render(self, cx: &mut WebContext) -> Result<(), WriteError> {
                cx.rsjs_island_used("hosted_fixture");
                html_sync! {
                    <link rel="stylesheet" href={cx.url("public/styles.css")} />
                    <img src={cx.url("public/images/a.png")} alt="" />
                    <a href={cx.url("/pricing")}>"Pricing"</a>
                }
                .render(cx)
            }
        }

        let temp = tempfile::tempdir().unwrap();
        let own = Sources::linked(&rsjs::manifest()).unwrap().digest(minify());
        let output = bundle(temp.path(), &own);
        let base = proa_core::BasePath::new("/shop").unwrap();
        let page = |prefix: Option<&str>| {
            let prefix = prefix.map(|prefix| AssetPrefix::new(prefix).unwrap());
            let assets = PublicAssets::load_hosted(&output, &base, prefix.as_ref()).unwrap();
            let app = FrameworkBuilder::new()
                .with_base_path(base.clone())
                .with_public_assets(assets)
                .with_rsjs_assets()
                .with_strict_csp()
                .with_dynamic_routes(
                    Router::new().route("/", get(|| async { RouteResponse::ssr(Page) })),
                )
                .build();
            async move {
                let response = app
                    .oneshot(Request::builder().uri("/").body(Body::empty()).unwrap())
                    .await
                    .unwrap();
                let policy = response.headers()[header::CONTENT_SECURITY_POLICY]
                    .to_str()
                    .unwrap()
                    .to_owned();
                let body = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
                (policy, String::from_utf8(body.to_vec()).unwrap())
            }
        };

        // Served by this server: everything is under the base path, and the
        // policy needs no host.
        let (policy, html) = page(None).await;
        assert!(
            html.contains(r#"href="/shop/_proa/immutable/styles."#),
            "{html}"
        );
        assert!(html.contains(r#"src="/shop/_proa/immutable/rsjs/runtime."#));
        assert!(!html.contains("crossorigin"));
        assert!(!policy.contains("cdn.example"), "{policy}");

        // Hosted elsewhere: published files move, application routes do not.
        let (policy, html) = page(Some("https://cdn.example/site/")).await;
        for hosted in [
            r#"href="https://cdn.example/site/_proa/immutable/styles."#,
            r#"src="https://cdn.example/site/_proa/immutable/images/a."#,
            r#"<script type="module" src="https://cdn.example/site/_proa/immutable/rsjs/runtime."#,
            r#"<link rel="modulepreload" href="https://cdn.example/site/_proa/immutable/rsjs/island/hosted_fixture."#,
        ] {
            assert!(html.contains(hosted), "missing {hosted} in {html}");
        }
        assert!(html.contains(r#"href="/shop/pricing""#));
        assert!(!html.contains(r#""/shop/_proa/"#), "{html}");
        // The runtime still learns the base path from the page.
        assert!(html.contains(r#"{"basePath":"/shop"}"#));
        // Every tag that fetches a module names its bytes, opts into CORS,
        // and carries the request nonce.
        let nonce = policy
            .split("'nonce-")
            .nth(1)
            .and_then(|rest| rest.split('\'').next())
            .expect("a nonce in the policy");
        let tags: Vec<&str> = html
            .split('<')
            .filter(|tag| tag.contains("/_proa/immutable/rsjs/"))
            .collect();
        assert!(tags.len() >= 3, "{html}");
        for tag in tags {
            assert!(tag.contains(r#" integrity="sha384-"#), "{tag}");
            assert!(tag.contains(" crossorigin"), "{tag}");
            assert!(tag.contains(&format!(r#" nonce="{nonce}""#)), "{tag}");
        }
        // The policy names the host for everything an allowlist governs, and
        // leaves scripts to the nonce.
        for directive in [
            "style-src 'self' 'unsafe-inline' https://cdn.example",
            "img-src 'self' https://cdn.example",
            "font-src 'self' https://cdn.example",
            "media-src 'self' https://cdn.example",
            "connect-src 'self' https://cdn.example",
            &format!("script-src 'nonce-{nonce}' 'strict-dynamic'"),
        ] {
            assert!(
                policy.split("; ").any(|part| part == directive),
                "missing `{directive}` in `{policy}`"
            );
        }
        assert!(policy.contains("default-src 'self'"));
    }

    #[tokio::test]
    async fn each_module_is_served_at_its_one_name_and_cached_forever() {
        let assets = RsjsAssets::build(&manifest(), true).unwrap();
        let router = assets.router();
        let get = |path: String, etag: Option<String>| {
            let mut request = Request::builder().uri(path);
            if let Some(etag) = etag {
                request = request.header(header::IF_NONE_MATCH, etag);
            }
            router.clone().oneshot(request.body(Body::empty()).unwrap())
        };
        let counter = file(&assets, "rsjs/island/counter.js");
        // The runtime adds a query to a module it loads on a trigger.
        for query in ["", "?rsjs-triggered=1"] {
            let response = get(format!("/{}{query}", counter.path), None)
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::OK);
            assert_eq!(
                response.headers()[header::CACHE_CONTROL],
                "public, max-age=31536000, immutable"
            );
            assert_eq!(
                response.headers()[header::CONTENT_TYPE],
                "application/javascript; charset=utf-8"
            );
            let etag = response.headers()[header::ETAG]
                .to_str()
                .unwrap()
                .to_owned();
            let body = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
            assert_eq!(body, counter.bytes);
            let unchanged = get(format!("/{}", counter.path), Some(etag)).await.unwrap();
            assert_eq!(unchanged.status(), StatusCode::NOT_MODIFIED);
        }
        for gone in [
            "/rsjs-runtime.js",
            "/rsjs/island/counter.js",
            "/_proa/immutable/rsjs/island/counter.js",
        ] {
            assert_eq!(
                get(gone.into(), None).await.unwrap().status(),
                StatusCode::NOT_FOUND,
                "{gone}"
            );
        }
    }
}
