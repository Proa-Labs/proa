// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Product-app stress page for stateful attributes in RSJS-analyzed components.
//!
//! This page intentionally leans on dynamic `class`, `data-state`,
//! ARIA state, and boolean attributes. It is a companion to
//! `ops_dashboard`: that page found the missing reactive-attribute
//! surface; this one checks whether the new surface is ergonomic for
//! tabbed/settings-style UI.

use proa_core::{DataLoader, StaticRaw, WebContext, WebRenderSync, WriteBuf, WriteError};
use proa_macros::html_sync;
#[allow(unused_imports)]
use rsjs::signal;
use rsjs_macros::rsjs;

#[derive(Debug, Clone, Copy)]
pub struct AuditEvent<'a> {
    pub area: &'a str,
    pub actor: &'a str,
    pub action: &'a str,
    pub age: &'a str,
}

pub const AUDIT_EVENTS: &[AuditEvent<'static>] = &[
    AuditEvent {
        area: "Access",
        actor: "Mina",
        action: "Rotated admin recovery codes",
        age: "2m ago",
    },
    AuditEvent {
        area: "Billing",
        actor: "Diego",
        action: "Updated invoice approvers",
        age: "18m ago",
    },
    AuditEvent {
        area: "Security",
        actor: "Priya",
        action: "Enabled SSO enforcement",
        age: "41m ago",
    },
];

pub fn render() -> String {
    // proa-perf-ok: standalone test/debug helper; route handlers render with their own response buffer.
    let mut out = Vec::new();
    let mut cx = WebContext::with_buffer(&mut out);
    let _ = SettingsConsole {
        audit_events: AUDIT_EVENTS,
    }
    .render(&mut cx);
    String::from_utf8(out).unwrap_or_default()
}

pub struct SettingsConsole<'a> {
    pub audit_events: &'a [AuditEvent<'a>],
}

impl<B: WriteBuf, L: DataLoader> WebRenderSync<L, B> for SettingsConsole<'_> {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        html_sync! {
            <!DOCTYPE html>
            <html lang="en">
                <head>
                    <meta charset="utf-8" />
                    <meta name="viewport" content="width=device-width, initial-scale=1" />
                    <title>"Settings Console Stress Page"</title>
                    <style>{StaticRaw(CSS)}</style>
                </head>
                <body>
                    <main class="settings-shell">
                        <header class="settings-header">
                            <div>
                                <p>"Admin workspace"</p>
                                <h1>"Settings console"</h1>
                            </div>
                            <a href="/showcase">"Showcases"</a>
                        </header>
                        {SettingsPanel { audit_events: self.audit_events }}
                    </main>
                </body>
            </html>
        }
        .render(cx)
    }
}

pub struct SettingsPanel<'a> {
    pub audit_events: &'a [AuditEvent<'a>],
}

struct AuditList<'a> {
    events: &'a [AuditEvent<'a>],
}

impl<B: WriteBuf, L: DataLoader> WebRenderSync<L, B> for AuditList<'_> {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        html_sync! {
            <ul>
                {for event in self.events {
                    let _ = html_sync! {
                        <li>
                            <span>{event.area}</span>
                            <strong>{event.action}</strong>
                            <em>{event.actor}" · "{event.age}</em>
                        </li>
                    }.render(cx);
                }}
            </ul>
        }
        .render(cx)
    }
}

#[rsjs(component, client)]
impl<'a, B: WriteBuf, L: DataLoader> WebRenderSync<L, B> for SettingsPanel<'a> {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let nav_open = signal(true);
        let nav_state = signal("expanded");
        let nav_class = signal("settings-nav settings-nav--expanded");

        let overview_class = signal("settings-tab settings-tab--active");
        let billing_class = signal("settings-tab");
        let security_class = signal("settings-tab");
        let overview_state = signal("active");
        let billing_state = signal("inactive");
        let security_state = signal("inactive");
        let overview_selected = signal(true);
        let billing_selected = signal(false);
        let security_selected = signal(false);
        let panel_title = signal("Workspace overview");

        let row_one_class = signal("settings-row settings-row--selected");
        let row_two_class = signal("settings-row");
        let row_three_class = signal("settings-row");
        let row_one_state = signal("selected");
        let row_two_state = signal("idle");
        let row_three_state = signal("idle");
        let selected_area = signal("Access");

        let changes_ready = signal(false);
        let audit_list = AuditList {
            events: self.audit_events,
        };

        html_sync! {
            <section class="settings-layout">
                <aside class={nav_class.get()} data_state={nav_state.get()}>
                    <div class="settings-nav__head">
                        <strong>"Configuration"</strong>
                        <button
                            type="button"
                            aria-expanded={nav_open.get()}
                            data_state={nav_state.get()}
                            on_click={|_| {
                                    nav_open.set(false);
                                    nav_state.set("collapsed");
                                    nav_class.set("settings-nav settings-nav--collapsed");
                                }}
                        >
                            "Compact"
                        </button>
                        <button
                            type="button"
                            aria-expanded={nav_open.get()}
                            data_state={nav_state.get()}
                            on_click={|_| {
                                    nav_open.set(true);
                                    nav_state.set("expanded");
                                    nav_class.set("settings-nav settings-nav--expanded");
                                }}
                        >
                            "Expand"
                        </button>
                    </div>
                    <nav aria-label="Settings sections">
                        <a href="#workspace">"Workspace"</a>
                        <a href="#billing">"Billing"</a>
                        <a href="#security">"Security"</a>
                    </nav>
                </aside>
                <article class="settings-panel">
                    <div class="settings-tabs" role="tablist" aria-label="Settings groups">
                        <button
                            type="button"
                            role="tab"
                            class={overview_class.get()}
                            data_state={overview_state.get()}
                            aria_selected={overview_selected.get()}
                            on_click={|_| {
                                    overview_class.set("settings-tab settings-tab--active");
                                    billing_class.set("settings-tab");
                                    security_class.set("settings-tab");
                                    overview_state.set("active");
                                    billing_state.set("inactive");
                                    security_state.set("inactive");
                                    overview_selected.set(true);
                                    billing_selected.set(false);
                                    security_selected.set(false);
                                    panel_title.set("Workspace overview");
                                }}
                        >
                            "Overview"
                        </button>
                        <button
                            type="button"
                            role="tab"
                            class={billing_class.get()}
                            data_state={billing_state.get()}
                            aria_selected={billing_selected.get()}
                            on_click={|_| {
                                    overview_class.set("settings-tab");
                                    billing_class.set("settings-tab settings-tab--active");
                                    security_class.set("settings-tab");
                                    overview_state.set("inactive");
                                    billing_state.set("active");
                                    security_state.set("inactive");
                                    overview_selected.set(false);
                                    billing_selected.set(true);
                                    security_selected.set(false);
                                    panel_title.set("Billing controls");
                                }}
                        >
                            "Billing"
                        </button>
                        <button
                            type="button"
                            role="tab"
                            class={security_class.get()}
                            data_state={security_state.get()}
                            aria_selected={security_selected.get()}
                            on_click={|_| {
                                    overview_class.set("settings-tab");
                                    billing_class.set("settings-tab");
                                    security_class.set("settings-tab settings-tab--active");
                                    overview_state.set("inactive");
                                    billing_state.set("inactive");
                                    security_state.set("active");
                                    overview_selected.set(false);
                                    billing_selected.set(false);
                                    security_selected.set(true);
                                    panel_title.set("Security posture");
                                }}
                        >
                            "Security"
                        </button>
                    </div>
                    <section class="settings-card" role="tabpanel">
                        <div class="settings-card__head">
                            <div>
                                <span>"Current group"</span>
                                <h2>{panel_title.get()}</h2>
                            </div>
                            <button
                                type="button"
                                class="settings-secondary"
                                on_click={|_| changes_ready.set(true)}
                            >
                                "Mark unsaved changes"
                            </button>
                            <button
                                type="button"
                                class="settings-primary"
                                disabled={!changes_ready.get()}
                            >
                                "Save changes"
                            </button>
                        </div>
                        <div class="settings-grid">
                            <button
                                type="button"
                                class={row_one_class.get()}
                                data_state={row_one_state.get()}
                                on_click={|_| {
                                        row_one_class.set("settings-row settings-row--selected");
                                        row_two_class.set("settings-row");
                                        row_three_class.set("settings-row");
                                        row_one_state.set("selected");
                                        row_two_state.set("idle");
                                        row_three_state.set("idle");
                                        selected_area.set("Access");
                                    }}
                            >
                                <strong>"Access"</strong>
                                <span>"Roles, approvals, and recovery controls"</span>
                            </button>
                            <button
                                type="button"
                                class={row_two_class.get()}
                                data_state={row_two_state.get()}
                                on_click={|_| {
                                        row_one_class.set("settings-row");
                                        row_two_class.set("settings-row settings-row--selected");
                                        row_three_class.set("settings-row");
                                        row_one_state.set("idle");
                                        row_two_state.set("selected");
                                        row_three_state.set("idle");
                                        selected_area.set("Billing");
                                    }}
                            >
                                <strong>"Billing"</strong>
                                <span>"Invoice routing and spend limits"</span>
                            </button>
                            <button
                                type="button"
                                class={row_three_class.get()}
                                data_state={row_three_state.get()}
                                on_click={|_| {
                                        row_one_class.set("settings-row");
                                        row_two_class.set("settings-row");
                                        row_three_class.set("settings-row settings-row--selected");
                                        row_one_state.set("idle");
                                        row_two_state.set("idle");
                                        row_three_state.set("selected");
                                        selected_area.set("Security");
                                    }}
                            >
                                <strong>"Security"</strong>
                                <span>"SAML, SCIM, session policy, and device trust"</span>
                            </button>
                        </div>
                        <div class="settings-detail">
                            <h3>"Selected area"</h3>
                            <p>{selected_area.get()}</p>
                            {audit_list}
                        </div>
                    </section>
                </article>
            </section>
        }
        .render(cx)
    }
}

const CSS: &str = r#"
:root {
  color-scheme: light;
  font-family: Inter, ui-sans-serif, system-ui, -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif;
  background: #f5f7fb;
  color: #17202a;
}
* { box-sizing: border-box; }
body { margin: 0; background: #f5f7fb; }
button { font: inherit; }
.settings-shell { width: min(1160px, calc(100% - 32px)); margin: 0 auto; padding: 28px 0 52px; }
.settings-header { display: flex; align-items: center; justify-content: space-between; gap: 20px; margin-bottom: 18px; }
.settings-header p { margin: 0 0 5px; color: #627086; font-size: 13px; font-weight: 800; text-transform: uppercase; }
.settings-header h1 { margin: 0; font-size: 30px; letter-spacing: 0; }
.settings-header a { color: #1d4ed8; font-weight: 800; text-decoration: none; }
.settings-layout { display: grid; grid-template-columns: 270px minmax(0, 1fr); gap: 16px; align-items: start; }
.settings-nav, .settings-panel, .settings-card {
  background: #fff; border: 1px solid #dfe5ef; border-radius: 8px; box-shadow: 0 1px 2px rgba(15, 23, 42, 0.05);
}
.settings-nav { padding: 14px; display: grid; gap: 14px; transition: opacity .15s ease; }
.settings-nav[data-state="collapsed"] { opacity: .72; }
.settings-nav__head { display: grid; gap: 8px; }
.settings-nav__head button, .settings-secondary, .settings-primary {
  border: 1px solid #ccd5e3; background: #fff; color: #17202a; border-radius: 6px; padding: 8px 10px; font-weight: 800; cursor: pointer;
}
.settings-primary { background: #14532d; border-color: #14532d; color: #fff; }
.settings-primary:disabled { background: #d8dee8; border-color: #d8dee8; color: #67758a; cursor: not-allowed; }
.settings-nav nav { display: grid; gap: 6px; }
.settings-nav a { color: #475569; text-decoration: none; padding: 7px 8px; border-radius: 6px; }
.settings-nav a:hover { background: #f1f5f9; }
.settings-panel { padding: 14px; }
.settings-tabs { display: flex; gap: 8px; border-bottom: 1px solid #e4e9f1; padding-bottom: 12px; margin-bottom: 14px; }
.settings-tab { border: 1px solid #d2dae7; background: #fff; color: #475569; border-radius: 6px; padding: 8px 12px; font-weight: 800; cursor: pointer; }
.settings-tab--active, .settings-tab[data-state="active"] { background: #eef6ff; border-color: #93c5fd; color: #1d4ed8; }
.settings-card { padding: 16px; }
.settings-card__head { display: grid; grid-template-columns: minmax(0, 1fr) auto auto; gap: 10px; align-items: end; margin-bottom: 16px; }
.settings-card__head span { color: #64748b; font-size: 13px; font-weight: 800; }
.settings-card__head h2 { margin: 4px 0 0; font-size: 22px; }
.settings-grid { display: grid; grid-template-columns: repeat(3, minmax(0, 1fr)); gap: 10px; }
.settings-row { text-align: left; display: grid; gap: 5px; border: 1px solid #dbe3ef; background: #fff; border-radius: 8px; padding: 13px; cursor: pointer; }
.settings-row--selected, .settings-row[data-state="selected"] { border-color: #22c55e; background: #f0fdf4; }
.settings-row span { color: #64748b; font-size: 13px; line-height: 1.35; }
.settings-detail { margin-top: 16px; border-top: 1px solid #e6ebf3; padding-top: 14px; }
.settings-detail h3 { margin: 0 0 4px; font-size: 15px; }
.settings-detail p { margin: 0 0 12px; font-weight: 900; color: #14532d; }
.settings-detail ul { list-style: none; padding: 0; margin: 0; display: grid; gap: 8px; }
.settings-detail li { display: grid; grid-template-columns: 90px minmax(0, 1fr) auto; gap: 10px; align-items: center; border: 1px solid #edf1f6; border-radius: 7px; padding: 9px; }
.settings-detail li span { color: #1d4ed8; font-weight: 900; }
.settings-detail li em { color: #64748b; font-style: normal; font-size: 13px; }
@media (max-width: 860px) {
  .settings-layout { grid-template-columns: 1fr; }
  .settings-card__head, .settings-grid, .settings-detail li { grid-template-columns: 1fr; }
  .settings-tabs { flex-wrap: wrap; }
}
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_console_renders_reactive_state_attrs() {
        let html = render();
        assert!(html.contains(r#"<rsjs-island data-rsjs-island-root="settingspanel_"#));
        assert!(html.contains(r#"class="settings-tab settings-tab--active""#));
        assert!(html.contains(r#"data-state="active""#));
        assert!(html.contains(r#"aria-selected="true""#));
        assert!(html.contains(r#"disabled"#));
        assert!(html.matches(r#"data-rsjs-attr=""#).count() >= 10);
    }
}
