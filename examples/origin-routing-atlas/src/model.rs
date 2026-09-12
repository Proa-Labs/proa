// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Curated network dossiers for the ORIGIN atlas.
//!
//! The page is a guided demo of four networks. Every value here is part of the
//! design's copy, ported verbatim from the ORIGIN v2 prototype, so the
//! server-rendered document stays deterministic and the golden test can pin it
//! byte for byte.

/// One six-tile summary metric.
#[derive(Debug, Clone, Copy)]
pub struct Metric {
    pub label: &'static str,
    pub value: &'static str,
    pub sub: &'static str,
}

/// One announced range row.
#[derive(Debug, Clone, Copy)]
pub struct CuratedPrefix {
    pub prefix: &'static str,
    pub since: &'static str,
}

impl CuratedPrefix {
    /// Whether the curated dataset marks this range as contested. No range
    /// currently is; the hook stays so a real incident can be represented.
    pub fn contested(&self) -> bool {
        false
    }
}

/// One curated network dossier.
#[derive(Debug, Clone, Copy)]
pub struct Site {
    pub host: &'static str,
    pub ip: &'static str,
    pub dns_note: &'static str,
    pub prefix: &'static str,
    pub bgp_note: &'static str,
    pub net_label: &'static str,
    pub asn: &'static str,
    pub asn_short: &'static str,
    pub asn_line: &'static str,
    pub prefix_count: &'static str,
    pub peer_count: &'static str,
    pub operator: &'static str,
    pub operator_detail: &'static str,
    pub unusual: bool,
    pub unusual_detail: &'static str,
    pub metrics: [Metric; 6],
    pub prefixes: &'static [CuratedPrefix],
}

impl Site {
    pub fn details_hint(&self) -> String {
        format!(
            "every range {} announces, who carries its traffic, and the routes in between",
            self.asn
        )
    }

    pub fn md_route(&self) -> String {
        format!("/site/{}.md", self.host)
    }
}

/// The networks this one connects to, shared across dossiers; the selected
/// site is filtered out at render time.
pub const PEERS: &[(&str, &str, bool)] = &[
    ("AS3356", "Lumen (Level 3)", true),
    ("AS174", "Cogent Communications", true),
    ("AS1299", "Arelion", true),
    ("AS2914", "NTT Global", true),
    ("AS6939", "Hurricane Electric", false),
    ("AS20940", "Akamai", false),
    ("AS13335", "Cloudflare", false),
    ("AS15169", "Google", false),
    ("AS32934", "Meta Platforms", false),
    ("AS8075", "Microsoft", false),
    ("AS9002", "RETN", false),
    ("AS6461", "Zayo Bandwidth", false),
    ("AS3257", "GTT Communications", false),
    ("AS4134", "China Telecom", false),
    ("AS5511", "Orange (Opentransit)", false),
    ("AS6762", "Sparkle (Seabone)", false),
];

/// Ring members of the neighbourhood graph: short ASN plus transit flag.
pub const TOPO_RING: &[(&str, bool)] = &[
    ("3356", true),
    ("174", true),
    ("1299", true),
    ("2914", true),
    ("6939", false),
    ("20940", false),
    ("13335", false),
    ("15169", false),
    ("9002", false),
    ("6461", false),
];

/// Well-known network operators by AS number, so the carriers pane can show a
/// name instead of peering-session jargon whenever one is known.
pub const CARRIER_NAMES: &[(&str, &str)] = &[
    ("174", "Cogent"),
    ("701", "Verizon"),
    ("1239", "Sprint / Cogent"),
    ("1299", "Arelion"),
    ("2914", "NTT"),
    ("3257", "GTT"),
    ("3320", "Deutsche Telekom"),
    ("3356", "Lumen"),
    ("3491", "PCCW Global"),
    ("4134", "China Telecom"),
    ("4637", "Telstra Global"),
    ("5511", "Orange"),
    ("6453", "Tata Communications"),
    ("6461", "Zayo"),
    ("6762", "Sparkle"),
    ("6830", "Liberty Global"),
    ("6939", "Hurricane Electric"),
    ("7018", "AT&T"),
    ("7922", "Comcast"),
    ("8075", "Microsoft"),
    ("9002", "RETN"),
    ("12956", "Telxius"),
    ("13335", "Cloudflare"),
    ("15169", "Google"),
    ("16509", "Amazon"),
    ("16625", "Akamai"),
    ("20940", "Akamai"),
    ("24940", "Hetzner"),
    ("32934", "Meta"),
    ("36351", "IBM SoftLayer"),
    ("37100", "SEACOM"),
    ("54113", "Fastly"),
];

/// RIPE RIS route collectors by number, for naming looking-glass vantage
/// points instead of printing raw peer addresses.
pub const RRC_LOCATIONS: &[(&str, &str)] = &[
    ("00", "Amsterdam"),
    ("01", "London"),
    ("03", "Amsterdam"),
    ("04", "Geneva"),
    ("05", "Vienna"),
    ("06", "Otemachi"),
    ("07", "Stockholm"),
    ("10", "Milan"),
    ("11", "New York"),
    ("12", "Frankfurt"),
    ("13", "Moscow"),
    ("14", "Palo Alto"),
    ("15", "Sao Paulo"),
    ("16", "Miami"),
    ("18", "Barcelona"),
    ("19", "Johannesburg"),
    ("20", "Zurich"),
    ("21", "Paris"),
    ("22", "Bucharest"),
    ("23", "Singapore"),
    ("24", "Montevideo"),
    ("25", "Amsterdam"),
    ("26", "Dubai"),
];

/// Name a bgp-state `source_id` ("00-102.208.105.2") after its collector.
// proa-perf-ok(scope): one short label per rendered route row.
pub fn collector_label(source_id: &str) -> String {
    let number = source_id.split('-').next().unwrap_or("");
    match RRC_LOCATIONS.iter().find(|(n, _)| *n == number) {
        Some((_, city)) => format!("rrc{number} · {city}"),
        None => source_id.to_string(),
    }
}

/// Collapse AS-path prepends: consecutive repeats of the same ASN become one
/// hop annotated with its repeat count.
// proa-perf-ok(scope): a handful of short hop strings per rendered route.
pub fn format_hops(hops: &[u32]) -> Vec<String> {
    let mut runs: Vec<(u32, usize)> = Vec::new();
    for &hop in hops {
        match runs.last_mut() {
            Some((previous, count)) if *previous == hop => *count += 1,
            _ => runs.push((hop, 1)),
        }
    }
    runs.into_iter()
        .map(|(asn, count)| {
            if count > 1 {
                format!("{asn} ×{count}")
            } else {
                asn.to_string()
            }
        })
        .collect()
}

pub const COLLECTORS: &[&str] = &[
    "rrc00", "rrc01", "rrc03", "rrc04", "rrc05", "rrc10", "rrc11", "rrc12", "rrc13", "rrc14",
    "rrc15", "rrc19", "rrc21", "rrc24", "rrc25", "rrc26",
];

/// Observed route chains rendered in the third dossier pane: hop lists end at
/// the selected origin, which is substituted at render time.
pub const PATH_SHAPES: &[&[&str]] = &[
    &["3356"],
    &["174"],
    &["1299"],
    &["2914", "6939"],
    &["6939"],
    &["9002", "1299"],
    &["4134", "3356"],
    &["37100", "174"],
];

/// Illustrative resolved-incident rail. Deliberately fictional: operators are
/// unnamed and every range is from the RFC 5737 / RFC 3849 documentation
/// space, so nothing here can be mistaken for a claim about a real network.
pub const RESOLVED: &[(&str, &str)] = &[
    (
        "Route leak of 198.51.100.0/24, 8,800 paths",
        "resolved after 27m · replay",
    ),
    (
        "Regional carrier outage, 203.0.113.0/24",
        "restored after 6m · replay",
    ),
    (
        "Signature mismatch on 192.0.2.0/24",
        "corrected by holder · replay",
    ),
    (
        "Visibility dip for 2001:db8::/32",
        "resolved after 2m · replay",
    ),
];

pub const SITES: &[Site] = &[
    Site {
        host: "github.com",
        ip: "140.82.112.4",
        dns_note: "A record · 14 ms · authoritative",
        prefix: "140.82.112.0/20",
        bgp_note: "inside 140.82.112.0/20 · announced continuously",
        net_label: "GitHub's network",
        asn: "AS36459",
        asn_short: "36459",
        asn_line: "AS36459 · GITHUB-NET · registered 2005-11-08",
        prefix_count: "36 records",
        peer_count: "312 neighbours",
        operator: "GitHub",
        operator_detail: "GitHub, Inc. runs the network that holds this address. Owned by Microsoft since 2018.",
        unusual: false,
        unusual_detail: "No unexpected origins or withdrawals observed for this network's ranges in the current RIS window.",
        metrics: [
            Metric { label: "IPv4 ranges", value: "24", sub: "announced now" },
            Metric { label: "IPv6 ranges", value: "12", sub: "announced now" },
            Metric { label: "Neighbours", value: "312", sub: "across 26 collectors" },
            Metric { label: "Verified", value: "97%", sub: "35 of 36 ranges" },
            Metric { label: "Reachability", value: "400/400", sub: "monitoring points" },
            Metric { label: "Last change", value: "4m", sub: "since something moved" },
        ],
        prefixes: &[
            CuratedPrefix { prefix: "140.82.112.0/20", since: "2018-02-14" },
            CuratedPrefix { prefix: "192.30.252.0/22", since: "2012-11-02" },
            CuratedPrefix { prefix: "185.199.108.0/22", since: "2016-06-21" },
            CuratedPrefix { prefix: "143.55.64.0/20", since: "2019-04-09" },
            CuratedPrefix { prefix: "140.82.121.0/24", since: "2018-02-14" },
            CuratedPrefix { prefix: "192.30.255.0/24", since: "2012-11-02" },
            CuratedPrefix { prefix: "2a0a:a440::/29", since: "2020-08-17" },
            CuratedPrefix { prefix: "2606:50c0::/32", since: "2016-06-21" },
            CuratedPrefix { prefix: "140.82.113.0/24", since: "2018-02-14" },
            CuratedPrefix { prefix: "140.82.114.0/24", since: "2018-03-01" },
            CuratedPrefix { prefix: "185.199.109.0/24", since: "2016-06-21" },
            CuratedPrefix { prefix: "185.199.110.0/24", since: "2016-06-21" },
            CuratedPrefix { prefix: "185.199.111.0/24", since: "2016-06-21" },
            CuratedPrefix { prefix: "192.30.253.0/24", since: "2012-11-02" },
        ],
    },
    Site {
        host: "cloudflare.com",
        ip: "104.16.132.229",
        dns_note: "A record · 9 ms · authoritative",
        prefix: "104.16.0.0/13",
        bgp_note: "inside 104.16.0.0/13 · anycast, announced worldwide",
        net_label: "Cloudflare's network",
        asn: "AS13335",
        asn_short: "13335",
        asn_line: "AS13335 · CLOUDFLARENET · registered 2010-07-14",
        prefix_count: "1,924 records",
        peer_count: "12,048 neighbours",
        operator: "Cloudflare",
        operator_detail: "Cloudflare, Inc. operates one of the largest networks on the internet, serving this address from 335 cities.",
        unusual: false,
        unusual_detail: "No unexpected origins, withdrawals, or visibility drops in the last 90 days.",
        metrics: [
            Metric { label: "IPv4 ranges", value: "1,412", sub: "announced now" },
            Metric { label: "IPv6 ranges", value: "512", sub: "announced now" },
            Metric { label: "Neighbours", value: "12,048", sub: "across 26 collectors" },
            Metric { label: "Verified", value: "99%", sub: "1,918 of 1,924" },
            Metric { label: "Reachability", value: "400/400", sub: "monitoring points" },
            Metric { label: "Last change", value: "90d+", sub: "since anything moved" },
        ],
        prefixes: &[
            CuratedPrefix { prefix: "104.16.0.0/13", since: "2014-03-11" },
            CuratedPrefix { prefix: "172.64.0.0/13", since: "2017-07-19" },
            CuratedPrefix { prefix: "162.158.0.0/15", since: "2015-09-02" },
            CuratedPrefix { prefix: "198.41.128.0/17", since: "2012-02-14" },
            CuratedPrefix { prefix: "141.101.64.0/18", since: "2013-06-05" },
            CuratedPrefix { prefix: "108.162.192.0/18", since: "2013-02-21" },
            CuratedPrefix { prefix: "2606:4700::/32", since: "2014-04-01" },
            CuratedPrefix { prefix: "2400:cb00::/32", since: "2011-08-23" },
            CuratedPrefix { prefix: "1.1.1.0/24", since: "2018-04-01" },
            CuratedPrefix { prefix: "103.21.244.0/22", since: "2014-08-30" },
            CuratedPrefix { prefix: "131.0.72.0/22", since: "2016-01-19" },
            CuratedPrefix { prefix: "188.114.96.0/20", since: "2015-04-07" },
            CuratedPrefix { prefix: "190.93.240.0/20", since: "2013-11-12" },
            CuratedPrefix { prefix: "197.234.240.0/22", since: "2014-06-18" },
        ],
    },
    Site {
        host: "wikipedia.org",
        ip: "208.80.154.224",
        dns_note: "A record · 22 ms · authoritative",
        prefix: "208.80.152.0/22",
        bgp_note: "inside 208.80.152.0/22 · announced from 5 data centres",
        net_label: "Wikimedia's network",
        asn: "AS14907",
        asn_short: "14907",
        asn_line: "AS14907 · WIKIMEDIA · registered 2007-09-27",
        prefix_count: "14 records",
        peer_count: "186 neighbours",
        operator: "Wikimedia",
        operator_detail: "The Wikimedia Foundation runs this network itself, a nonprofit operating its own infrastructure.",
        unusual: false,
        unusual_detail: "No unexpected origins or withdrawals in the last 90 days.",
        metrics: [
            Metric { label: "IPv4 ranges", value: "9", sub: "announced now" },
            Metric { label: "IPv6 ranges", value: "5", sub: "announced now" },
            Metric { label: "Neighbours", value: "186", sub: "across 26 collectors" },
            Metric { label: "Verified", value: "100%", sub: "14 of 14 ranges" },
            Metric { label: "Reachability", value: "398/400", sub: "monitoring points" },
            Metric { label: "Last change", value: "31d", sub: "since anything moved" },
        ],
        prefixes: &[
            CuratedPrefix { prefix: "208.80.152.0/22", since: "2008-01-14" },
            CuratedPrefix { prefix: "185.15.56.0/22", since: "2014-09-30" },
            CuratedPrefix { prefix: "91.198.174.0/24", since: "2007-11-06" },
            CuratedPrefix { prefix: "198.35.26.0/23", since: "2013-05-21" },
            CuratedPrefix { prefix: "103.102.166.0/24", since: "2019-02-11" },
            CuratedPrefix { prefix: "2620:0:860::/46", since: "2011-03-08" },
            CuratedPrefix { prefix: "2a02:ec80::/32", since: "2014-09-30" },
            CuratedPrefix { prefix: "2001:df2:e500::/48", since: "2019-02-11" },
            CuratedPrefix { prefix: "208.80.153.0/24", since: "2008-01-14" },
        ],
    },
    Site {
        host: "google.com",
        ip: "142.250.80.46",
        dns_note: "A record · 7 ms · authoritative",
        prefix: "142.250.0.0/15",
        bgp_note: "inside 142.250.0.0/15 · announced worldwide",
        net_label: "Google's network",
        asn: "AS15169",
        asn_short: "15169",
        asn_line: "AS15169 · GOOGLE · registered 2000-03-30",
        prefix_count: "1,494 records",
        peer_count: "9,412 neighbours",
        operator: "Google",
        operator_detail: "Google LLC operates this network, carrying its own traffic on private infrastructure worldwide.",
        unusual: false,
        unusual_detail: "One backup path was withdrawn 20 minutes ago, which is routine maintenance.",
        metrics: [
            Metric { label: "IPv4 ranges", value: "1,088", sub: "announced now" },
            Metric { label: "IPv6 ranges", value: "406", sub: "announced now" },
            Metric { label: "Neighbours", value: "9,412", sub: "across 26 collectors" },
            Metric { label: "Verified", value: "98%", sub: "1,468 of 1,494" },
            Metric { label: "Reachability", value: "400/400", sub: "monitoring points" },
            Metric { label: "Last change", value: "20m", sub: "routine withdrawal" },
        ],
        prefixes: &[
            CuratedPrefix { prefix: "142.250.0.0/15", since: "2018-06-12" },
            CuratedPrefix { prefix: "8.8.8.0/24", since: "2009-12-03" },
            CuratedPrefix { prefix: "172.217.0.0/16", since: "2015-01-27" },
            CuratedPrefix { prefix: "216.58.192.0/19", since: "2011-05-16" },
            CuratedPrefix { prefix: "74.125.0.0/16", since: "2007-03-13" },
            CuratedPrefix { prefix: "64.233.160.0/19", since: "2004-07-08" },
            CuratedPrefix { prefix: "2001:4860::/32", since: "2005-11-01" },
            CuratedPrefix { prefix: "2607:f8b0::/32", since: "2008-04-16" },
            CuratedPrefix { prefix: "35.190.0.0/17", since: "2016-09-22" },
            CuratedPrefix { prefix: "130.211.0.0/16", since: "2014-02-05" },
            CuratedPrefix { prefix: "104.154.0.0/15", since: "2015-06-30" },
            CuratedPrefix { prefix: "146.148.0.0/17", since: "2013-10-11" },
        ],
    },
];

pub fn default_site() -> &'static Site {
    &SITES[0]
}

/// Resolve a lookup query the way the prototype does: trim, lowercase, strip
/// scheme, `www.`, and any path.
pub fn site_for(raw: &str) -> Option<&'static Site> {
    let normalized = normalize_query(raw);
    SITES.iter().find(|site| site.host == normalized)
}

pub fn normalize_query(raw: &str) -> String {
    let mut q = raw.trim().to_ascii_lowercase();
    for scheme in ["https://", "http://"] {
        if let Some(rest) = q.strip_prefix(scheme) {
            q = rest.to_string();
        }
    }
    if let Some(rest) = q.strip_prefix("www.") {
        q = rest.to_string();
    }
    if let Some(index) = q.find('/') {
        q.truncate(index);
    }
    q
}

pub fn carrier_name(short: &str) -> &'static str {
    CARRIER_NAMES
        .iter()
        .find(|(asn, _)| *asn == short)
        .map(|(_, name)| *name)
        .unwrap_or("")
}

// ---------------------------------------------------------------------------
// Live RIPEstat data model, loaded per request through the AtlasLoader.
// ---------------------------------------------------------------------------

use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct Subject {
    pub query: String,
    pub domain: Option<String>,
    pub address: Option<String>,
    pub prefix: String,
    pub asn: u32,
}

#[derive(Debug, Clone, Default)]
pub struct RoutingStatus {
    pub first_seen: String,
    pub last_seen: String,
    pub seen_by: u64,
    pub collectors: u64,
    pub route_objects: Vec<String>,
}

#[derive(Debug, Clone, Default)]
pub struct NetworkOverview {
    pub holder: String,
    pub announced: bool,
    pub block: String,
}

#[derive(Debug, Clone)]
pub struct PrefixRecord {
    pub prefix: String,
    pub since: String,
    pub until: String,
}

#[derive(Debug, Clone)]
pub struct Neighbour {
    pub asn: u32,
    pub relation: String,
    pub power: u64,
    pub v4_peers: u64,
    pub v6_peers: u64,
}

#[derive(Debug, Clone)]
pub struct RoutePath {
    pub collector: String,
    pub hops: Vec<u32>,
}

#[derive(Debug, Clone)]
pub struct Dossier {
    pub subject: Subject,
    pub overview: NetworkOverview,
    pub routing: RoutingStatus,
    pub prefixes: Vec<PrefixRecord>,
    pub neighbours: Vec<Neighbour>,
    pub paths: Vec<RoutePath>,
    pub rpki_status: String,
    pub fetched_at: String,
    pub fetch_millis: u128,
    /// Set when this dossier was served from the loader's TTL cache instead
    /// of a fresh RIPEstat fan-out, so surfaces like the receipt can say so.
    pub cache_age_seconds: Option<u64>,
    /// True when any widget soft-degraded (timeout or transient failure);
    /// degraded dossiers are never cached, so one blip cannot stick.
    pub degraded: bool,
}

impl Dossier {
    pub fn records(&self) -> usize {
        self.prefixes.len() + self.neighbours.len() + self.paths.len()
    }

    pub fn display_name(&self) -> &str {
        if self.overview.holder.is_empty() {
            "Unknown network"
        } else {
            &self.overview.holder
        }
    }

    pub fn visibility_percent(&self) -> u64 {
        self.routing
            .seen_by
            .saturating_mul(100)
            .checked_div(self.routing.collectors)
            .unwrap_or(0)
    }
}

pub type SharedDossier = Arc<Dossier>;
