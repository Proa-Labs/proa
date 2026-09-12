// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use proa_core::{DataLoader, WebContext, WebRenderSync, WriteError};
use proa_macros::html_sync;
use rsjs::{rsjs, signal};

use super::recipe::base;

const TOOLS: &[(&str, &str)] = &[
    (
        "lookup_site(hostname)",
        "resolve a name to its network dossier",
    ),
    (
        "inspect_prefix(prefix)",
        "who announces a range and since when",
    ),
    (
        "validate_route(prefix, origin)",
        "RPKI check for one announcement",
    ),
    ("compare_networks(a, b)", "two dossiers, side by side"),
    (
        "watch_announcements(resource)",
        "follow changes to one resource",
    ),
];

/// The agent surface as a first-class section: the same page, served as
/// markdown, with the exact requests that fetch it. Everything shown here is
/// live: the routes exist on this server and the header overlay previews the
/// same document.
pub struct AgentSurface {
    pub query: String,
    /// The request's Host header when known; the golden path renders
    /// without one and keeps a placeholder.
    pub host: Option<String>,
}

/// Percent-encode a query for embedding in the copyable command: anything
/// outside the RFC 3986 unreserved set is encoded, so a crafted lookup can
/// never smuggle shell syntax into a pasted command line.
// proa-perf-ok(scope): one short string per request.
pub(crate) fn encode_query(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    for byte in raw.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                out.push(byte as char)
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

/// Only a plausible host (letters, digits, dots, dashes, colons, brackets)
/// is echoed into the command; anything else keeps the placeholder, so a
/// forwarded Host header cannot inject shell syntax either.
fn plausible_host(host: &str) -> bool {
    !host.is_empty()
        && host
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b':' | b'[' | b']'))
}

/// `http://` for local hosts, `https://` otherwise; placeholder when the
/// request did not say.
// proa-perf-ok(scope): one short string per request.
pub(crate) fn base_url(host: &Option<String>) -> String {
    match host {
        Some(h) if !plausible_host(h) => "https://<this-host>".to_string(),
        Some(h) if h.starts_with("localhost") || h.starts_with("127.") => {
            format!("http://{h}")
        }
        Some(h) => format!("https://{h}"),
        None => "https://<this-host>".to_string(),
    }
}

impl<L: DataLoader> WebRenderSync<L> for AgentSurface {
    fn render(self, cx: &mut WebContext<L>) -> Result<(), WriteError> {
        // proa-perf-ok(scope): a few short strings once per request.
        let md_path = format!("/site/{}.md", self.query); // proa-perf-ok: a few short strings once per request.
                                                          // proa-perf-ok: a few short strings once per request.
        let command = format!(
            "curl -H 'Accept: text/markdown' {}/?q={}",
            base_url(&self.host),
            encode_query(&self.query)
        );
        let curl_accept = format!("$ {command}"); // proa-perf-ok: a few short strings once per request.
        html_sync! {
            <section
                id="agents"
                data-agent-surface
                data-screen-label="For agents"
                class="grid grid-cols-[1fr_420px] border-b border-line"
            >
                <div class="pt-[32px] px-[22px] pb-[36px] border-r border-line">
                    <div class="text-[10px] tracking-[0.18em] text-ink4 uppercase">
                        "The other rendering"
                    </div>
                    <h2 class="font-display text-[70px] font-bold tracking-[-0.05em] mt-[14px] mx-0 mb-0 leading-[0.9]">
                        "ONE PAGE,"
                        <br />
                        "TWO READERS"
                    </h2>
                    <p class="max-w-[58ch] mt-[24px] mx-0 mb-0 text-[12px] leading-[1.75] text-ink3 text-pretty">
                        "Every component on this page renders twice from the same Rust: the HTML you are reading, and a markdown dossier for agents and pipelines. No second template, no scraper, no drift. Ask for it with a header, or fetch the document route directly:"
                    </p>
                    <div class="grid gap-[8px] max-w-[62ch] mt-[20px]">
                        <div class="group relative">
                            <code class={base::CURL_LINE}>{curl_accept.as_str()}</code>
                            {CopyCommand { command }}
                        </div>
                    </div>
                    <div class="mt-[16px] text-[11px]">
                        <a
                            href={md_path.as_str()}
                            class="hv-cyan text-cyan tracking-[0.08em]"
                        >
                            "open this lookup as markdown →"
                        </a>
                    </div>
                </div>
                <div class="pt-[32px] px-[20px] pb-[36px]">
                    <div class="text-[9px] tracking-[0.18em] text-ink4 uppercase mb-[10px]">
                        "Agent tools on this route"
                    </div>
                    {ToolRows}
                    <p class="mt-[16px] mx-0 mb-0 text-[10px] leading-[1.7] text-ink4">
                        "The markdown document lists the same tools, so an agent that fetches one lookup knows every follow-up it can make."
                    </p>
                </div>
            </section>
        }
        .render(cx)
    }
}

struct ToolRows;

impl<L: DataLoader> WebRenderSync<L> for ToolRows {
    fn render(self, cx: &mut WebContext<L>) -> Result<(), WriteError> {
        for (signature, what) in TOOLS {
            html_sync! {
                <div class={base::TOOL_ROW}>
                    <code class="text-cyan whitespace-nowrap">{*signature}</code>
                    <span class="text-ink3">{*what}</span>
                </div>
            }
            .render(cx)?;
        }
        Ok(())
    }
}

/// Hover-revealed copy for the curl line; clipboard access needs a typed
/// browser API, so this is a small island. The copied text omits the `$ `
/// prompt so it pastes ready to run.
struct CopyCommand {
    command: String,
}

// proa-perf-ok(scope): island signals take owned initial values; the one
// clone per render seeds the client-side command payload.
#[rsjs(component, client)]
impl<L: DataLoader> WebRenderSync<L> for CopyCommand {
    fn render(self, cx: &mut WebContext<L>) -> Result<(), WriteError> {
        let command = signal(self.command.clone());
        let copied = signal(false);
        html_sync! {
            <button
                type="button"
                aria-label="Copy command"
                class="hv-cyan absolute right-[6px] top-1/2 -translate-y-1/2 opacity-0 group-hover:opacity-100 focus-visible:opacity-100 transition-opacity border border-line-strong bg-bg2 [font-family:inherit] py-[3px] px-[9px] cursor-pointer text-ink text-[9px] tracking-[0.14em] uppercase"
                data-curl-copy
                on_click={|_| { rsjs::clipboard::write_text(command.get()); copied.set(true); }}
            >
                {if copied.get() { html_sync! {
                    <span>"Copied"</span>
                } } else { html_sync! {
                    <span>"Copy"</span>
                } }}
            </button>
        }
        .render(cx)
    }
}
