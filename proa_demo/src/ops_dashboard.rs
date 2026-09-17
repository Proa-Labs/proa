// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Product-app stress page for `html_sync!` and RSJS-analyzed components.
//!
//! This is intentionally less pixel-perfect than the travel/marketplace/footwear
//! showcases. Its job is to exercise authoring shapes that show up in
//! internal tools: filters, repeated row actions, status badges, optional
//! panels, forms, checkboxes, and islands embedded inside table rows.

use proa_core::{DataLoader, StaticRaw, WebContext, WebRenderSync, WriteBuf, WriteError};
use proa_macros::html_sync;
#[allow(unused_imports)]
use rsjs::signal;
use rsjs_macros::rsjs;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TicketStatus {
    Open,
    Watching,
    Resolved,
}

impl TicketStatus {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Open => "Open",
            Self::Watching => "Watching",
            Self::Resolved => "Resolved",
        }
    }

    const fn class(self) -> &'static str {
        match self {
            Self::Open => "ops-pill ops-pill--open",
            Self::Watching => "ops-pill ops-pill--watching",
            Self::Resolved => "ops-pill ops-pill--resolved",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Priority {
    Low,
    Medium,
    High,
    Critical,
}

impl Priority {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Low => "Low",
            Self::Medium => "Medium",
            Self::High => "High",
            Self::Critical => "Critical",
        }
    }

    const fn class(self) -> &'static str {
        match self {
            Self::Low => "ops-priority ops-priority--low",
            Self::Medium => "ops-priority ops-priority--medium",
            Self::High => "ops-priority ops-priority--high",
            Self::Critical => "ops-priority ops-priority--critical",
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Ticket<'a> {
    pub id: &'a str,
    pub account: &'a str,
    pub owner: &'a str,
    pub title: &'a str,
    pub status: TicketStatus,
    pub priority: Priority,
    pub updated: &'a str,
    pub value: &'a str,
    pub stale: bool,
}

#[derive(Debug, Clone, Copy)]
pub struct Kpi<'a> {
    pub label: &'a str,
    pub value: &'a str,
    pub trend: &'a str,
}

pub struct DashboardData<'a> {
    pub kpis: &'a [Kpi<'a>],
    pub tickets: &'a [Ticket<'a>],
}

pub fn default_data() -> DashboardData<'static> {
    DashboardData {
        kpis: &[
            Kpi {
                label: "Open escalations",
                value: "18",
                trend: "+4 today",
            },
            Kpi {
                label: "At-risk ARR",
                value: "$4.8M",
                trend: "6 critical accounts",
            },
            Kpi {
                label: "Median response",
                value: "7m",
                trend: "-18% vs last week",
            },
            Kpi {
                label: "Resolved this week",
                value: "143",
                trend: "92% within SLA",
            },
        ],
        tickets: &[
            Ticket {
                id: "ESC-1042",
                account: "Northstar Robotics",
                owner: "Mina",
                title: "Checkout latency above enterprise SLA",
                status: TicketStatus::Open,
                priority: Priority::Critical,
                updated: "4m ago",
                value: "$1.2M",
                stale: true,
            },
            Ticket {
                id: "ESC-1039",
                account: "Tandem Health",
                owner: "Diego",
                title: "SAML certificate rotation blocked",
                status: TicketStatus::Watching,
                priority: Priority::High,
                updated: "18m ago",
                value: "$840K",
                stale: false,
            },
            Ticket {
                id: "ESC-1034",
                account: "Bluebird Freight",
                owner: "Priya",
                title: "Regional search index lagging",
                status: TicketStatus::Open,
                priority: Priority::Medium,
                updated: "31m ago",
                value: "$410K",
                stale: false,
            },
            Ticket {
                id: "ESC-1027",
                account: "Aster Bank",
                owner: "Jon",
                title: "Webhook replay window needs audit trail",
                status: TicketStatus::Resolved,
                priority: Priority::High,
                updated: "1h ago",
                value: "$2.1M",
                stale: false,
            },
            Ticket {
                id: "ESC-1021",
                account: "Frame Labs",
                owner: "Lena",
                title: "Usage export missing cost-center column",
                status: TicketStatus::Watching,
                priority: Priority::Low,
                updated: "2h ago",
                value: "$190K",
                stale: true,
            },
            Ticket {
                id: "ESC-1018",
                account: "Harbor Cloud",
                owner: "Sam",
                title: "Billing page intermittently returns stale plan",
                status: TicketStatus::Open,
                priority: Priority::Critical,
                updated: "3h ago",
                value: "$960K",
                stale: true,
            },
        ],
    }
}

pub fn render() -> String {
    // proa-perf-ok: standalone demo helper; route handlers render with their own response buffers
    let mut out = Vec::new();
    let mut cx = WebContext::with_buffer(&mut out);
    let _ = OpsDashboard {
        data: &default_data(),
    }
    .render(&mut cx);
    String::from_utf8(out).unwrap_or_default()
}

pub struct OpsDashboard<'a> {
    pub data: &'a DashboardData<'a>,
}

impl<L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for OpsDashboard<'_> {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let _ = html_sync! {
            <!DOCTYPE html>
            <html lang="en">
                <head>
                    <meta charset="utf-8" />
                    <meta name="viewport" content="width=device-width, initial-scale=1" />
                    <title>"Ops Dashboard Stress Page"</title>
                    <style>{StaticRaw(CSS)}</style>
                </head>
                <body>
                    <main class="ops-shell">
                        <header class="ops-header">
                            <div>
                                <p class="ops-eyebrow">"Enterprise support"</p>
                                <h1>"Escalation command center"</h1>
                            </div>
                            <div class="ops-header__actions">
                                <a href="/showcase">"Showcases"</a>
                                <button type="button">"Export"</button>
                            </div>
                        </header>
                        <section class="ops-kpis" aria-label="Escalation metrics">
                            {for kpi in self.data.kpis {
                                render_kpi(cx, kpi);
                            }}
                        </section>
                        {DashboardControls {
                            initial_query: "latency",
                            initial_urgent_only: true,
                            initial_show_resolved: false,
                            result_count: self.data.tickets.len(),
                        }}
                        <section class="ops-panel" aria-labelledby="escalations-title">
                            <div class="ops-panel__header">
                                <div>
                                    <h2 id="escalations-title">"Live escalations"</h2>
                                    <p>
                                        "Rows are server-rendered; controls and row menus are rsjs islands."
                                    </p>
                                </div>
                                <span class="ops-count">{self.data.tickets.len()}</span>
                            </div>
                            <div class="ops-table-wrap">
                                <table class="ops-table">
                                    <thead>
                                        <tr>
                                            <th>"Ticket"</th>
                                            <th>"Account"</th>
                                            <th>"Owner"</th>
                                            <th>"Priority"</th>
                                            <th>"Status"</th>
                                            <th>"Updated"</th>
                                            <th>"Value"</th>
                                            <th>"Actions"</th>
                                        </tr>
                                    </thead>
                                    <tbody>
                                        {for ticket in self.data.tickets {
                                            render_ticket_row(cx, ticket);
                                        }}
                                    </tbody>
                                </table>
                            </div>
                        </section>
                    </main>
                </body>
            </html>
        }
        .render(cx);
        Ok(())
    }
}

fn render_kpi<B: WriteBuf, L: DataLoader>(cx: &mut WebContext<L, B>, kpi: &Kpi<'_>) {
    let _ = html_sync! {
        <article class="ops-kpi">
            <span>{kpi.label}</span>
            <strong>{kpi.value}</strong>
            <em>{kpi.trend}</em>
        </article>
    }
    .render(cx);
}

fn render_ticket_row<B: WriteBuf, L: DataLoader>(cx: &mut WebContext<L, B>, ticket: &Ticket<'_>) {
    let row_class = if ticket.stale {
        "ops-row ops-row--stale"
    } else {
        "ops-row"
    };
    let _ = html_sync! {
        <tr class={row_class}>
            <td>
                <a class="ops-ticket" href="#">{ticket.id}</a>
                <span>{ticket.title}</span>
            </td>
            <td>{ticket.account}</td>
            <td>{ticket.owner}</td>
            <td>
                <span class={ticket.priority.class()}>{ticket.priority.as_str()}</span>
            </td>
            <td>
                <span class={ticket.status.class()}>{ticket.status.as_str()}</span>
            </td>
            <td>{ticket.updated}</td>
            <td>{ticket.value}</td>
            <td>
                {RowActions {
                    ticket_id: ticket.id,
                    account: ticket.account,
                    owner: ticket.owner,
                    initially_acknowledged: ticket.status == TicketStatus::Resolved,
                }}
            </td>
        </tr>
    }
    .render(cx);
}

struct ServerPill<'a> {
    label: &'a str,
}

impl<L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for ServerPill<'_> {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        html_sync! {
            <span class="ops-server-pill">{self.label}</span>
        }
        .render(cx)
    }
}

struct ServerCount {
    count: usize,
}

impl<L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for ServerCount {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        html_sync! {
            <span class="ops-server-pill">{self.count}" total"</span>
        }
        .render(cx)
    }
}

pub struct DashboardControls<'a> {
    pub initial_query: &'a str,
    pub initial_urgent_only: bool,
    pub initial_show_resolved: bool,
    pub result_count: usize,
}

#[rsjs(component, client)]
impl<'a, L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for DashboardControls<'a> {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let query = signal(self.initial_query);
        let urgent_only = signal(self.initial_urgent_only);
        let show_resolved = signal(self.initial_show_resolved);
        let advanced_open = signal(false);
        let applied = signal(false);
        let result_count = ServerCount {
            count: self.result_count,
        };
        html_sync! {
            <form class="ops-controls" role="search" on_submit={|_| applied.set(true)}>
                <div class="ops-controls__search">
                    <label for="ops-search">"Search accounts or tickets"</label>
                    <input
                        id="ops-search"
                        type="search"
                        placeholder="Account, owner, ticket..."
                        bind_value={query}
                    />
                </div>
                <label class="ops-check">
                    <input type="checkbox" bind_checked={urgent_only} />
                    <span>"Urgent only"</span>
                </label>
                <label class="ops-check">
                    <input type="checkbox" bind_checked={show_resolved} />
                    <span>"Show resolved"</span>
                </label>
                <button
                    type="button"
                    class="ops-button"
                    aria-expanded={advanced_open.get()}
                    on_click={|_| advanced_open.update(|v| !v)}
                >
                    "Advanced"
                </button>
                <button
                    type="submit"
                    class="ops-button ops-button--primary"
                    disabled={applied.get()}
                >
                    "Apply"
                </button>
                <div class="ops-controls__summary">
                    <span>"Query"</span>
                    <strong>{query.get()}</strong>
                </div>
                <div class="ops-controls__status">
                    {if applied.get() {
                        html_sync! {
                            <p>"Filters applied to the current view."</p>
                        }
                    } else {
                        html_sync! {
                            <p>"Editing a draft filter set."</p>
                        }
                    }}
                </div>
                <div class="ops-controls__advanced">
                    {if advanced_open.get() {
                        html_sync! {
                            <section class="ops-advanced">
                                <h2>"Advanced filters"</h2>
                                <p>"This panel is client-toggled but its count is server-provided."</p>
                                <span>"Matching rows: "</span>
                                <strong>{result_count}</strong>
                            </section>
                        }
                    }}
                </div>
            </form>
        }
        .render(cx)
    }
}

pub struct RowActions<'a> {
    pub ticket_id: &'a str,
    pub account: &'a str,
    pub owner: &'a str,
    pub initially_acknowledged: bool,
}

#[rsjs(component, client)]
impl<'a, L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for RowActions<'a> {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let menu_open = signal(false);
        let acknowledged = signal(self.initially_acknowledged);
        let ticket = ServerPill {
            label: self.ticket_id,
        };
        let account = ServerPill {
            label: self.account,
        };
        let owner = ServerPill { label: self.owner };
        html_sync! {
            <div class="ops-row-actions">
                <button
                    type="button"
                    class="ops-icon-button"
                    aria-label="Toggle row actions"
                    aria-expanded={menu_open.get()}
                    on_click={|_| menu_open.update(|v| !v)}
                >
                    "..."
                </button>
                <div>
                    {if acknowledged.get() {
                        html_sync! {
                            <span class="ops-ack">"Acked"</span>
                        }
                    } else {
                        html_sync! {
                            <button type="button" class="ops-link-button"
                                    on_click={|_| acknowledged.set(true)}>
                                "Acknowledge"
                            </button>
                        }
                    }}
                </div>
                <div>
                    {if menu_open.get() {
                        html_sync! {
                            <aside class="ops-menu">
                                <p>{ticket}</p>
                                <p>{account}</p>
                                <p>{owner}</p>
                                <button type="button">"Assign"</button>
                                <button type="button">"Snooze"</button>
                            </aside>
                        }
                    }}
                </div>
            </div>
        }
        .render(cx)
    }
}

const CSS: &str = r#"
:root {
  color-scheme: light;
  font-family: Inter, ui-sans-serif, system-ui, -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif;
  background: #f6f7f9;
  color: #17202a;
}
* { box-sizing: border-box; }
body { margin: 0; background: #f6f7f9; }
button, input { font: inherit; }
.ops-shell { width: min(1180px, calc(100% - 32px)); margin: 0 auto; padding: 28px 0 48px; }
.ops-header { display: flex; justify-content: space-between; align-items: center; gap: 24px; margin-bottom: 22px; }
.ops-eyebrow { margin: 0 0 6px; color: #637083; font-size: 13px; font-weight: 700; text-transform: uppercase; }
h1 { margin: 0; font-size: 30px; line-height: 1.1; letter-spacing: 0; }
.ops-header__actions { display: flex; gap: 8px; align-items: center; }
.ops-header__actions a, .ops-header__actions button, .ops-button {
  border: 1px solid #ccd3dd; background: #fff; color: #17202a; text-decoration: none; border-radius: 6px;
  padding: 9px 12px; font-weight: 700; cursor: pointer;
}
.ops-button--primary { background: #1d4ed8; color: #fff; border-color: #1d4ed8; }
.ops-kpis { display: grid; grid-template-columns: repeat(4, minmax(0, 1fr)); gap: 12px; margin-bottom: 14px; }
.ops-kpi, .ops-panel, .ops-controls {
  background: #fff; border: 1px solid #dfe4eb; border-radius: 8px; box-shadow: 0 1px 2px rgba(15, 23, 42, 0.05);
}
.ops-kpi { padding: 16px; display: grid; gap: 7px; }
.ops-kpi span, .ops-panel__header p, .ops-controls label, .ops-controls__summary span { color: #687486; font-size: 13px; }
.ops-kpi strong { font-size: 26px; }
.ops-kpi em { color: #116149; font-style: normal; font-size: 13px; }
.ops-controls { display: grid; grid-template-columns: minmax(260px, 1fr) auto auto auto auto; gap: 12px; align-items: end; padding: 14px; margin-bottom: 14px; }
.ops-controls__search { display: grid; gap: 6px; }
.ops-controls input[type="search"] { width: 100%; border: 1px solid #ccd3dd; border-radius: 6px; padding: 10px 11px; }
.ops-check { display: flex; align-items: center; gap: 7px; height: 39px; white-space: nowrap; }
.ops-controls__summary, .ops-controls__status, .ops-controls__advanced { grid-column: 1 / -1; }
.ops-controls__summary { display: flex; gap: 8px; align-items: center; min-height: 22px; }
.ops-controls__status p { margin: 0; color: #536070; }
.ops-advanced { border-top: 1px solid #e4e8ef; margin-top: 4px; padding-top: 12px; }
.ops-advanced h2 { margin: 0 0 6px; font-size: 15px; }
.ops-advanced p { margin: 0 0 8px; color: #536070; }
.ops-panel { overflow: visible; }
.ops-panel__header { display: flex; justify-content: space-between; gap: 16px; align-items: center; padding: 16px; border-bottom: 1px solid #e4e8ef; }
.ops-panel__header h2 { margin: 0; font-size: 18px; }
.ops-panel__header p { margin: 5px 0 0; }
.ops-count { background: #eef2ff; color: #1d4ed8; border-radius: 999px; padding: 6px 10px; font-weight: 800; }
.ops-table-wrap { overflow-x: auto; }
.ops-table { width: 100%; border-collapse: collapse; font-size: 14px; }
.ops-table th { color: #687486; font-size: 12px; text-align: left; padding: 11px 12px; text-transform: uppercase; border-bottom: 1px solid #e4e8ef; }
.ops-table td { padding: 13px 12px; border-bottom: 1px solid #edf0f4; vertical-align: top; }
.ops-row--stale { background: #fff8ed; }
.ops-ticket { display: block; color: #1d4ed8; font-weight: 800; text-decoration: none; margin-bottom: 4px; }
.ops-pill, .ops-priority, .ops-ack, .ops-server-pill {
  display: inline-flex; align-items: center; border-radius: 999px; padding: 4px 8px; font-size: 12px; font-weight: 800;
}
.ops-pill--open { background: #fee2e2; color: #991b1b; }
.ops-pill--watching { background: #fef3c7; color: #854d0e; }
.ops-pill--resolved { background: #dcfce7; color: #166534; }
.ops-priority--low { background: #edf2f7; color: #475569; }
.ops-priority--medium { background: #e0f2fe; color: #075985; }
.ops-priority--high { background: #ffedd5; color: #9a3412; }
.ops-priority--critical { background: #ffe4e6; color: #9f1239; }
.ops-row-actions { position: relative; display: flex; align-items: center; gap: 8px; }
.ops-icon-button, .ops-link-button { border: 1px solid #ccd3dd; background: #fff; border-radius: 6px; min-height: 30px; padding: 4px 8px; cursor: pointer; }
.ops-link-button { color: #1d4ed8; font-weight: 800; }
.ops-ack { background: #dcfce7; color: #166534; }
.ops-menu {
  position: absolute; z-index: 20; top: 36px; right: 0; width: 190px; display: grid; gap: 7px;
  background: #fff; border: 1px solid #ccd3dd; border-radius: 8px; padding: 10px; box-shadow: 0 12px 28px rgba(15, 23, 42, 0.16);
}
.ops-menu p { margin: 0; }
.ops-menu button { text-align: left; border: 0; background: #f7f9fc; border-radius: 5px; padding: 7px; cursor: pointer; }
@media (max-width: 860px) {
  .ops-header, .ops-panel__header { align-items: flex-start; flex-direction: column; }
  .ops-kpis { grid-template-columns: repeat(2, minmax(0, 1fr)); }
  .ops-controls { grid-template-columns: 1fr; align-items: stretch; }
}
@media (max-width: 520px) {
  .ops-kpis { grid-template-columns: 1fr; }
}
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dashboard_renders_server_table_and_islands() {
        let html = render();
        assert_eq!(html.matches("<tr class=").count(), 6);
        assert_eq!(
            html.matches(r#"<rsjs-island data-rsjs-island-root="rowactions_"#)
                .count(),
            6
        );
        assert_eq!(
            html.matches(r#"<rsjs-island data-rsjs-island-root="dashboardcontrols_"#)
                .count(),
            1
        );
        assert!(html.contains(r#"data-rsjs-input-value"#));
        assert!(html.contains(r#"data-rsjs-input-checked"#));
        assert!(html.contains(r#"data-rsjs-attr"#));
        assert!(html.contains(r#"aria-expanded="false""#));
    }
}
