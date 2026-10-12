// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

pub fn app_state(has_database: bool) -> String {
    if has_database {
        r#"use std::sync::Arc;

#[derive(Clone)]
pub struct AppState {
    pub db: sqlx::SqlitePool,
    pub loader: Arc<crate::loader::AppLoader>,
}
"#
        .to_string()
    } else {
        "#[derive(Clone, Default)]\npub struct AppState;\n".to_string()
    }
}

pub fn config_rs(package: &str, has_database: bool) -> String {
    let database_field = if has_database {
        "    pub database_url: String,\n"
    } else {
        ""
    };
    let database_load = if has_database {
        r#"        let database_url = std::env::var("DATABASE_URL")
            .unwrap_or_else(|_| "__DATABASE_URL__".to_string());
"#
    } else {
        ""
    };
    let initializer = if has_database {
        "Self { address: SocketAddr::new(host, port), database_url }"
    } else {
        "Self { address: SocketAddr::new(host, port) }"
    };
    r#"use std::net::{IpAddr, SocketAddr};

pub struct Config {
    pub address: SocketAddr,
__DATABASE_FIELD__
}

impl Config {
    pub fn from_env() -> anyhow::Result<Self> {
        dotenvy::dotenv().ok();
        let host: IpAddr = std::env::var("PROA_HOST")
            .unwrap_or_else(|_| "127.0.0.1".to_string())
            .parse()?;
        let port = std::env::var("PORT")
            .unwrap_or_else(|_| "3000".to_string())
            .parse()?;
__DATABASE_LOAD__        Ok(__INITIALIZER__)
    }
}
"#
    .replace("__DATABASE_FIELD__", database_field.trim_end())
    .replace("__DATABASE_LOAD__", database_load)
    .replace("__INITIALIZER__", initializer)
    .replace("__DATABASE_URL__", &format!("sqlite://data/{package}.db"))
}

pub fn db_rs() -> &'static str {
    r#"use std::{str::FromStr, time::Duration};

use sqlx::{
    migrate::Migrator,
    sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions},
    SqlitePool,
};

static MIGRATOR: Migrator = sqlx::migrate!();

pub async fn connect(database_url: &str) -> anyhow::Result<SqlitePool> {
    let options = SqliteConnectOptions::from_str(database_url)?
        .create_if_missing(true)
        .foreign_keys(true)
        .journal_mode(SqliteJournalMode::Wal)
        .busy_timeout(Duration::from_secs(5));
    let pool = SqlitePoolOptions::new()
        .max_connections(5)
        .connect_with(options)
        .await?;
    MIGRATOR.run(&pool).await?;
    Ok(pool)
}
"#
}

pub fn loader_rs() -> &'static str {
    r#"use std::{future::Future, sync::atomic::{AtomicUsize, Ordering}};

use proa_core::{DataLoader, LoadError, LoadKey, LoadRequest, LoadValue};
use sqlx::SqlitePool;

#[derive(Debug)]
pub struct DatabaseStatus {
    pub driver: &'static str,
    pub version: String,
}

pub fn database_status_key() -> LoadKey {
    LoadKey::named("system", "database-status")
}

pub struct AppLoader {
    pub db: SqlitePool,
    origin_loads: AtomicUsize,
}

impl AppLoader {
    pub fn new(db: SqlitePool) -> Self {
        Self { db, origin_loads: AtomicUsize::new(0) }
    }
}

impl DataLoader for AppLoader {
    fn load(
        &self,
        request: &LoadRequest,
    ) -> impl Future<Output = Result<LoadValue, LoadError>> + Send {
        let db = self.db.clone();
        let key = request.key().to_string();
        self.origin_loads.fetch_add(1, Ordering::Relaxed);
        async move {
            match key.as_str() {
                "system:database-status" => {
                    let version = sqlx::query_scalar::<_, String>("SELECT sqlite_version()")
                        .fetch_one(&db)
                        .await
                        .map_err(|error| LoadError::custom(error.to_string()))?;
                    Ok(LoadValue::typed(DatabaseStatus { driver: "SQLite", version }))
                }
                _ => Err(LoadError::custom(format!("unknown load key: {key}"))),
            }
        }
    }
}
"#
}

pub fn error_rs() -> &'static str {
    r#"use axum::{http::StatusCode, response::{IntoResponse, Response}};

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("render failed: {0}")]
    Render(#[from] proa_core::WriteError),
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        tracing::error!(error = %self, "request failed");
        (StatusCode::INTERNAL_SERVER_ERROR, "request failed").into_response()
    }
}
"#
}

pub fn root_layout() -> &'static str {
    r#"// proa:document-root
use std::future::Future;

use proa_core::{raw_text_ok, DataLoader, RenderOutcome, WebContext, WebRender, WriteError};
use proa_macros::html;

pub struct RootDocument<C> {
    pub title: &'static str,
    pub content: C,
}

impl<L, C> WebRender<L> for RootDocument<C>
where
    L: DataLoader,
    C: WebRender<L> + Send + 'static,
{
    fn render(
        self,
        cx: &mut WebContext<L>,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>> {
        RenderOutcome::pending(async move {
            let RootDocument { title, content } = self;
            html! {
                <!DOCTYPE html>
                <html lang="en">
                    <head>
                        <meta charset="utf-8">
                        <meta name="viewport" content="width=device-width, initial-scale=1">
                        <meta name="color-scheme" content="dark">
                        <title>{raw_text_ok(title)}</title>
                        <link rel="icon" href="/static/favicon.svg">
                        <link rel="stylesheet" href="/static/styles.css">
                    </head>
                    <body>{content}</body>
                </html>
            }
            .render(cx)
            .resolve()
            .await
        })
    }
}
"#
}

pub fn database_badge_rs() -> &'static str {
    r#"use std::future::Future;

use proa_core::{DataLoader, RenderOutcome, WebContext, WebRender, WebRenderSync, WriteError};
use proa_macros::html_sync;

use crate::loader::{database_status_key, DatabaseStatus};

#[derive(Debug, Clone, Copy)]
pub struct DatabaseBadge;

impl<L: DataLoader> WebRender<L> for DatabaseBadge {
    fn render(
        self,
        cx: &mut WebContext<L>,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>> {
        RenderOutcome::pending(async move {
            let status = cx.load_typed::<DatabaseStatus>(database_status_key()).await?;
            WebRenderSync::render(html_sync! {
                <span data-database-badge data-driver={status.driver} class="status-pill">
                    <span class="status-dot"></span>
                    {status.driver} " "
                    {status.version.as_str()}
                </span>
            }, cx)
        })
    }
}

impl<L: DataLoader> proa_core::MdRender<L> for DatabaseBadge {
    fn render_md(self, cx: &mut WebContext<L>) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>> {
        RenderOutcome::pending(async move {
            let status = cx.load_typed::<DatabaseStatus>(database_status_key()).await?;
            proa_core::MdRenderSync::render_md(proa_macros::md_sync! {
                {status.driver} " "
                {status.version.as_str()}
            }, cx)
        })
    }
}
"#
}

pub fn database_card_rs() -> &'static str {
    r###"use std::future::Future;

use proa_core::{DataLoader, RenderOutcome, WebContext, WebRender, WebRenderSync, WriteError};
use proa_macros::html_sync;

use crate::loader::{database_status_key, DatabaseStatus};

#[derive(Debug, Clone, Copy)]
pub struct DatabaseCard;

impl<L: DataLoader> WebRender<L> for DatabaseCard {
    fn render(
        self,
        cx: &mut WebContext<L>,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>> {
        RenderOutcome::pending(async move {
            let status = cx.load_typed::<DatabaseStatus>(database_status_key()).await?;
            WebRenderSync::render(html_sync! {
                <article data-database-card class="metric-card">
                    <p class="metric-label">"DATA ORIGIN"</p>
                    <p class="metric-value">{status.driver}</p>
                    <p class="metric-note">"Two component requests resolve through one keyed origin query."</p>
                </article>
            }, cx)
        })
    }
}

impl<L: DataLoader> proa_core::MdRender<L> for DatabaseCard {
    fn render_md(self, cx: &mut WebContext<L>) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>> {
        RenderOutcome::pending(async move {
            let status = cx.load_typed::<DatabaseStatus>(database_status_key()).await?;
            proa_core::MdRenderSync::render_md(proa_macros::md_sync! {
                "## " "DATA ORIGIN"
                "\n\n" {status.driver}
                "\n\n" "Two component requests resolve through one keyed origin query."
            }, cx)
        })
    }
}
"###
}

pub fn home_rs(has_database: bool, has_ui: bool, markdown: bool, blank: bool) -> String {
    let imports = match (has_database, has_ui) {
        (true, true) => {
            "use crate::components::{Badge, Button, Card, DatabaseBadge, DatabaseCard};"
        }
        (true, false) => "use crate::components::{DatabaseBadge, DatabaseCard};",
        (false, true) => "use crate::components::{Badge, Button, Card};",
        (false, false) => "",
    };
    let html_macro = if has_database { "html" } else { "html_sync" };
    let body = if blank {
        "__HTML_MACRO__! { <main class=\"blank-page\"><h1>\"Ready.\"</h1></main> }.render(cx)"
            .replace("__HTML_MACRO__", html_macro)
    } else {
        let badge = if has_ui {
            if has_database {
                "{proa_core::to_async(Badge::new(\"SERVER RENDERED IN RUST\"))}"
            } else {
                "{Badge::new(\"SERVER RENDERED IN RUST\")}"
            }
        } else {
            "<span class=\"status-pill\">\"SERVER RENDERED IN RUST\"</span>"
        };
        let database_badge = if has_database {
            "{DatabaseBadge}"
        } else {
            "<span class=\"status-pill\">\"NO DATABASE SELECTED\"</span>"
        };
        let card = if has_ui {
            if has_database {
                r#"{proa_core::to_async(Card { eyebrow: "REPRESENTATIONS", title: "HTML + Markdown", body: "One typed page serves browsers and agents without duplicating route policy.", class: None })}"#
            } else {
                r#"{Card { eyebrow: "REPRESENTATIONS", title: "HTML + Markdown", body: "One typed page serves browsers and agents without duplicating route policy.", class: None }}"#
            }
        } else {
            r#"<article class="metric-card"><h2>"HTML from Rust"</h2></article>"#
        };
        let db_card = if has_database {
            "{DatabaseCard}"
        } else {
            r#"<article class="metric-card"><p class="metric-label">"DATA"</p><p class="metric-value">"NONE"</p></article>"#
        };
        let button = if has_ui {
            if has_database {
                r#"{proa_core::to_async(Button { label: "READ THE MARKDOWN", href: Some("/index.md"), class: None })}"#
            } else {
                r#"{Button { label: "READ THE MARKDOWN", href: Some("/index.md"), class: None }}"#
            }
        } else {
            "<a href=\"/index.md\">\"READ THE MARKDOWN\"</a>"
        };
        r#"__HTML_MACRO__! {
            <main>
                <header class="topbar"><a class="wordmark" href="/">"PROA"</a><span>"COMPILED APPLICATION STARTER"</span></header>
                <section class="hero">
                    <p class="kicker">"THE SERVER ALREADY DID THE WORK"</p>
                    <h1>"Build the page in Rust. Ship only what moves."</h1>
                    <p class="lede">"Typed SSR, request-local data loading, semantic Markdown, and source-owned components in one production-shaped application."</p>
                    <div class="badges">__BADGE____DATABASE_BADGE__</div>
                    <div class="hero-action">__BUTTON__</div>
                </section>
                <section class="metrics">
                    <article class="metric-card"><p class="metric-label">"CLIENT"</p><p class="metric-value">"0 B"</p><p class="metric-note">"No client root on this route."</p></article>
                    __DB_CARD__
                    __CARD__
                </section>
                <footer class="receipt"><span>"2 component requests → 1 origin query"</span><span>"HTML + semantic Markdown"</span><strong>"Rendered by Proa"</strong></footer>
            </main>
        }
        .render(cx)"#
            .replace("__BADGE__", badge)
            .replace("__DATABASE_BADGE__", database_badge)
            .replace("__BUTTON__", button)
            .replace("__DB_CARD__", db_card)
            .replace("__CARD__", card)
            .replace("__HTML_MACRO__", html_macro)
    };
    let copy = if markdown {
        starter_markdown_copy(&body)
    } else {
        String::new()
    };
    let markdown_impl = if markdown {
        home_markdown_impl(has_database, has_ui, blank, &copy)
    } else {
        String::new()
    };
    let (core_imports, macro_import, render_impl) = if has_database {
        (
            "use std::future::Future;\n\nuse proa_core::{DataLoader, RenderOutcome, WebContext, WebRender, WriteError};",
            "use proa_macros::html;",
            r#"impl<L: DataLoader> WebRender<L> for HomePage {
    fn render(self, cx: &mut WebContext<L>) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>> {
        __BODY__
    }
}"#,
        )
    } else {
        (
            "use proa_core::{DataLoader, WebContext, WebRenderSync, WriteError};",
            "use proa_macros::html_sync;",
            r#"impl<L: DataLoader> WebRenderSync<L> for HomePage {
    fn render(self, cx: &mut WebContext<L>) -> Result<(), WriteError> {
        __BODY__
    }
}"#,
        )
    };
    r#"__CORE_IMPORTS__
__MACRO_IMPORT__
__MD_IMPORTS__
__COMPONENT_IMPORTS__

#[derive(Debug, Clone, Copy)]
pub struct HomePage;

__RENDER_IMPL__
__MARKDOWN_IMPL__
"#
    .replace("__CORE_IMPORTS__", core_imports)
    .replace("__MACRO_IMPORT__", macro_import)
    .replace(
        "__MD_IMPORTS__",
        if markdown {
            if has_database {
                "use proa_core::MdRender;\nuse proa_macros::md;"
            } else {
                "use proa_core::{MdRenderSync, WriteBuf};\nuse proa_macros::md_sync;"
            }
        } else {
            ""
        },
    )
    .replace("__COMPONENT_IMPORTS__", imports)
    .replace("__RENDER_IMPL__", &render_impl.replace("__BODY__", &body))
    .replace("__MARKDOWN_IMPL__", &markdown_impl)
}

fn starter_markdown_copy(body: &str) -> String {
    use proa_dsl::parse::{Attribute, Node};
    fn collect(nodes: &[Node], values: &mut Vec<syn::LitStr>) {
        for node in nodes {
            match node {
                Node::Text(value) if value.value().chars().any(char::is_alphanumeric) => {
                    values.push(value.clone());
                }
                Node::Element(element) => {
                    for attribute in &element.attrs {
                        if let Attribute::Static { name, value } = attribute {
                            if matches!(name.to_string().as_str(), "href" | "src" | "alt" | "title")
                            {
                                values.push(value.clone());
                            }
                        }
                    }
                    collect(&element.children, values);
                }
                Node::Fragment(children) => collect(children, values),
                _ => {}
            }
        }
    }
    let invocation = crate::source::find_macros(body).remove(0);
    let root = crate::lint::parse_invocation(body, &invocation).expect("valid starter HTML");
    let mut values = Vec::new();
    collect(&root.nodes, &mut values);
    let mut markdown = String::new();
    for value in values {
        // Emit literal Markdown directly. Escape inline/list punctuation and
        // double braces for the raw-template slot grammar.
        for character in value.value().chars() {
            match character {
                '{' => markdown.push_str("{{"),
                '}' => markdown.push_str("}}"),
                '\\' | '`' | '*' | '_' | '[' | ']' | '(' | ')' | '<' | '>' | '|' | '~' | '&'
                | '#' | '+' | '-' | '.' | '!' | '=' | ':' => {
                    markdown.push('\\');
                    markdown.push(character);
                }
                '\r' | '\n' | '\t' => markdown.push(' '),
                _ => markdown.push(character),
            }
        }
        markdown.push_str("\n\n");
    }
    markdown
}

fn home_markdown_impl(has_database: bool, has_ui: bool, blank: bool, copy: &str) -> String {
    let mut children = copy.to_string();
    let lift = |child: &str| {
        if has_database {
            format!("{{proa_core::to_md_async({child})}}")
        } else {
            format!("{{{child}}}")
        }
    };
    if !blank {
        if has_ui {
            children.push_str(&lift("Badge::new(\"SERVER RENDERED IN RUST\")"));
            children.push_str("\n\n");
            children.push_str(&lift(
                r#"Button { label: "READ THE MARKDOWN", href: Some("/index.md"), class: None }"#,
            ));
            children.push_str("\n\n");
        }
        if has_database {
            children.push_str("{DatabaseBadge}\n\n{DatabaseCard}\n\n");
        }
        if has_ui {
            children.push_str(&lift(r#"Card { eyebrow: "REPRESENTATIONS", title: "HTML + Markdown", body: "One typed page serves browsers and agents without duplicating route policy.", class: None }"#));
        }
    }
    let implementation = if has_database {
        "impl<L: DataLoader> MdRender<L> for HomePage"
    } else {
        "impl<L: DataLoader, B: WriteBuf> MdRenderSync<L, B> for HomePage"
    };
    let signature = if has_database {
        "fn render_md(self, cx: &mut WebContext<L>) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>>"
    } else {
        "fn render_md(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError>"
    };
    r####"
__IMPL__ {
    __SIGNATURE__ {
        __MACRO__! { r###"
            __CHILDREN__
            "###
        }
        .render_md(cx)
    }
}
"####
        .replace("__IMPL__", implementation)
        .replace("__SIGNATURE__", signature)
        .replace("__MACRO__", if has_database { "md" } else { "md_sync" })
        .replace("__CHILDREN__", &children.replace('\n', "\n            "))
}

pub fn home_handler(has_database: bool, markdown: bool, title: &str) -> String {
    let args = match (has_database, markdown) {
        (true, true) => "State(state): State<AppState>, headers: HeaderMap",
        (true, false) => "State(state): State<AppState>",
        (false, true) => "headers: HeaderMap",
        (false, false) => "",
    };
    let html = if has_database {
        r#"let resolver = DataResolver::request_scope(state.loader.clone());
    let html = RouteResponse::ssr_async_with_loader(
        RootDocument { title: __TITLE__, content: HomePage },
        resolver.clone(),
    );"#
    } else {
        r#"let html = RouteResponse::ssr_async(
        RootDocument { title: __TITLE__, content: proa_core::to_async(HomePage) },
    );"#
    };
    let response = if markdown {
        if has_database {
            r#"Ok(negotiate_async_with_loader(&headers, html, HomePage, resolver, MarkdownAlternate::new("/index.md")))"#
        } else {
            r#"Ok(negotiate(&headers, html, HomePage, MarkdownAlternate::new("/index.md"))?)"#
        }
    } else {
        "Ok(html)"
    };
    let md_handler = if markdown {
        if has_database {
            r#"
pub async fn markdown(State(state): State<AppState>) -> Result<impl IntoResponse, AppError> {
    let resolver = DataResolver::request_scope(state.loader.clone());
    Ok(explicit_markdown_async_with_loader(HomePage, resolver))
}
"#
        } else {
            r#"
pub async fn markdown() -> Result<impl IntoResponse, AppError> {
    Ok(explicit_markdown(HomePage)?)
}
"#
        }
    } else {
        ""
    };
    let axum_import = match (has_database, markdown) {
        (true, true) => "use axum::{extract::State, http::HeaderMap, response::IntoResponse};",
        (true, false) => "use axum::{extract::State, response::IntoResponse};",
        (false, true) => "use axum::{http::HeaderMap, response::IntoResponse};",
        (false, false) => "use axum::response::IntoResponse;",
    };
    let crate_import = if has_database {
        "use crate::{app::AppState, error::AppError, layouts::RootDocument};"
    } else {
        "use crate::{error::AppError, layouts::RootDocument};"
    };
    r#"__AXUM_IMPORT__
__FRAMEWORK_IMPORT__
__RESOLVER_IMPORT__

__CRATE_IMPORT__

pub async fn handler(__ARGS__) -> Result<impl IntoResponse, AppError> {
    __HTML__
    __RESPONSE__
}
__MD_HANDLER__
"#
    .replace("__AXUM_IMPORT__", axum_import)
    .replace(
        "__FRAMEWORK_IMPORT__",
        if markdown {
            if has_database {
                "use proa_framework_axum::{explicit_markdown_async_with_loader, negotiate_async_with_loader, MarkdownAlternate, RouteResponse};"
            } else {
                "use proa_framework_axum::{explicit_markdown, negotiate, MarkdownAlternate, RouteResponse};"
            }
        } else {
            "use proa_framework_axum::RouteResponse;"
        },
    )
    .replace(
        "__RESOLVER_IMPORT__",
        if has_database {
            "use proa_stream::DataResolver;"
        } else {
            ""
        },
    )
    .replace("__CRATE_IMPORT__", crate_import)
    .replace("__ARGS__", args)
    .replace(
        "__HTML__",
        &html.replace("__TITLE__", &super::rust_string_literal(title)),
    )
    .replace("__RESPONSE__", response)
    .replace("__MD_HANDLER__", md_handler)
}

pub fn routes_rs(markdown: bool) -> String {
    let md_route = if markdown {
        ".route(\"/index.md\", get(pages::home::markdown))"
    } else {
        ""
    };
    r#"use std::{collections::HashMap, sync::Arc};

use axum::{routing::get, Router};
use proa_framework_axum::{router_from_spec, IslandManifest, Route, RouteContextLayer};
use tower_http::trace::TraceLayer;

use crate::{app::AppState, endpoints, pages, static_assets};

pub fn router(state: AppState) -> Router {
    let (router, metadata) = router_from_spec(vec![
        // proa:route-specs:start
        Route::endpoint("/", get(pages::home::handler)),
        // proa:route-specs:end
    ]);
    let manifest = Arc::new(IslandManifest {
        islands: HashMap::new(),
        assets: Default::default(),
        import_map: HashMap::new(),
        ssr_bundle: None,
    });
    router
        .route("/healthz", get(endpoints::health::live))
        .route("/readyz", get(endpoints::health::ready))
        // proa:explicit-routes:start
        __MARKDOWN_ROUTE__
        // proa:explicit-routes:end
        .nest_service("/static", static_assets::service())
        .with_state(state)
        .layer(RouteContextLayer::new(manifest, metadata))
        .layer(TraceLayer::new_for_http())
}
"#
    .replace("__MARKDOWN_ROUTE__", md_route)
}

pub fn health_rs(has_database: bool) -> String {
    if has_database {
        r#"use axum::{extract::State, http::StatusCode, response::IntoResponse, Json};
use serde::Serialize;

use crate::app::AppState;

#[derive(Serialize)]
pub struct StatusBody { status: &'static str }

pub async fn live() -> Json<StatusBody> {
    Json(StatusBody { status: "ok" })
}

pub async fn ready(State(state): State<AppState>) -> impl IntoResponse {
    match sqlx::query_scalar::<_, i64>("SELECT 1").fetch_one(&state.db).await {
        Ok(1) => (StatusCode::OK, Json(StatusBody { status: "ready" })),
        _ => (StatusCode::SERVICE_UNAVAILABLE, Json(StatusBody { status: "unavailable" })),
    }
}
"#
        .to_string()
    } else {
        r#"use axum::Json;
use serde::Serialize;

#[derive(Serialize)]
pub struct StatusBody { status: &'static str }

pub async fn live() -> Json<StatusBody> {
    Json(StatusBody { status: "ok" })
}

pub async fn ready() -> Json<StatusBody> {
    Json(StatusBody { status: "ready" })
}
"#
        .to_string()
    }
}

pub fn main_rs(has_database: bool) -> String {
    let modules = if has_database {
        "mod db;\nmod loader;\n"
    } else {
        ""
    };
    let state = if has_database {
        r#"    let db = db::connect(&config.database_url).await?;
    let loader = Arc::new(loader::AppLoader::new(db.clone()));
    let state = app::AppState { db, loader };
"#
    } else {
        "    let state = app::AppState;\n"
    };
    r#"__STD_IMPORT__

use tokio::net::TcpListener;
use tracing_subscriber::EnvFilter;

mod app;
mod components;
mod config;
__MODULES__mod endpoints;
mod error;
mod layouts;
mod pages;
mod routes;
mod static_assets;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt().with_env_filter(EnvFilter::from_default_env()).init();
    let config = config::Config::from_env()?;
__STATE__    let app = routes::router(state);
    let listener = TcpListener::bind(config.address).await?;
    tracing::info!(address = %listener.local_addr()?, "Proa site listening");
    let _ = proa_framework_axum::serve(listener, app, Duration::from_secs(30)).await?;
    Ok(())
}
"#
    .replace(
        "__STD_IMPORT__",
        if has_database {
            "use std::{sync::Arc, time::Duration};"
        } else {
            "use std::time::Duration;"
        },
    )
    .replace("__MODULES__", modules)
    .replace("__STATE__", state)
}

pub fn static_assets_rs() -> &'static str {
    r#"use tower_http::services::ServeDir;

pub fn service() -> ServeDir {
    ServeDir::new("public").append_index_html_on_directories(false)
}
"#
}
