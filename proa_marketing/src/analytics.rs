// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use std::{env, sync::OnceLock};

use proa_core::{
    csp_nonce_attr, DataLoader, StaticRaw, WebContext, WebRenderSync, WriteBuf, WriteError,
};
use proa_macros::html_sync;
use proa_third_party::google::{AnalyticsConsent, GoogleAnalytics};

static MEASUREMENT_ID: OnceLock<Option<String>> = OnceLock::new();

/// Public runtime configuration and production-host policy are app-owned.
pub(crate) fn google_analytics() -> Option<MarketingAnalytics> {
    let measurement_id = MEASUREMENT_ID
        .get_or_init(|| {
            let value = env::var("GOOGLE_ANALYTICS_MEASUREMENT_ID").ok()?;
            let Ok(analytics) = GoogleAnalytics::new(&value) else {
                tracing::warn!(
                    "Google Analytics disabled: invalid GOOGLE_ANALYTICS_MEASUREMENT_ID"
                );
                return None;
            };
            Some(analytics.measurement_id().to_owned())
        })
        .as_deref()?;
    Some(MarketingAnalytics {
        analytics: GoogleAnalytics::new(measurement_id)
            .ok()?
            .consent(AnalyticsConsent::Granted),
    })
}

pub(crate) struct MarketingAnalytics {
    analytics: GoogleAnalytics<'static>,
}

impl<L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for MarketingAnalytics {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let nonce = cx.csp_nonce_cloned();
        html_sync! {
            <script
                data-proa-analytics-policy
                data-measurement-id={self.analytics.measurement_id()}
                nonce={csp_nonce_attr(nonce.as_ref())}
            >
                {StaticRaw(include_str!("analytics.js"))}
            </script>
            {self.analytics}
        }
        .render(cx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn browser_policy_excludes_previews_and_embedded_pages() {
        let output = std::process::Command::new("node")
            .arg("--test")
            .arg(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/tests/analytics-policy.test.mjs"
            ))
            .output()
            .expect("Node.js is required to test the browser analytics policy");
        assert!(
            output.status.success(),
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    #[test]
    fn host_policy_precedes_the_shared_analytics_component() {
        let mut cx = WebContext::new();
        MarketingAnalytics {
            analytics: GoogleAnalytics::new("G-TEST123456")
                .unwrap()
                .consent(AnalyticsConsent::Granted),
        }
        .render(&mut cx)
        .unwrap();
        let html = cx.into_output().into_string();
        assert!(
            html.find("data-proa-analytics-policy").unwrap()
                < html.find("data-proa-google-analytics").unwrap()
        );
        assert!(html.contains(r#"data-strategy="after-load""#));
        assert_eq!(
            html.matches(r#"data-measurement-id="G-TEST123456""#)
                .count(),
            2
        );
    }
}
