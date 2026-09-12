// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use std::env;
use std::sync::OnceLock;

const DEFAULT_SITE_URL: &str = "https://proa.so";

static PUBLIC_SITE_URL: OnceLock<String> = OnceLock::new();

/// Returns the public origin for absolute links emitted by this deployment.
///
/// `SITE_URL` remains an explicit platform-neutral override. Railway preview
/// environments should leave it unset so their injected public domain wins.
pub(crate) fn public_site_url() -> &'static str {
    PUBLIC_SITE_URL
        .get_or_init(|| {
            resolve_public_site_url(
                env::var("SITE_URL").ok().as_deref(),
                env::var("RAILWAY_PUBLIC_DOMAIN").ok().as_deref(),
                env::var("RAILWAY_ENVIRONMENT_NAME").ok().as_deref(),
            )
        })
        .as_str()
}

fn resolve_public_site_url(
    site_url: Option<&str>,
    railway_domain: Option<&str>,
    railway_environment: Option<&str>,
) -> String {
    // Railway copies production variables into native PR environments. While
    // production keeps an explicit canonical SITE_URL during the
    // preview.proa.so -> proa.so migration, a PR must still advertise its own
    // injected public domain instead of inheriting the production canonical.
    if non_empty(railway_environment) != Some("production") {
        if let Some(domain) = non_empty(railway_domain) {
            return railway_site_url(domain);
        }
    }

    if let Some(site_url) = non_empty(site_url) {
        return site_url.trim_end_matches('/').to_owned();
    }

    if let Some(domain) = non_empty(railway_domain) {
        return railway_site_url(domain);
    }

    DEFAULT_SITE_URL.to_owned()
}

fn railway_site_url(domain: &str) -> String {
    let domain = domain.trim_end_matches('/');
    if domain.starts_with("http://") || domain.starts_with("https://") {
        return domain.to_owned();
    }
    format!("https://{domain}")
}

fn non_empty(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|value| !value.is_empty())
}
