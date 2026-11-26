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
        "Self { address: SocketAddr::new(host, port), base_path, database_url }"
    } else {
        "Self { address: SocketAddr::new(host, port), base_path }"
    };
    r#"use std::net::{IpAddr, SocketAddr};

pub struct Config {
    pub address: SocketAddr,
    pub base_path: proa::BasePath,
__DATABASE_FIELD__
}

impl Config {
    pub fn from_env() -> anyhow::Result<Self> {
        dotenvy::dotenv().ok();
        let base_path = proa::BasePath::new(
            &std::env::var("PROA_BASE_PATH").unwrap_or_default(),
        )?;
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

use anyhow::Context;
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
    // SQLite creates a missing database file but not its directory. An
    // in-memory URL has no directory.
    if let Some(directory) = options
        .get_filename()
        .parent()
        .filter(|directory| !directory.as_os_str().is_empty())
    {
        std::fs::create_dir_all(directory)
            .with_context(|| format!("create database directory {}", directory.display()))?;
    }
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

use proa::{DataLoader, LoadError, LoadKey, LoadRequest, LoadValue};
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
    Render(#[from] proa::WriteError),
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

use proa::{ComponentRole, DataLoader, RenderOutcome, WebContext, WebRender, WriteError};
use proa::html;

pub struct RootDocument<C> {
    pub content: C,
}

impl<L, C> WebRender<L> for RootDocument<C>
where
    L: DataLoader,
    C: WebRender<L> + Send + 'static,
{
    const ROLE: ComponentRole = ComponentRole::LAYOUT;

    fn render(
        self,
        cx: &mut WebContext<L>,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>> {
        RenderOutcome::pending(async move {
            let (favicon, stylesheet) = if cx.request().has_public_assets() {
                ("public/favicon.svg", "public/styles.css")
            } else {
                ("/static/favicon.svg", "/static/styles.css")
            };
            let favicon = cx.url(favicon);
            let stylesheet = cx.url(stylesheet);
            html! {
                <!DOCTYPE html>
                <html lang="en">
                    <head>
                        <meta charset="utf-8">
                        <meta name="viewport" content="width=device-width, initial-scale=1">
                        <meta name="color-scheme" content="dark">
                        {cx.head()}
                        <link rel="icon" href={favicon}>
                        <link rel="stylesheet" href={stylesheet}>
                    </head>
                    <body>{self.content}</body>
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

use proa::{DataLoader, RenderOutcome, WebContext, WebRender, WebRenderSync, WriteError};
use proa::html_sync;

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

impl<L: DataLoader> proa::MdRender<L> for DatabaseBadge {
    fn render_md(self, cx: &mut WebContext<L>) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>> {
        RenderOutcome::pending(async move {
            let status = cx.load_typed::<DatabaseStatus>(database_status_key()).await?;
            proa::MdRenderSync::render_md(proa::md_sync! { "{status.driver} {status.version.as_str()}" }, cx)
        })
    }
}
"#
}

pub fn database_card_rs() -> &'static str {
    r####"use std::future::Future;

use proa::{DataLoader, RenderOutcome, WebContext, WebRender, WebRenderSync, WriteError};
use proa::html_sync;

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

impl<L: DataLoader> proa::MdRender<L> for DatabaseCard {
    fn render_md(self, cx: &mut WebContext<L>) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>> {
        RenderOutcome::pending(async move {
            let status = cx.load_typed::<DatabaseStatus>(database_status_key()).await?;
            proa::MdRenderSync::render_md(proa::md_sync! { r###"
                ## DATA ORIGIN

                {status.driver}

                Two component requests resolve through one keyed origin query."###
            }, cx)
        })
    }
}
"####
}

pub fn home_rs(
    has_database: bool,
    has_ui: bool,
    markdown: bool,
    blank: bool,
    title: &str,
) -> String {
    let mut imports = Vec::new();
    if has_database {
        imports.push("use std::future::Future;");
        imports.push("");
    }
    imports.push(match (has_database, markdown) {
        (true, true) => "use axum::{extract::State, http::HeaderMap, response::IntoResponse};",
        (true, false) => "use axum::{extract::State, response::IntoResponse};",
        (false, true) => "use axum::{http::HeaderMap, response::IntoResponse};",
        (false, false) => "use axum::response::IntoResponse;",
    });
    imports.push(if markdown {
        if has_database {
            "use proa::axum::{explicit_markdown_async_with_loader, negotiate_async_with_loader, MarkdownAlternate, RouteResponse};"
        } else {
            "use proa::axum::{explicit_markdown, negotiate, MarkdownAlternate, RouteResponse};"
        }
    } else {
        "use proa::axum::RouteResponse;"
    });
    if has_database {
        imports.push("use proa::stream::DataResolver;");
    }
    imports.push(match (has_database, blank) {
        (true, true) => "use proa::{html, ComponentRole, DataLoader, HeadConfig, RenderOutcome, WebContext, WebRender, WriteError};",
        (true, false) => "use proa::{html, ComponentRole, DataLoader, HeadConfig, HeadMetadata, RenderOutcome, WebContext, WebRender, WriteError};",
        (false, true) => "use proa::{html_sync, ComponentRole, DataLoader, HeadConfig, SyncHeadOperation, WebContext, WebRenderSync, WriteError};",
        (false, false) => "use proa::{html_sync, ComponentRole, DataLoader, HeadConfig, HeadMetadata, SyncHeadOperation, WebContext, WebRenderSync, WriteError};",
    });
    if markdown {
        if has_database {
            imports.push("use proa::{md, MdRender};");
        } else {
            imports.push("use proa::{md_sync, MdRenderSync, WriteBuf};");
        }
    }
    imports.push("");
    if has_database {
        imports.push("use crate::{app::AppState, error::AppError, layouts::RootDocument};");
    } else {
        imports.push("use crate::{error::AppError, layouts::RootDocument};");
    }
    if !blank {
        match (has_database, has_ui) {
            (true, true) => imports
                .push("use crate::components::{Badge, Button, Card, DatabaseBadge, DatabaseCard};"),
            (true, false) => {
                imports.push("use crate::components::{DatabaseBadge, DatabaseCard};");
            }
            (false, true) => imports.push("use crate::components::{Badge, Button, Card};"),
            (false, false) => {}
        }
    }

    let title_constant = format!("const TITLE: &str = {};", super::rust_string_literal(title));
    let mut constants = vec![title_constant.as_str()];
    if blank {
        constants.push("const READY_HEADING: &str = \"Ready.\";");
    } else {
        constants.extend([
            "const WORDMARK: &str = \"PROA\";",
            "const SUBTITLE: &str = \"COMPILED APPLICATION STARTER\";",
            "const KICKER: &str = \"THE SERVER ALREADY DID THE WORK\";",
            "const HEADLINE: &str = \"Build the page in Rust. Ship only what moves.\";",
            "const LEDE: &str = \"Typed SSR, request-local data loading, semantic Markdown, and source-owned components in one production-shaped application.\";",
            "const SERVER_BADGE: &str = \"SERVER RENDERED IN RUST\";",
            "const READ_MARKDOWN_LABEL: &str = \"READ THE MARKDOWN\";",
            "const READ_MARKDOWN_HREF: &str = \"/index.md\";",
            "const CLIENT_LABEL: &str = \"CLIENT\";",
            "const CLIENT_VALUE: &str = \"0 B\";",
            "const CLIENT_NOTE: &str = \"No client root on this route.\";",
            "const RECEIPT_QUERY: &str = \"2 component requests → 1 origin query\";",
            "const RECEIPT_RENDER: &str = \"Composable, multi-target rendering\";",
            "const RECEIPT_BRAND: &str = \"Rendered by Proa\";",
        ]);
        if !has_database {
            constants.extend([
                "const NO_DATABASE_BADGE: &str = \"NO DATABASE SELECTED\";",
                "const NO_DATABASE_LABEL: &str = \"DATA\";",
                "const NO_DATABASE_VALUE: &str = \"NONE\";",
            ]);
        }
        if has_ui {
            constants.push(
                r#"const REPRESENTATIONS_CARD: Card = Card {
    eyebrow: "REPRESENTATIONS",
    title: "Multi-target rendering",
    body: "Share component data and typed children across HTML, Markdown, and other render targets.",
    class: None,
};"#,
            );
        } else {
            constants.push("const HTML_CARD_TITLE: &str = \"HTML from Rust\";");
        }
    }

    let html_macro = if has_database { "html" } else { "html_sync" };
    let body = if blank {
        r#"__HTML_MACRO__! {
            <main class="blank-page">
                <h1>{READY_HEADING}</h1>
            </main>
        }
        .render(cx)"#
            .replace("__HTML_MACRO__", html_macro)
    } else {
        let badge = if has_ui {
            if has_database {
                "{proa::to_async(Badge::new(SERVER_BADGE))}"
            } else {
                "{Badge::new(SERVER_BADGE)}"
            }
        } else {
            "<span class=\"status-pill\">{SERVER_BADGE}</span>"
        };
        let database_badge = if has_database {
            "{DatabaseBadge}"
        } else {
            "<span class=\"status-pill\">{NO_DATABASE_BADGE}</span>"
        };
        let card = if has_ui {
            if has_database {
                "{proa::to_async(REPRESENTATIONS_CARD)}"
            } else {
                "{REPRESENTATIONS_CARD}"
            }
        } else {
            "<article class=\"metric-card\">\n                        <h2>{HTML_CARD_TITLE}</h2>\n                    </article>"
        };
        let db_card = if has_database {
            "{DatabaseCard}"
        } else {
            "<article class=\"metric-card\">\n                        <p class=\"metric-label\">{NO_DATABASE_LABEL}</p>\n                        <p class=\"metric-value\">{NO_DATABASE_VALUE}</p>\n                    </article>"
        };
        let button = if has_ui {
            if has_database {
                "{proa::to_async(Button::link(READ_MARKDOWN_LABEL, READ_MARKDOWN_HREF))}"
            } else {
                "{Button::link(READ_MARKDOWN_LABEL, READ_MARKDOWN_HREF)}"
            }
        } else {
            "<a href={cx.url(READ_MARKDOWN_HREF)}>{READ_MARKDOWN_LABEL}</a>"
        };
        r#"__HTML_MACRO__! {
            <main>
                <header class="topbar">
                    <a class="wordmark" href={cx.url("/")}>{WORDMARK}</a>
                    <span>{SUBTITLE}</span>
                </header>
                <section class="hero">
                    <p class="kicker">{KICKER}</p>
                    <h1>{HEADLINE}</h1>
                    <p class="lede">{LEDE}</p>
                    <div class="badges">
                        __BADGE__
                        __DATABASE_BADGE__
                    </div>
                    <div class="hero-action">
                        __BUTTON__
                    </div>
                </section>
                <section class="metrics">
                    <article class="metric-card">
                        <p class="metric-label">{CLIENT_LABEL}</p>
                        <p class="metric-value">{CLIENT_VALUE}</p>
                        <p class="metric-note">{CLIENT_NOTE}</p>
                    </article>
                    __DB_CARD__
                    __CARD__
                </section>
                <footer class="receipt">
                    <span>{RECEIPT_QUERY}</span>
                    <span>{RECEIPT_RENDER}</span>
                    <strong>{RECEIPT_BRAND}</strong>
                </footer>
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

    let markdown_impl = if markdown {
        home_markdown_impl(has_database, has_ui, blank)
    } else {
        String::new()
    };

    let head = if blank {
        "cx.head_mut().title(TITLE)"
    } else {
        "cx.head_mut().metadata(HeadMetadata {
            title: TITLE.into(),
            description: LEDE.into(),
            ..Default::default()
        })"
    };
    let render_impl = if has_database {
        r#"impl<L: DataLoader> WebRender<L> for HomePage {
    const ROLE: ComponentRole = ComponentRole::PAGE;

    async fn render_head(&self, cx: &mut WebContext<L>) -> Result<(), WriteError> {
        __HEAD__
    }

    fn render(
        self,
        cx: &mut WebContext<L>,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>> {
        __BODY__
    }
}"#
    } else {
        r#"impl<L: DataLoader> WebRenderSync<L> for HomePage {
    const ROLE: ComponentRole = ComponentRole::PAGE;

    fn render_head(&self, cx: &mut WebContext<L>) -> impl SyncHeadOperation {
        __HEAD__
    }

    fn render(self, cx: &mut WebContext<L>) -> Result<(), WriteError> {
        __BODY__
    }
}"#
    };

    let args = match (has_database, markdown) {
        (true, true) => "State(state): State<AppState>, headers: HeaderMap",
        (true, false) => "State(state): State<AppState>",
        (false, true) => "headers: HeaderMap",
        (false, false) => "",
    };
    let html_response = if has_database {
        r#"let resolver = DataResolver::request_scope(state.loader.clone());
    let html = RouteResponse::ssr_async_with_loader(RootDocument { content: HomePage }, resolver.clone())
        .with_head(HeadConfig::default());"#
    } else {
        r#"let html = RouteResponse::ssr_async(RootDocument { content: proa::to_async(HomePage) })
        .with_head(HeadConfig::default());"#
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

    r#"__IMPORTS__

__CONSTANTS__

#[derive(Debug, Clone, Copy)]
pub struct HomePage;

__RENDER_IMPL__
__MARKDOWN_IMPL__

pub async fn handler(__ARGS__) -> Result<impl IntoResponse, AppError> {
    __HTML_RESPONSE__
    __RESPONSE__
}
__MD_HANDLER__"#
        .replace("__IMPORTS__", &imports.join("\n"))
        .replace("__CONSTANTS__", &constants.join("\n"))
        .replace(
            "__RENDER_IMPL__",
            &render_impl
                .replace("__HEAD__", head)
                .replace("__BODY__", &body),
        )
        .replace("__MARKDOWN_IMPL__", &markdown_impl)
        .replace("__ARGS__", args)
        .replace("__HTML_RESPONSE__", html_response)
        .replace("__RESPONSE__", response)
        .replace("__MD_HANDLER__", md_handler)
}

fn home_markdown_impl(has_database: bool, has_ui: bool, blank: bool) -> String {
    let lift = |child: &str| {
        if has_database {
            format!("{{proa::to_md_async({child})}}")
        } else {
            format!("{{{child}}}")
        }
    };
    let lines = if blank {
        "# {READY_HEADING}".to_string()
    } else {
        let server_badge = if has_ui {
            lift("Badge::new(SERVER_BADGE)")
        } else {
            "{SERVER_BADGE}".to_string()
        };
        let db_badge = if has_database {
            "{DatabaseBadge}"
        } else {
            "{NO_DATABASE_BADGE}"
        };
        let action = if has_ui {
            lift("Button::link(READ_MARKDOWN_LABEL, READ_MARKDOWN_HREF)")
        } else {
            "[{READ_MARKDOWN_LABEL}]({cx.url(READ_MARKDOWN_HREF)})".to_string()
        };
        let db_section = if has_database {
            "{DatabaseCard}"
        } else {
            "## {NO_DATABASE_LABEL}\n\n{NO_DATABASE_VALUE}"
        };
        let card_section = if has_ui {
            lift("REPRESENTATIONS_CARD")
        } else {
            "## {HTML_CARD_TITLE}".to_string()
        };
        r#"[{WORDMARK}]({cx.url("/")}) · {SUBTITLE}

{KICKER}

# {HEADLINE}

{LEDE}

__SERVER_BADGE__ · __DB_BADGE__

__ACTION__

## {CLIENT_LABEL}

{CLIENT_VALUE} · {CLIENT_NOTE}

__DB_SECTION__

__CARD_SECTION__

---

{RECEIPT_QUERY} · {RECEIPT_RENDER} · **{RECEIPT_BRAND}**"#
            .replace("__SERVER_BADGE__", &server_badge)
            .replace("__DB_BADGE__", db_badge)
            .replace("__ACTION__", &action)
            .replace("__DB_SECTION__", db_section)
            .replace("__CARD_SECTION__", &card_section)
    };
    let implementation = if has_database {
        "impl<L: DataLoader> MdRender<L> for HomePage"
    } else {
        "impl<L: DataLoader, B: WriteBuf> MdRenderSync<L, B> for HomePage"
    };
    let signature = if has_database {
        "fn render_md(\n        self,\n        cx: &mut WebContext<L>,\n    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>>"
    } else {
        "fn render_md(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError>"
    };
    let indented = lines
        .lines()
        .map(|line| {
            if line.is_empty() {
                String::new()
            } else {
                format!("            {line}")
            }
        })
        .collect::<Vec<_>>()
        .join("\n");
    r####"
__IMPL__ {
    __SIGNATURE__ {
        __MACRO__! { r###"
__LINES__
            "###
        }
        .render_md(cx)
    }
}"####
        .replace("__IMPL__", implementation)
        .replace("__SIGNATURE__", signature)
        .replace("__MACRO__", if has_database { "md" } else { "md_sync" })
        .replace("__LINES__", &indented)
}

pub fn routes_rs(markdown: bool) -> String {
    let md_route = if markdown {
        ".route(\"/index.md\", get(pages::home::markdown))"
    } else {
        ""
    };
    r#"use axum::{routing::get, Router};
use proa::axum::{router_from_spec, FrameworkBuildError, FrameworkBuilder, Route};
use tower_http::trace::TraceLayer;

use crate::{app::AppState, endpoints, pages, static_assets};

pub fn router(state: AppState, base_path: proa::BasePath) -> Result<Router, FrameworkBuildError> {
    let (router, metadata) = router_from_spec(vec![
        // proa:route-specs:start
        Route::endpoint("/", get(pages::home::handler)),
        // proa:route-specs:end
    ]);
    let router = router
        .route("/healthz", get(endpoints::health::live))
        .route("/readyz", get(endpoints::health::ready))
        // proa:explicit-routes:start
        __MARKDOWN_ROUTE__
        // proa:explicit-routes:end
        .with_state(state);
    let router = if !proa::axum::PublicAssets::configured() {
        // Fallback when running bare `cargo test` without `proa` publishing `public/`.
        router.nest_service("/static", static_assets::service(base_path.clone()))
    } else {
        router
    };
    let app = FrameworkBuilder::new()
        .with_base_path(base_path)
        .with_dynamic_routes_and_metadata(router, metadata)
        .try_build()?;
    Ok(app.layer(TraceLayer::new_for_http()))
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
__STATE__    let app = routes::router(state, config.base_path)?;
    let listener = TcpListener::bind(config.address).await?;
    tracing::info!(address = %listener.local_addr()?, "Proa site listening");
    let _ = proa::axum::serve(listener, app, Duration::from_secs(30)).await?;
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

pub fn service(base_path: proa::BasePath) -> axum::Router {
    proa::axum::base_path_assets(
        axum::Router::new().fallback_service(
            // proa-perf-ok: only for a bare `cargo test`; a build serves the catalog
            ServeDir::new("public").append_index_html_on_directories(false),
        ),
        base_path,
    )
}
"#
}
