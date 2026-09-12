// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! The RIPEstat-backed [`DataLoader`] for the atlas.
//!
//! Two layers keep RIPEstat traffic proportional to what actually changed:
//!
//! 1. **Per request**: every dossier pane requests the same
//!    `ripestat:dossier:<query>` key through `cx.load_typed`, and the
//!    request-scoped [`DataResolver`] deduplicates them — one page render
//!    performs exactly one concurrent RIPEstat fan-out no matter how many
//!    components consume the result.
//! 2. **Across requests**: successful dossiers are kept for
//!    [`DOSSIER_TTL`]; a reload inside that window is served from memory
//!    (routing data moves on BGP timescales, so sixty seconds of reuse is
//!    invisible), and the served copy carries its cache age so the receipt
//!    can label it honestly. Errors are never cached: a transient RIPEstat
//!    failure should not stick for a minute.

use std::collections::HashMap;
use std::future::Future;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use proa_core::{DataLoader, LoadError, LoadKey, LoadRequest, LoadValue};

use crate::api::{RipeClient, RipeError};
use crate::model::Dossier;

const DOSSIER_PREFIX: &str = "dossier:";

pub fn dossier_key(query: &str) -> LoadKey {
    LoadKey::named("ripestat", format!("{DOSSIER_PREFIX}{query}"))
}

/// How long a fetched dossier keeps serving reloads before the next request
/// reaches RIPEstat again.
pub const DOSSIER_TTL: Duration = Duration::from_secs(60);

/// Upper bound on distinct cached queries so arbitrary lookups cannot grow
/// the map without limit; expired entries are pruned first, then the oldest
/// live entry makes room.
const CACHE_CAP: usize = 256;

type DossierCache = Arc<Mutex<HashMap<String, (Instant, Dossier)>>>;

fn cache_lookup(cache: &DossierCache, ttl: Duration, query: &str) -> Option<Dossier> {
    let map = cache.lock().expect("dossier cache lock");
    let (fetched, dossier) = map.get(query)?;
    let age = fetched.elapsed();
    if age >= ttl {
        return None;
    }
    // proa-perf-ok: one deep clone per cache-served lookup; the alternative
    // is a full RIPEstat fan-out.
    let mut served = dossier.clone();
    served.cache_age_seconds = Some(age.as_secs());
    Some(served)
}

fn cache_insert(cache: &DossierCache, ttl: Duration, query: &str, dossier: &Dossier) {
    let mut map = cache.lock().expect("dossier cache lock");
    map.retain(|_, (fetched, _)| fetched.elapsed() < ttl);
    if map.len() >= CACHE_CAP {
        let oldest = map
            .iter()
            .min_by_key(|(_, (fetched, _))| *fetched)
            .map(|(key, _)| key.clone());
        if let Some(oldest) = oldest {
            map.remove(&oldest);
        }
    }
    map.insert(query.to_string(), (Instant::now(), dossier.clone()));
}

/// One cache-aware fetch, shared by the [`DataLoader`] impl and the markdown
/// routes. Concurrent misses for the same query each fetch (the per-request
/// resolver already dedupes the common case); only successes are cached.
async fn fetch_with_cache(
    client: RipeClient,
    cache: DossierCache,
    ttl: Duration,
    origin_loads: Arc<AtomicUsize>,
    query: &str,
) -> Result<Dossier, RipeError> {
    if let Some(hit) = cache_lookup(&cache, ttl, query) {
        return Ok(hit);
    }
    let dossier = client.dossier(query).await?;
    origin_loads.fetch_add(1, Ordering::SeqCst);
    // A dossier missing widget data must not stick for a minute: serve it
    // once, and let the next request retry upstream.
    if !dossier.degraded {
        cache_insert(&cache, ttl, query, &dossier);
    }
    Ok(dossier)
}

pub struct AtlasLoader {
    client: RipeClient,
    origin_loads: Arc<AtomicUsize>,
    ttl: Duration,
    cache: DossierCache,
}

impl AtlasLoader {
    pub fn new(client: RipeClient) -> Self {
        Self::with_ttl(client, DOSSIER_TTL)
    }

    /// Construct with an explicit TTL (tests use zero to force refetches).
    pub fn with_ttl(client: RipeClient, ttl: Duration) -> Self {
        Self {
            client,
            origin_loads: Arc::new(AtomicUsize::new(0)),
            ttl,
            cache: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn client(&self) -> &RipeClient {
        &self.client
    }

    /// Fetch one dossier through the TTL cache, exactly as the page's load
    /// path does; the markdown routes use this so `/site/<host>.md` and the
    /// HTML page share freshness.
    pub async fn cached_dossier(&self, query: &str) -> Result<Dossier, RipeError> {
        fetch_with_cache(
            self.client.clone(),
            Arc::clone(&self.cache),
            self.ttl,
            Arc::clone(&self.origin_loads),
            query,
        )
        .await
    }

    /// How many times the loader actually reached RIPEstat: cache hits do
    /// not count. The dataloader test asserts one per page render.
    pub fn origin_loads(&self) -> usize {
        self.origin_loads.load(Ordering::SeqCst)
    }
}

impl DataLoader for AtlasLoader {
    fn load(
        &self,
        request: &LoadRequest,
    ) -> impl Future<Output = Result<LoadValue, LoadError>> + Send {
        let client = self.client.clone();
        let cache = Arc::clone(&self.cache);
        let ttl = self.ttl;
        let origin_loads = Arc::clone(&self.origin_loads);
        let key = request.key().to_string();
        async move {
            let query = key
                .strip_prefix("ripestat:")
                .and_then(|rest| rest.strip_prefix(DOSSIER_PREFIX))
                .ok_or_else(|| LoadError::custom(format!("unknown load key: {key}")))?;
            let dossier = fetch_with_cache(client, cache, ttl, origin_loads, query)
                .await
                .map_err(|error| {
                    // Stable prefixes the panes sniff to tell "your query
                    // does not resolve" apart from "RIPEstat is unreachable".
                    let class = if error.is_query_problem() {
                        "unresolvable"
                    } else {
                        "unreachable"
                    };
                    LoadError::custom(format!("{class}: {error}"))
                })?;
            Ok(LoadValue::typed(dossier))
        }
    }
}

/// A canned loader for tests and the golden fixture: serves one fixed dossier
/// for every dossier key and counts how often it is asked to reach origin.
pub struct FixtureLoader {
    dossier: Dossier,
    origin_loads: AtomicUsize,
}

impl FixtureLoader {
    pub fn new(dossier: Dossier) -> Self {
        Self {
            dossier,
            origin_loads: AtomicUsize::new(0),
        }
    }

    /// A clone of the canned dossier, for tests that need a realistic value.
    pub fn dossier(&self) -> Dossier {
        self.dossier.clone()
    }

    /// The github.com dossier the ORIGIN v2 prototype ships.
    pub fn github() -> Self {
        use crate::model::{
            Neighbour, NetworkOverview, PrefixRecord, RoutePath, RoutingStatus, Subject,
        };
        Self::new(Dossier {
            subject: Subject {
                query: "github.com".to_string(),
                domain: Some("github.com".to_string()),
                address: Some("140.82.112.4".to_string()),
                prefix: "140.82.112.0/20".to_string(),
                asn: 36459,
            },
            overview: NetworkOverview {
                holder: "GITHUB - GitHub, Inc.".to_string(),
                announced: true,
                block: "IANA 16-bit Autonomous System (AS) Numbers Registry".to_string(),
            },
            routing: RoutingStatus {
                first_seen: "2018-06-31T00:00:00".to_string(),
                last_seen: "2026-09-02T00:00:00".to_string(),
                seen_by: 324,
                collectors: 326,
                route_objects: Vec::new(),
            },
            prefixes: vec![
                PrefixRecord {
                    prefix: "140.82.112.0/20".to_string(),
                    since: "2018-02-14".to_string(),
                    until: "2026-09-02".to_string(),
                },
                PrefixRecord {
                    prefix: "192.30.252.0/22".to_string(),
                    since: "2012-11-02".to_string(),
                    until: "2026-09-02".to_string(),
                },
                PrefixRecord {
                    prefix: "185.199.108.0/22".to_string(),
                    since: "2016-06-21".to_string(),
                    until: "2026-09-02".to_string(),
                },
                PrefixRecord {
                    prefix: "2a0a:a440::/29".to_string(),
                    since: "2020-08-17".to_string(),
                    until: "2026-09-02".to_string(),
                },
            ],
            neighbours: vec![
                Neighbour {
                    asn: 3356,
                    relation: "left".to_string(),
                    power: 2609,
                    v4_peers: 2609,
                    v6_peers: 115,
                },
                Neighbour {
                    asn: 1299,
                    relation: "left".to_string(),
                    power: 1400,
                    v4_peers: 1400,
                    v6_peers: 90,
                },
                Neighbour {
                    asn: 6939,
                    relation: "right".to_string(),
                    power: 30,
                    v4_peers: 30,
                    v6_peers: 4,
                },
            ],
            paths: vec![RoutePath {
                collector: "rrc00".to_string(),
                hops: vec![34224, 1299, 36459],
            }],
            rpki_status: "valid".to_string(),
            fetched_at: "2026-09-02T00:00:00Z".to_string(),
            fetch_millis: 845,
            cache_age_seconds: None,
            degraded: false,
        })
    }

    pub fn origin_loads(&self) -> usize {
        self.origin_loads.load(Ordering::SeqCst)
    }
}

impl DataLoader for FixtureLoader {
    fn load(
        &self,
        request: &LoadRequest,
    ) -> impl Future<Output = Result<LoadValue, LoadError>> + Send {
        self.origin_loads.fetch_add(1, Ordering::SeqCst);
        let key = request.key().to_string();
        let dossier = self.dossier.clone();
        async move {
            if key.starts_with("ripestat:") {
                Ok(LoadValue::typed(dossier))
            } else {
                Err(LoadError::custom(format!("unknown load key: {key}")))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cache() -> DossierCache {
        Arc::new(Mutex::new(HashMap::new()))
    }

    #[test]
    fn cache_hit_within_ttl_carries_its_age() {
        let cache = cache();
        let dossier = FixtureLoader::github().dossier();
        cache_insert(&cache, DOSSIER_TTL, "github.com", &dossier);
        let served = cache_lookup(&cache, DOSSIER_TTL, "github.com")
            .expect("entry inside the TTL is served");
        assert_eq!(served.cache_age_seconds, Some(0));
        assert_eq!(served.subject.asn, dossier.subject.asn);
        assert!(
            cache_lookup(&cache, DOSSIER_TTL, "gitlab.com").is_none(),
            "unrelated queries miss"
        );
    }

    #[test]
    fn cache_entries_expire_at_the_ttl() {
        let cache = cache();
        let dossier = FixtureLoader::github().dossier();
        cache_insert(&cache, Duration::ZERO, "github.com", &dossier);
        assert!(
            cache_lookup(&cache, Duration::ZERO, "github.com").is_none(),
            "a zero TTL forces every lookup back to origin"
        );
    }

    #[test]
    fn cache_evicts_oldest_at_capacity() {
        let cache = cache();
        let dossier = FixtureLoader::github().dossier();
        cache_insert(&cache, DOSSIER_TTL, "site-0.test", &dossier);
        // Make the first entry strictly oldest even on coarse clocks.
        std::thread::sleep(Duration::from_millis(2));
        for index in 1..CACHE_CAP {
            cache_insert(&cache, DOSSIER_TTL, &format!("site-{index}.test"), &dossier);
        }
        cache_insert(&cache, DOSSIER_TTL, "one-more.test", &dossier);
        let map = cache.lock().unwrap();
        assert_eq!(map.len(), CACHE_CAP);
        assert!(map.contains_key("one-more.test"));
        assert!(
            !map.contains_key("site-0.test"),
            "the oldest live entry makes room"
        );
    }

    #[test]
    fn fresh_dossiers_carry_no_cache_age() {
        assert_eq!(FixtureLoader::github().dossier().cache_age_seconds, None);
    }
}
