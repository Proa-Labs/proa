// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

pub mod home;
use crate::{
    accept,
    app::AppState,
    auth,
    components::{
        markdown, status_badge, Badge, BadgeVariant, Button, ButtonType, Card, Input, InputType,
        IssueReport, Runbook, Table, TableBody, TableHeader, TableRow, Textarea,
    },
    domain::{self, Principal, ResolveInput, SearchInput},
    error::{AppError, Result},
    loader::RequestScope,
    security::random,
};
use axum::{
    body::Bytes,
    extract::{Path, Query, State},
    http::{header, HeaderMap},
    response::{Html, IntoResponse, Response},
    Json,
};
use proa_core::{WebContext, WebRenderSync};
use proa_framework_axum::explicit_markdown;
use proa_macros::html_sync;

pub fn page<C: WebRenderSync>(
    title: &'static str,
    principal: Option<&Principal>,
    content: C,
) -> Result<Response> {
    let mut cx = WebContext::new();
    html_sync!{
        <!DOCTYPE html>
        <html lang="en">
            <head>
                <meta charset="utf-8" />
                <meta name="viewport" content="width=device-width, initial-scale=1" />
                <title>{proa_core::raw_text_ok(title)}" · Incident Desk"</title>
                <meta name="color-scheme" content="light" />
                <link rel="icon" href="/static/favicon.svg" />
                <link rel="stylesheet" href="/static/styles.css" />
                <link rel="stylesheet" href="/static/incident.css" />
            </head>
            <body>
                <a class="skip-link" href="#main-content">"Skip to content"</a>
                <aside data-app-navigation class="sidebar">
                    <a class="brand" href="/issues">
                        <span class="brand-icon">"i"</span>
                        "Incident Desk"
                    </a>
                    <p class="sidebar-label">"WORKSPACE"</p>
                    {if let Some(p)=principal{html_sync! {
                        <strong class="workspace">{p.workspace_name.as_str()}</strong>
                    }}else{html_sync! {
                        <strong class="workspace">"Demo workspace"</strong>
                    }}}
                    <nav aria-label="Main">
                        <a
                            href="/issues"
                            aria-current={matches!(title, "Issues" | "Issue detail" | "Runbook").then_some("page")}
                        >
                            <span class="nav-symbol" aria-hidden="true">"▤"</span>
                            "Issues"
                        </a>
                        <a
                            href="/connections"
                            aria-current={(title=="Connections").then_some("page")}
                        >
                            <span class="nav-symbol" aria-hidden="true">"◇"</span>
                            "Connections"
                        </a>
                        <a
                            href="/connect"
                            aria-current={(title=="Connect an agent").then_some("page")}
                        >
                            <span class="nav-symbol" aria-hidden="true">"↗"</span>
                            "Connect an agent"
                        </a>
                        <a href="/lab" aria-current={(title=="Developer lab").then_some("page")}>
                            <span class="nav-symbol" aria-hidden="true">"⌘"</span>
                            "Developer lab"
                        </a>
                    </nav>
                    <div class="sidebar-bottom">
                        <span class="live-dot"></span>
                        "Built with Proa"
                        <p>"HTML for people. Context for agents."</p>
                    </div>
                </aside>
                <div class="app">
                    <header class="topbar">
                        <span class="breadcrumb">
                            <span>"Workspace"</span>
                            <span class="breadcrumb-separator">"/"</span>
                            <strong>{title}</strong>
                        </span>
                        {if let Some(p)=principal{html_sync! {
                            <span class="identity">
                                <span class="avatar">{p.name.as_str()}</span>
                                <span class="identity-role">{p.role.as_str()}</span>
                            </span>
                        }}else{html_sync! {
                            <a href="/auth/login">"Sign in"</a>
                        }}}
                    </header>
                    <main id="main-content">{content}</main>
                    <footer>
                        "Incident Desk is a fictional demonstration. Your workspace permissions apply everywhere."
                    </footer>
                </div>
            </body>
        </html>
    }.render(&mut cx)?;
    Ok(auth::no_store(
        Html(cx.into_output().into_string()).into_response(),
    ))
}
async fn user(
    s: &AppState,
    h: &HeaderMap,
) -> std::result::Result<(Principal, auth::Session), Response> {
    auth::browser_principal(s, h)
        .await
        .map_err(|_| auth::redirect("/auth/login"))
}
pub async fn issues(
    State(s): State<AppState>,
    h: HeaderMap,
    Query(query): Query<SearchInput>,
) -> Result<Response> {
    let (p, _) = match user(&s, &h).await {
        Ok(v) => v,
        Err(r) => return Ok(r),
    };
    let q = query.query.clone().unwrap_or_default();
    let status = query.status.clone().unwrap_or_default();
    let results = domain::search(&s.db, &p, query).await?;
    let rows: Vec<_> = results
        .issues
        .iter()
        // proa-perf-ok: HTTP adapter prepares navigation values before typed rendering.
        .map(|i| (i, format!("/issues/{}", i.id)))
        // proa-perf-ok: HTTP adapter prepares navigation values before typed rendering.
        .collect();
    let next = results.next_cursor.as_ref().map(|c| {
        // proa-perf-ok: HTTP adapter prepares navigation values before typed rendering.
        format!(
            "/issues?query={}&status={}&cursor={}",
            urlencoding::encode(&q),
            urlencoding::encode(&status),
            urlencoding::encode(c)
        )
    });
    page(
        "Issues",
        Some(&p),
        html_sync! {
            <div class="page-heading">
                <div>
                    <p class="eyebrow">{p.workspace_name.as_str()}" / TRIAGE"</p>
                    <h1>"Issues"</h1>
                    <p class="muted">
                        {rows.len()}
                        " issues in this view · Evidence for your next decision."
                    </p>
                </div>
                <a class="button secondary" href="/connect">"Connect an agent ↗"</a>
            </div>
            <form class="filters" method="get" action="/issues">
                <input
                    name="query"
                    aria-label="Search issues"
                    placeholder="Search issues…"
                    value={q.as_str()}
                />
                <select name="status" aria-label="Issue status">
                    <option value="">"All statuses"</option>
                    <option value="open" selected={status=="open"}>"Open"</option>
                    <option value="resolved" selected={status=="resolved"}>"Resolved"</option>
                </select>
                {Button {
                    button_type: ButtonType::Submit,
                    class: Some("desk-button secondary"),
                    ..Button::text("Apply filters")
                }}
            </form>
            <div class="table-wrap">
                {Table {
                    class: Some("issue-table"),
                    children: Some(html_sync! {
                        <caption class="sr-only">
                            "Issues in the current workspace matching your filters"
                        </caption>
                        {TableHeader {
                            class: None,
                            children: Some(html_sync! {
                                {TableRow {
                                    class: None,
                                    children: Some(html_sync! {
                                        <th scope="col">"Issue"</th>
                                        <th scope="col">"Status"</th>
                                        <th scope="col">"Severity"</th>
                                        <th scope="col">"Users"</th>
                                        <th scope="col">"Last seen"</th>
                                    }),
                                }}
                            }),
                        }}
                        {TableBody {
                            class: None,
                            children: Some(html_sync! {
                                {for (i,url) in &rows {TableRow { class: None, children: Some(html_sync! {
                                    <td><a class="issue-link" href={url.as_str()}><span class="issue-id">{i.id.as_str()}</span><strong>{i.title.as_str()}</strong></a></td>
                                    <td>{status_badge(i.status.as_str())}</td>
                                    <td><span class="severity" data-severity={i.severity.as_str()}>{i.severity.as_str()}</span></td>
                                    <td class="mono">{i.affected_users}</td>
                                    <td class="muted"><time>{i.last_seen.as_str()}</time></td>
                                }) }}}
                            }),
                        }}
                    }),
                }}
                {if rows.is_empty(){html_sync! {
                    <p class="empty">"No issues match these filters."</p>
                }}}
            </div>
            {if let Some(next)=&next{html_sync! {
                <a class="button secondary" href={next.as_str()}>"Next page →"</a>
            }}}
        },
    )
}
pub async fn issue(
    State(s): State<AppState>,
    h: HeaderMap,
    Path(raw): Path<String>,
) -> Result<Response> {
    let machine = raw.ends_with(".md") || raw.ends_with(".json") || accept::prefers_markdown(&h);
    let (p, session) = match auth::browser_principal(&s, &h).await {
        Ok(v) => v,
        Err(e) => {
            return if machine {
                Err(e)
            } else {
                Ok(auth::redirect("/auth/login"))
            }
        }
    };
    let id = raw
        .strip_suffix(".md")
        .or_else(|| raw.strip_suffix(".json"))
        .unwrap_or(&raw);
    let scope = RequestScope::new(&s.db, p.clone()).await?;
    let d = scope.report(id, &s.config.app_origin).await?;
    if raw.ends_with(".json") {
        return Ok(auth::no_store(Json(d).into_response()));
    }
    if machine {
        let mut r = auth::no_store(explicit_markdown(IssueReport::new(&d))?.into_response());
        r.headers_mut()
            .insert(header::VARY, "Accept".parse().unwrap());
        return Ok(r);
    }
    // proa-perf-ok: HTTP adapter prepares navigation values before typed rendering.
    let action = format!("/issues/{id}/resolve");
    // proa-perf-ok: HTTP adapter prepares navigation values before typed rendering.
    let runbook = format!("/runbooks/{id}");
    let key = random();
    let mut r = page(
        "Issue detail",
        Some(&p),
        html_sync! {
            <a class="back" href="/issues">"← All issues"</a>
            <div class="detail-grid">
                <div class="report">{IssueReport::new(&d)}</div>
                <aside class="issue-sidebar">
                    <section>
                        {Card {
                            class: Some("desk-panel panel"),
                            children: Some(html_sync! {
                                <p class="eyebrow">"NEXT STEP"</p>
                                {if d.can_resolve{html_sync! {
                                    <h2>"Record a resolution"</h2>
                                    <form method="post" action={action.as_str()}>
                                        <input type="hidden" name="csrf" value={session.csrf.as_str()} />
                                        <input type="hidden" name="expected_version" value={d.issue.version} />
                                        <input type="hidden" name="idempotency_key" value={key.as_str()} />
                                        <label for="resolution-note">"What changed?"</label>
                                        {Textarea { id: Some("resolution-note"), name: Some("note"), required: true, maxlength: Some("2000"), placeholder: Some("Describe the fix and verification…"), class: Some("desk-textarea"), ..Textarea::new() }}
                                        {Button { button_type: ButtonType::Submit, class: Some("desk-button"), ..Button::text("Resolve issue") }}
                                    </form>
                                }}else if d.issue.status=="resolved" {html_sync! {
                                    <span class="resolved-symbol" aria-hidden="true">"✓"</span>
                                    <h2>"Resolved"</h2>
                                    <p>"Recovery has been recorded. The investigation and its history remain available."</p>
                                    <p class="muted">"Revision "{d.issue.version}</p>
                                }}else{html_sync! {
                                    <h2>"Read access"</h2>
                                    <p>"Share your findings with a workspace maintainer to record a resolution."</p>
                                }}}
                            }),
                        }}
                    </section>
                    <section>
                        {Card {
                            class: Some("desk-panel panel"),
                            children: Some(html_sync! {
                                <h3>"Investigate with an agent"</h3>
                                <p>"Same incident. Same evidence. Your workspace permissions."</p>
                                <a href="/connect">"Connect via MCP →"</a>
                            }),
                        }}
                    </section>
                    <a href={runbook.as_str()}>"Open standalone runbook ↗"</a>
                </aside>
            </div>
        },
    )?;
    r.headers_mut()
        .insert(header::VARY, "Accept".parse().unwrap());
    // proa-perf-ok(scope): HTTP adapter prepares a dynamic alternate URL outside typed rendering.
    // proa-perf-ok: HTTP adapter prepares this dynamic alternate URL outside typed rendering.
    r.headers_mut().insert(
        header::LINK,
        format!("</issues/{id}.md>; rel=\"alternate\"; type=\"text/markdown\"") // proa-perf-ok: dynamic HTTP header prepared outside typed rendering
            .parse()
            .map_err(|_| AppError::Invalid("invalid issue link"))?,
    );
    Ok(r)
}
pub async fn resolve(
    State(s): State<AppState>,
    h: HeaderMap,
    Path(id): Path<String>,
    body: Bytes,
) -> Result<Response> {
    auth::origin(&h, &s.config.app_origin)?;
    let (p, session) = auth::browser_principal(&s, &h).await?;
    let values = auth::form(&body)?;
    auth::csrf(&session, auth::required(&values, "csrf")?)?;
    domain::resolve(
        &s.db,
        &p,
        ResolveInput {
            issue_id: id.clone(),
            expected_version: auth::required(&values, "expected_version")?
                .parse()
                .map_err(|_| AppError::Invalid("invalid version"))?,
            note: auth::required(&values, "note")?.into(),
            idempotency_key: auth::required(&values, "idempotency_key")?.into(),
        },
    )
    .await?;
    // proa-perf-ok: HTTP adapter prepares navigation values before typed rendering.
    Ok(auth::redirect(&format!("/issues/{id}")))
}
pub async fn runbook(
    State(s): State<AppState>,
    h: HeaderMap,
    Path(id): Path<String>,
) -> Result<Response> {
    let (p, _) = auth::browser_principal(&s, &h).await?;
    let scope = RequestScope::new(&s.db, p.clone()).await?;
    let d = scope.report(&id, &s.config.app_origin).await?;
    page(
        "Runbook",
        Some(&p),
        html_sync! {
            <div class="report">{Runbook::new(&d)}</div>
        },
    )
}
pub async fn connections(State(s): State<AppState>, h: HeaderMap) -> Result<Response> {
    let (p, session) = match user(&s, &h).await {
        Ok(v) => v,
        Err(r) => return Ok(r),
    };
    let grants:Vec<(String,String,String,i64,i64)>=sqlx::query_as("SELECT g.id,c.name,g.scopes,g.expires,g.revoked FROM grants g JOIN clients c ON c.id=g.client_id WHERE g.user_id=? AND g.workspace_id=? AND g.resource=? ORDER BY g.expires DESC LIMIT 50").bind(&p.user_id).bind(&p.workspace_id).bind(s.config.resource()).fetch_all(&s.db).await?;
    page(
        "Connections",
        Some(&p),
        html_sync! {
            <div class="page-heading">
                <div>
                    <p class="eyebrow">"ACCESS"</p>
                    <h1>"Your connections"</h1>
                    <p class="muted">"Control which agents can access this workspace."</p>
                </div>
                <a href="/connect" class="button">"Connect an agent"</a>
            </div>
            {if grants.is_empty(){html_sync! {
                <section>{Card { class: Some("desk-panel panel"), children: Some(html_sync! {
                    <h2>"No agent connections yet"</h2>
                    <p>"Connect Claude or Codex to investigate your issues."</p>
                }) }}</section>
            }}}
            {for(g,name,scopes,expiry,revoked)in &grants{html_sync! {
                <section class="connection panel">
                    <div>
                        <h2>{name.as_str()}</h2>
                        <p>
                            <code>{scopes.as_str()}</code>
                        </p>
                        <p class="muted">"Expires at Unix time "{*expiry}</p>
                    </div>
                    {if *revoked==0{html_sync! {
                        <form action="/connections/revoke" method="post">
                            <input type="hidden" name="grant" value={g.as_str()} />
                            <input type="hidden" name="csrf" value={session.csrf.as_str()} />
                            {Button { button_type: ButtonType::Submit, class: Some("desk-button secondary"), ..Button::text("Revoke access") }}
                        </form>
                    }}else{html_sync! {
                        {Badge { variant: BadgeVariant::Outline, class: Some("status-badge"), children: Some("Revoked") }}
                    }}}
                </section>
            }}}
            <form action="/auth/logout" method="post">
                <input type="hidden" name="csrf" value={session.csrf.as_str()} />
                {Button {
                    button_type: ButtonType::Submit,
                    class: Some("desk-button secondary"),
                    ..Button::text("Sign out of website")
                }}
            </form>
        },
    )
}
pub async fn revoke(State(s): State<AppState>, h: HeaderMap, body: Bytes) -> Result<Response> {
    auth::origin(&h, &s.config.app_origin)?;
    let (p, session) = auth::browser_principal(&s, &h).await?;
    let values = auth::form(&body)?;
    auth::csrf(&session, auth::required(&values, "csrf")?)?;
    sqlx::query("UPDATE grants SET revoked=1 WHERE id=? AND user_id=? AND workspace_id=?")
        .bind(auth::required(&values, "grant")?)
        .bind(p.user_id)
        .bind(p.workspace_id)
        .execute(&s.db)
        .await?;
    Ok(auth::redirect("/connections"))
}
pub async fn connect(State(s): State<AppState>, h: HeaderMap) -> Result<Response> {
    let (p, _) = match user(&s, &h).await {
        Ok(v) => v,
        Err(r) => return Ok(r),
    };
    let resource = s.config.resource();
    page(
        "Connect an agent",
        Some(&p),
        html_sync! {
            <div class="page-heading">
                <div>
                    <p class="eyebrow">"HUMANS + AGENTS"</p>
                    <h1>"Bring your agent into the investigation."</h1>
                    <p>"Sign in through your browser. Choose a workspace. Approve access."</p>
                </div>
            </div>
            <section>
                {Card {
                    class: Some("desk-panel panel"),
                    children: Some(html_sync! {
                        <h2>"MCP endpoint"</h2>
                        <code class="endpoint">{resource.as_str()}</code>
                        <p class="muted">
                            "Your browser login and agent connection have separate sessions. Revoke agent access in Connections."
                        </p>
                    }),
                }}
            </section>
            <div class="two-up">
                <section>
                    {Card {
                        class: Some("desk-panel panel"),
                        children: Some(html_sync! {
                            <h2>"Claude Desktop"</h2>
                            <p>
                                "Add a custom connector using a publicly reachable HTTPS profile, or the documented local OAuth bridge in the README."
                            </p>
                            <p>"The bridge performs browser OAuth and validates TLS."</p>
                        }),
                    }}
                </section>
                <section>
                    {Card {
                        class: Some("desk-panel panel"),
                        children: Some(html_sync! {
                            <h2>"Codex Desktop"</h2>
                            <p>
                                "Add a Streamable HTTP MCP server using the endpoint above. Complete browser authorization when prompted."
                            </p>
                            <p>"See the README for local certificate trust and configuration."</p>
                        }),
                    }}
                </section>
            </div>
            <section>
                {Card {
                    class: Some("desk-panel panel"),
                    children: Some(html_sync! {
                        <h2>"Try an investigation"</h2>
                        <blockquote>
                            "Investigate AST-1042. Show the evidence and the next checks."
                        </blockquote>
                        <p>
                            "Your agent receives the same authorized information shown in the browser."
                        </p>
                    }),
                }}
            </section>
        },
    )
}
pub async fn lab(State(s): State<AppState>, h: HeaderMap) -> Result<Response> {
    let (p, _) = match user(&s, &h).await {
        Ok(v) => v,
        Err(r) => return Ok(r),
    };
    let id = if p.workspace_id == "aster" {
        "AST-1042"
    } else {
        "HBR-201"
    };
    let scope = RequestScope::new(&s.db, p.clone()).await?;
    let d = scope.report(id, &s.config.app_origin).await?;
    let md = markdown(&d)?;
    // proa-perf-ok: developer lab materializes response artifacts for explicit comparison.
    let json = serde_json::to_string_pretty(&d).map_err(anyhow::Error::from)?;
    // proa-perf-ok: developer lab materializes response artifacts for explicit comparison.
    let compact = serde_json::to_string(&d).map_err(anyhow::Error::from)?;
    let mut cx = WebContext::new();
    IssueReport::new(&d).render(&mut cx)?;
    let html = cx.into_output().into_string();
    let tokenizer = tiktoken_rs::cl100k_base()?;
    let md_tokens = tokenizer.encode_ordinary(&md).len();
    let json_tokens = tokenizer.encode_ordinary(&compact).len();
    let reads = scope.inner.reads.load(std::sync::atomic::Ordering::Relaxed);
    page(
        "Developer lab",
        Some(&p),
        html_sync! {
            <p class="eyebrow">"ONE AUTHORIZED REVISION"</p>
            <h1>"Inspect every representation."</h1>
            <p class="muted">
                "These outputs share one loaded projection. Token counts use cl100k_base / tiktoken-rs 0.7. The representations contain different fields; these numbers do not isolate format savings."
            </p>
            <div class="metrics panel">
                <div>
                    <strong>{d.issue.version}</strong>
                    <span>"issue revision"</span>
                </div>
                <div>
                    <strong>{reads}</strong>
                    <span>"keyed SQL reads / 3 logical loads"</span>
                </div>
                <div>
                    <strong>{md_tokens}</strong>
                    <span>"Markdown tokens"</span>
                </div>
                <div>
                    <strong>{json_tokens}</strong>
                    <span>"compact JSON tokens"</span>
                </div>
            </div>
            <details open>
                <summary>"HTML preview"</summary>
                <div class="report">{IssueReport::new(&d)}</div>
            </details>
            <details>
                <summary>"Markdown output"</summary>
                <pre>{md.as_str()}</pre>
            </details>
            <details>
                <summary>"JSON output"</summary>
                <pre>{json.as_str()}</pre>
            </details>
            <details>
                <summary>"HTML source"</summary>
                <pre>{html.as_str()}</pre>
            </details>
            <section>
                {Card {
                    class: Some("desk-panel panel"),
                    children: Some(html_sync! {
                        <h2>"Shared source"</h2>
                        <p>
                            <code>"src/components/issue_report/issue_report.rs"</code>
                            " — composed HTML and Markdown"
                        </p>
                        <p>
                            <code>"src/loader.rs"</code>
                            " — scoped typed loading"
                        </p>
                        <p>
                            <code>"src/domain/mod.rs"</code>
                            " — shared queries and mutation"
                        </p>
                    }),
                }}
            </section>
        },
    )
}
pub fn provider(
    _s: &AppState,
    a: &auth::provider::Authorization,
    flow: &str,
    user: Option<&(String, Vec<(String, String, String)>)>,
    error: Option<&str>,
) -> Result<Response> {
    let mut response = page(
        "Authorize",
        None,
        html_sync! {
            <div class="auth-card panel">
                <p class="eyebrow">"INCIDENT DESK · DEMO IDENTITY"</p>
                <h1>"Authorize your connection"</h1>
                <p>
                    <strong>{a.client_name.as_str()}</strong>
                    " is requesting access."
                </p>
                {if let Some(error)=error{html_sync! {
                    <p class="error" role="alert">{error}</p>
                }}}
                {if let Some((name,workspaces))=user{html_sync! {
                    <p>
                        "Signed in as "
                        <strong>{name.as_str()}</strong>
                    </p>
                    <form action="/consent" method="post">
                        <input type="hidden" name="flow" value={flow} />
                        <label for="workspace">"Workspace"</label>
                        <select id="workspace" name="workspace">
                            {for(id,name,role)in workspaces{html_sync! {
                                <option value={id.as_str()}>{name.as_str()}" · "{role.as_str()}</option>
                            }}}
                        </select>
                        <h3>"Requested permissions"</h3>
                        <p>
                            <code>{a.scopes.as_str()}</code>
                        </p>
                        <p class="muted">
                            "Access is limited by your workspace role. Viewer accounts cannot resolve issues."
                        </p>
                        <div class="actions">
                            <button class="button" name="decision" value="approve">"Approve access"</button>
                            <button class="button secondary" name="decision" value="deny">"Deny"</button>
                        </div>
                    </form>
                }}else{html_sync! {
                    <form action="/authorize/login" method="post">
                        <input type="hidden" name="flow" value={flow} />
                        <label for="user">"Demo account"</label>
                        {Input { id: Some("user"), name: Some("user"), autocomplete: Some("username"), required: true, placeholder: Some("alice, ben, or casey"), class: Some("desk-input"), ..Input::new() }}
                        <label for="password">"Password"</label>
                        {Input { id: Some("password"), name: Some("password"), input_type: InputType::Password, autocomplete: Some("current-password"), required: true, class: Some("desk-input"), ..Input::new() }}
                        <p class="muted">"Use the generated password from your local setup."</p>
                        {Button { button_type: ButtonType::Submit, class: Some("desk-button"), ..Button::text("Sign in") }}
                    </form>
                }}}
            </div>
        },
    )?;
    // proa-perf-ok: the OAuth adapter allows only this client's validated callback origin.
    let callback = url::Url::parse(&a.redirect_uri)
        .map_err(anyhow::Error::from)?
        .origin()
        .ascii_serialization();
    // proa-perf-ok: CSP is prepared once at the HTTP boundary, outside rendering.
    response.headers_mut().insert("content-security-policy", format!("default-src 'none'; style-src 'self'; img-src 'self' data:; form-action 'self' {callback}; base-uri 'none'; frame-ancestors 'none'").parse().map_err(|_| AppError::Invalid("invalid callback origin"))?);
    Ok(response)
}
