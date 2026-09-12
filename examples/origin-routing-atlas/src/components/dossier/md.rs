// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Markdown representation of the dossier: the machine-readable document
//! served at `/site/<host>.md` and via Accept negotiation, rendered from the
//! same data view the HTML panes consume.

use std::sync::Arc;

use proa_core::{md::MdContext, MdRenderSync, WriteBuf, WriteError};
use proa_macros::md_sync;

use crate::model::{site_for, Dossier, Site};

pub struct DossierMd {
    pub query: String,
    pub dossier: Option<Arc<Dossier>>,
}

impl DossierMd {
    fn site(&self) -> Option<&'static Site> {
        let host = self
            .dossier
            .as_ref()
            .and_then(|d| d.subject.domain.as_deref())
            .unwrap_or(self.query.as_str());
        site_for(host)
    }
}

struct LiveDossierMd<'a> {
    host: &'a str,
    dossier: &'a Dossier,
    site: Option<&'static Site>,
}

// Resources render as inline code spans via typed `cx.code` composition; the
// interleaved static list/table bytes cannot be expressed in `md_sync!`
// because slots inside backticks stay literal.
// proa-perf-ok(scope): typed MdContext composition for inline code spans.
impl<B: WriteBuf> MdRenderSync<B> for LiveDossierMd<'_> {
    fn render_md(self, cx: &mut MdContext<'_, B>) -> Result<(), WriteError> {
        // proa-perf-ok(scope): a handful of short row strings per markdown
        // response on a guided demo page.
        let dossier = self.dossier;
        let address = dossier.subject.address.as_deref().unwrap_or("(no address)");
        let v4 = dossier
            .prefixes
            .iter()
            .filter(|p| !p.prefix.contains(':'))
            .count();
        let v6 = dossier.prefixes.len() - v4;
        // proa-perf-ok: two short strings per markdown response.
        let (v4, v6) = if dossier.prefixes.is_empty() {
            ("unavailable".to_string(), "unavailable".to_string())
        } else {
            (v4.to_string(), v6.to_string())
        };
        let last_change: String = dossier.routing.last_seen.chars().take(10).collect();
        let ranges: Vec<(String, String)> = dossier
            .prefixes
            .iter()
            .take(6)
            .map(|p| (p.prefix.clone(), p.since.chars().take(10).collect()))
            .collect();
        let carriers: Vec<(u32, &'static str, u64)> = {
            let mut sorted: Vec<_> = dossier.neighbours.iter().collect();
            sorted.sort_by_key(|n| std::cmp::Reverse(n.v4_peers + n.v6_peers));
            sorted
                .iter()
                .take(8)
                .map(|n| {
                    (
                        n.asn,
                        match n.relation.as_str() {
                            "left" => "upstream in paths",
                            "right" => "downstream in paths",
                            _ => "adjacent",
                        },
                        n.v4_peers + n.v6_peers,
                    )
                })
                .collect()
        };
        let routes: Vec<(String, String)> = {
            let mut seen_collectors: Vec<&str> = Vec::new();
            let mut selected = Vec::new();
            for path in &dossier.paths {
                let rrc = path.collector.split('-').next().unwrap_or("");
                if seen_collectors.contains(&rrc) {
                    continue;
                }
                seen_collectors.push(rrc);
                selected.push(path);
                if selected.len() == 8 {
                    break;
                }
            }
            selected
                .iter()
                .map(|path| {
                    (
                        crate::model::collector_label(&path.collector),
                        crate::model::format_hops(&path.hops)
                            .iter()
                            .map(|hop| format!("AS{hop}"))
                            .collect::<Vec<_>>()
                            .join(" → "),
                    )
                })
                .collect()
        };
        let unusual_row = match self.site {
            Some(site) if site.unusual => format!("yes — {}", site.unusual_detail),
            Some(site) => format!("no — {}", site.unusual_detail),
            None => "none observed in the current RIS window".to_string(),
        };
        // Hostnames, addresses, and prefixes go through `cx.code` so they
        // arrive as inline code spans instead of backslash-escaped prose
        // (template slots inside backticks stay literal by design).
        cx.heading_with(1, |cx| {
            cx.code(self.host)?;
            cx.write_text(" — network dossier")
        })?;
        cx.block_boundary()?;
        md_sync! { r###"
            ## How the internet reaches it
            "###
        }
        .render_md(cx)?;
        cx.block_boundary()?;
        cx.write_prepared_static(b"1. DNS resolves ")?;
        cx.code(self.host)?;
        cx.write_prepared_static(b" to ")?;
        cx.code(address)?;
        cx.write_prepared_static(b"\n2. That address sits inside ")?;
        cx.code(&dossier.subject.prefix)?;
        cx.write_prepared_static(b"\n3. That range is announced by AS")?;
        cx.write_text(&dossier.subject.asn.to_string())?;
        cx.write_prepared_static(b"\n")?;
        cx.block_boundary()?;
        md_sync! { r###"
            ## Summary

            | question | answer |
            | --- | --- |
            | who operates it | {dossier.display_name()} |
            | reachable | {dossier.routing.seen_by} of {dossier.routing.collectors} collectors |
            | ownership verified | RPKI {dossier.rpki_status.as_str()} |
            | anything unusual | {unusual_row.as_str()} |
            "###
        }
        .render_md(cx)?;
        cx.block_boundary()?;
        md_sync! { r###"
            ## At a glance

            | metric | value |
            | --- | --- |
            | IPv4 ranges announced | {v4.as_str()} |
            | IPv6 ranges announced | {v6.as_str()} |
            | neighbours | {dossier.neighbours.len()} |
            | RPKI (queried prefix) | {dossier.rpki_status.as_str()} |
            | visibility | {dossier.routing.seen_by} of {dossier.routing.collectors} RIS peers |
            | last seen | {last_change.as_str()} (RIS window) |

            ## Announced ranges (first {ranges.len()} of {dossier.prefixes.len()})
            "###
        }
        .render_md(cx)?;
        cx.block_boundary()?;
        if dossier.prefixes.is_empty() {
            md_sync! { r###"
                Range list unavailable: it exceeded this page's data budget.
                "###
            }
            .render_md(cx)?;
        } else {
            cx.write_prepared_static(b"| prefix | observed |\n| --- | --- |\n")?;
            for (prefix, since) in &ranges {
                cx.write_prepared_static(b"| ")?;
                cx.code(prefix)?;
                cx.write_prepared_static(b" | ")?;
                cx.write_text(since)?;
                cx.write_prepared_static(b" |\n")?;
            }
        }
        cx.block_boundary()?;
        md_sync! { r###"
            ## Who carries its traffic

            {for (asn, relation, sessions) in carriers.iter() {
            - AS{*asn} ({*relation}, seen by {*sessions} RIS peers)
            }}

            ## Routes traffic takes
            "###
        }
        .render_md(cx)?;
        cx.block_boundary()?;
        for (collector, hops) in &routes {
            cx.write_prepared_static(b"- ")?;
            cx.code(collector)?;
            cx.write_prepared_static(b": ")?;
            cx.write_text(hops)?;
            cx.write_prepared_static(b"\n")?;
        }
        cx.block_boundary()?;
        if let Some(site) = self.site {
            md_sync! { r###"
                ## Open routing situations

                {if site.unusual {**Status:** open · **Severity:** high} else {None reported.}}

                {site.unusual_detail}
                "###
            }
            .render_md(cx)?;
        }
        cx.block_boundary()
    }
}

struct CuratedDossierMd {
    site: &'static Site,
}

impl<B: WriteBuf> MdRenderSync<B> for CuratedDossierMd {
    fn render_md(self, cx: &mut MdContext<'_, B>) -> Result<(), WriteError> {
        let site = self.site;
        // Same heading shape and row wording as the live dossier, so readers
        // (and tests) see one format regardless of which path served them.
        cx.heading_with(1, |cx| {
            cx.code(site.host)?;
            cx.write_text(" — network dossier")
        })?;
        cx.block_boundary()?;
        md_sync! { r###"
            RIPEstat was unreachable; this is the curated dossier.

            - operator: {site.operator}
            - network: {site.asn_line}
            - anything unusual: {site.unusual_detail}
            "###
        }
        .render_md(cx)
    }
}

impl<B: WriteBuf> MdRenderSync<B> for DossierMd {
    fn render_md(self, cx: &mut MdContext<'_, B>) -> Result<(), WriteError> {
        cx.block_boundary()?;
        let site = self.site();
        match (&self.dossier, site) {
            (Some(dossier), site) => {
                let host = dossier
                    .subject
                    .domain
                    .clone()
                    .unwrap_or_else(|| dossier.subject.query.clone());
                LiveDossierMd {
                    host: host.as_str(),
                    dossier,
                    site,
                }
                .render_md(cx)?;
            }
            (None, Some(site)) => CuratedDossierMd { site }.render_md(cx)?,
            (None, None) => {
                cx.heading_with(1, |cx| cx.code(self.query.as_str()))?;
                cx.block_boundary()?;
                md_sync! { r###"
                    This name did not resolve, or RIPEstat was unreachable. Check the
                    spelling, or retry shortly.
                    "###
                }
                .render_md(cx)?;
            }
        }
        md_sync! { r###"
            ## Agent tools on this route

            - lookup_site(hostname)
            - inspect_prefix(prefix)
            - validate_route(prefix, origin)
            - compare_networks(a, b)
            - watch_announcements(resource)
            "###
        }
        .render_md(cx)?;
        cx.block_boundary()
    }
}
