// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use std::{net::IpAddr, time::Instant};

use reqwest::{Client, Url};
use serde_json::Value;

use crate::model::{
    Dossier, Neighbour, NetworkOverview, PrefixRecord, RoutePath, RoutingStatus, Subject,
};

const API_ROOT: &str = "https://stat.ripe.net/data";

#[derive(Clone)]
pub struct RipeClient {
    client: Client,
}

#[derive(Debug, thiserror::Error)]
pub enum RipeError {
    #[error("unsupported lookup `{0}`; enter a domain, IP address, prefix, or ASN")]
    Unsupported(String),
    /// The query was well-formed but nothing answers to it: a name with no
    /// DNS answer, an ASN announcing nothing, or an address outside every
    /// announced range. This is a fact about the query, not about RIPEstat.
    #[error("could not resolve `{0}`")]
    Unresolvable(String),
    #[error("RIPEstat request failed: {0}")]
    Request(#[from] reqwest::Error),
    #[error("RIPEstat returned incomplete data for {0}")]
    Incomplete(String),
}

impl RipeError {
    /// True when the failure is the caller's query rather than the data
    /// source: fix the input, not the network.
    pub fn is_query_problem(&self) -> bool {
        matches!(self, Self::Unsupported(_) | Self::Unresolvable(_))
    }
}

impl RipeClient {
    pub fn new() -> Result<Self, reqwest::Error> {
        Ok(Self {
            client: Client::builder()
                .user_agent(concat!("origin-routing-atlas/", env!("CARGO_PKG_VERSION")))
                .connect_timeout(std::time::Duration::from_secs(5))
                .timeout(std::time::Duration::from_secs(18))
                .build()?,
        })
    }

    pub async fn dossier(&self, raw: &str) -> Result<Dossier, RipeError> {
        let started = Instant::now();
        let subject = self.resolve(raw.trim()).await?;
        let asn = format!("AS{}", subject.asn);
        // One slow widget must not drag the whole dossier: RIPEstat's
        // announced-prefixes for a hyperscaler ASN is megabytes and can take
        // longer than the page deserves, so every widget gets its own budget
        // and degrades softly past it.
        const WIDGET_BUDGET: std::time::Duration = std::time::Duration::from_secs(8);
        let budgeted = |name: &'static str| move |_| RipeError::Incomplete(name.to_string());
        let (overview, routing, prefixes, neighbours, paths, rpki) = tokio::join!(
            async {
                tokio::time::timeout(WIDGET_BUDGET, self.overview(&asn))
                    .await
                    .map_err(budgeted("overview"))?
            },
            async {
                tokio::time::timeout(WIDGET_BUDGET, self.routing_status(&subject.prefix))
                    .await
                    .map_err(budgeted("routing-status"))?
            },
            async {
                tokio::time::timeout(WIDGET_BUDGET, self.announced_prefixes(&asn))
                    .await
                    .map_err(budgeted("announced-prefixes"))?
            },
            async {
                tokio::time::timeout(WIDGET_BUDGET, self.neighbours(&asn))
                    .await
                    .map_err(budgeted("asn-neighbours"))?
            },
            async {
                tokio::time::timeout(WIDGET_BUDGET, self.paths(&subject.prefix))
                    .await
                    .map_err(budgeted("looking-glass"))?
            },
            async {
                tokio::time::timeout(WIDGET_BUDGET, self.rpki(&asn, &subject.prefix))
                    .await
                    .map_err(budgeted("rpki-validation"))?
            },
        );
        let degraded = overview.is_err()
            || routing.is_err()
            || prefixes.is_err()
            || neighbours.is_err()
            || paths.is_err()
            || rpki.is_err();
        Ok(Dossier {
            subject,
            // A dossier fans out across independent RIPEstat widgets. Keep the
            // page useful when one of those widgets is briefly slow or
            // unavailable; resolving the subject itself remains the hard
            // boundary because everything else depends on it.
            overview: overview.unwrap_or_else(|_| NetworkOverview {
                holder: "RIPEstat metadata temporarily unavailable".to_string(),
                announced: true,
                block: "global".to_string(),
            }),
            routing: routing.unwrap_or_else(|_| RoutingStatus {
                first_seen: "unknown".to_string(),
                last_seen: "unknown".to_string(),
                seen_by: 0,
                collectors: 0,
                route_objects: Vec::new(),
            }),
            prefixes: prefixes.unwrap_or_default(),
            neighbours: neighbours.unwrap_or_default(),
            paths: paths.unwrap_or_default(),
            rpki_status: rpki.unwrap_or_else(|_| "unknown".to_string()),
            fetched_at: now_label(),
            fetch_millis: started.elapsed().as_millis(),
            cache_age_seconds: None,
            degraded,
        })
    }

    async fn resolve(&self, raw: &str) -> Result<Subject, RipeError> {
        let raw = if raw.is_empty() { "github.com" } else { raw };
        if let Some(asn) = parse_asn(raw) {
            let prefixes = self.announced_prefixes(&format!("AS{asn}")).await?;
            let prefix = prefixes
                .iter()
                .find(|record| !record.prefix.contains(':'))
                .or_else(|| prefixes.first())
                .map(|record| record.prefix.clone())
                .ok_or_else(|| RipeError::Unresolvable(raw.to_string()))?;
            return Ok(Subject {
                query: raw.to_string(),
                domain: None,
                address: None,
                prefix,
                asn,
            });
        }
        if raw.contains('/') {
            let status = self.value("routing-status", &[("resource", raw)]).await?;
            let asn = status["origins"]
                .as_array()
                .and_then(|origins| origins.first())
                .and_then(|origin| origin["origin"].as_u64())
                .ok_or_else(|| RipeError::Unresolvable(raw.to_string()))?
                as u32;
            return Ok(Subject {
                query: raw.to_string(),
                domain: None,
                address: None,
                prefix: raw.to_string(),
                asn,
            });
        }
        if raw.parse::<IpAddr>().is_ok() {
            return self.resolve_address(raw, raw.to_string(), None).await;
        }
        if raw.contains('.') {
            let dns = self.value("dns-chain", &[("resource", raw)]).await?;
            let address = dns["forward_nodes"]
                .as_object()
                .and_then(|nodes| nodes.get(raw).or_else(|| nodes.values().next()))
                .and_then(Value::as_array)
                .and_then(|addresses| addresses.first())
                .and_then(Value::as_str)
                .ok_or_else(|| RipeError::Unresolvable(raw.to_string()))?;
            return self
                .resolve_address(address, raw.to_string(), Some(raw.to_string()))
                .await;
        }
        Err(RipeError::Unsupported(raw.to_string()))
    }

    async fn resolve_address(
        &self,
        address: &str,
        query: String,
        domain: Option<String>,
    ) -> Result<Subject, RipeError> {
        let info = self.value("network-info", &[("resource", address)]).await?;
        let asn = info["asns"]
            .as_array()
            .and_then(|values| values.first())
            .and_then(|value| value.as_u64().or_else(|| value.as_str()?.parse().ok()))
            .ok_or_else(|| RipeError::Unresolvable(address.to_string()))? as u32;
        let prefix = info["prefix"]
            .as_str()
            .ok_or_else(|| RipeError::Unresolvable(address.to_string()))?
            .to_string();
        Ok(Subject {
            query,
            domain,
            address: Some(address.to_string()),
            prefix,
            asn,
        })
    }

    async fn overview(&self, asn: &str) -> Result<NetworkOverview, RipeError> {
        let data = self.value("as-overview", &[("resource", asn)]).await?;
        // RIPEstat holders look like "GITHUB - GitHub, Inc."; the part after
        // the registry handle reads better everywhere we show it.
        let holder = data["holder"].as_str().unwrap_or("Unknown network");
        let holder = holder
            .split_once(" - ")
            .map(|(_, name)| name.trim())
            .filter(|name| !name.is_empty())
            .unwrap_or(holder)
            .to_string();
        Ok(NetworkOverview {
            holder,
            announced: data["announced"].as_bool().unwrap_or(false),
            block: data["block"]["name"]
                .as_str()
                .unwrap_or("global")
                .to_string(),
        })
    }

    async fn routing_status(&self, prefix: &str) -> Result<RoutingStatus, RipeError> {
        let data = self
            .value("routing-status", &[("resource", prefix)])
            .await?;
        let visibility = if prefix.contains(':') {
            &data["visibility"]["v6"]
        } else {
            &data["visibility"]["v4"]
        };
        let route_objects = data["origins"]
            .as_array()
            .and_then(|origins| origins.first())
            .and_then(|origin| origin["route_objects"].as_array())
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .map(ToString::to_string)
            .collect();
        Ok(RoutingStatus {
            first_seen: data["first_seen"]["time"]
                .as_str()
                .unwrap_or("unknown")
                .to_string(),
            last_seen: data["last_seen"]["time"]
                .as_str()
                .unwrap_or("unknown")
                .to_string(),
            seen_by: visibility["ris_peers_seeing"].as_u64().unwrap_or(0),
            collectors: visibility["total_ris_peers"].as_u64().unwrap_or(0),
            route_objects,
        })
    }

    async fn announced_prefixes(&self, asn: &str) -> Result<Vec<PrefixRecord>, RipeError> {
        let data = self
            .value(
                "announced-prefixes",
                &[("resource", asn), ("min_peers_seeing", "10")],
            )
            .await?;
        Ok(data["prefixes"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|record| {
                let prefix = record["prefix"].as_str()?.to_string();
                let timeline = record["timelines"]
                    .as_array()
                    .and_then(|items| items.last());
                Some(PrefixRecord {
                    prefix,
                    since: timeline
                        .and_then(|value| value["starttime"].as_str())
                        .unwrap_or("unknown")
                        .to_string(),
                    until: timeline
                        .and_then(|value| value["endtime"].as_str())
                        .unwrap_or("present")
                        .to_string(),
                })
            })
            .collect())
    }

    async fn neighbours(&self, asn: &str) -> Result<Vec<Neighbour>, RipeError> {
        let data = self.value("asn-neighbours", &[("resource", asn)]).await?;
        Ok(data["neighbours"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|value| {
                Some(Neighbour {
                    asn: value["asn"].as_u64()? as u32,
                    relation: value["type"].as_str().unwrap_or("peer").to_string(),
                    power: value["power"].as_u64().unwrap_or(0),
                    v4_peers: value["v4_peers"].as_u64().unwrap_or(0),
                    v6_peers: value["v6_peers"].as_u64().unwrap_or(0),
                })
            })
            .collect())
    }

    async fn paths(&self, prefix: &str) -> Result<Vec<RoutePath>, RipeError> {
        let data = self.value("bgp-state", &[("resource", prefix)]).await?;
        // Entries arrive sorted by collector, so a small cap would keep only
        // the first one or two vantage points; take enough to sample eight
        // distinct collectors downstream.
        Ok(data["bgp_state"]
            .as_array()
            .into_iter()
            .flatten()
            .take(512)
            .filter_map(|value| {
                Some(RoutePath {
                    collector: value["source_id"].as_str()?.to_string(),
                    hops: value["path"]
                        .as_array()?
                        .iter()
                        .filter_map(Value::as_u64)
                        .map(|asn| asn as u32)
                        .collect(),
                })
            })
            .collect())
    }

    async fn rpki(&self, asn: &str, prefix: &str) -> Result<String, RipeError> {
        let data = self
            .value("rpki-validation", &[("resource", asn), ("prefix", prefix)])
            .await?;
        Ok(data["status"].as_str().unwrap_or("unknown").to_string())
    }

    async fn value(&self, widget: &str, parameters: &[(&str, &str)]) -> Result<Value, RipeError> {
        let mut url = Url::parse(&format!("{API_ROOT}/{widget}/data.json"))
            .expect("the RIPEstat API root is a valid URL");
        for (name, value) in parameters {
            url.query_pairs_mut().append_pair(name, value);
        }
        let response = self.client.get(url).send().await?;
        // RIPEstat answers 4xx when the resource itself is rejected (for
        // example a name with an invalid top-level domain). That is a fact
        // about the query, not an outage.
        if response.status().is_client_error() {
            let resource = parameters
                .iter()
                .find(|(name, _)| *name == "resource")
                .map(|(_, value)| *value)
                .unwrap_or(widget);
            return Err(RipeError::Unresolvable(resource.to_string()));
        }
        let response = response.error_for_status()?;
        let envelope: Value = response.json().await?;
        Ok(envelope["data"].clone())
    }
}

fn parse_asn(raw: &str) -> Option<u32> {
    raw.trim()
        .strip_prefix("AS")
        .or_else(|| raw.trim().strip_prefix("as"))
        .and_then(|value| value.parse().ok())
}

fn now_label() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    format!("UNIX {seconds}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_explicit_asns_without_treating_bare_numbers_as_asns() {
        assert_eq!(parse_asn("AS13335"), Some(13335));
        assert_eq!(parse_asn("as15169"), Some(15169));
        assert_eq!(parse_asn("13335"), None);
    }
}
