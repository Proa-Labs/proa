// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use std::{env, sync::OnceLock};

use proa_core::{DataLoader, StaticRaw, WebContext, WebRenderSync, WriteBuf, WriteError};
use proa_macros::html_sync;

static MEASUREMENT_ID: OnceLock<Option<String>> = OnceLock::new();
const BOOTSTRAP: &str = include_str!("analytics.js");

/// The measurement ID is public configuration, never an OAuth credential.
pub(crate) fn google_analytics() -> Option<GoogleAnalytics> {
    let measurement_id = MEASUREMENT_ID
        .get_or_init(|| {
            let value = env::var("GOOGLE_ANALYTICS_MEASUREMENT_ID").ok()?;
            let Some(value) = validate_measurement_id(&value) else {
                tracing::warn!(
                    "Google Analytics disabled: GOOGLE_ANALYTICS_MEASUREMENT_ID must be a G- prefixed measurement ID"
                );
                return None;
            };
            Some(value.to_owned())
        })
        .as_deref()?;
    Some(GoogleAnalytics { measurement_id })
}

fn validate_measurement_id(value: &str) -> Option<&str> {
    let value = value.trim();
    let suffix = value.strip_prefix("G-")?;
    (!suffix.is_empty()
        && suffix
            .bytes()
            .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit()))
    .then_some(value)
}

pub(crate) struct GoogleAnalytics {
    measurement_id: &'static str,
}

// Shared document heads support the owning shell's custom writer parameter.
impl<L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for GoogleAnalytics {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        html_sync! {
            <script data-proa-analytics data-measurement-id={self.measurement_id}>
                {StaticRaw(BOOTSTRAP)}
            </script>
        }
        .render(cx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn measurement_id_accepts_ga4_ids_and_rejects_malformed_configuration() {
        assert_eq!(
            validate_measurement_id(" G-ABC1234567 \n"),
            Some("G-ABC1234567")
        );
        for invalid in [
            "",
            "G-",
            "GTM-ABC123",
            "G-abc123",
            "G-ABC\"",
            "G-ABC</script>",
        ] {
            assert_eq!(validate_measurement_id(invalid), None, "{invalid}");
        }
    }

    #[test]
    fn tag_passes_the_measurement_id_as_data_without_an_extra_script_request() {
        let mut cx = WebContext::new();
        GoogleAnalytics {
            measurement_id: "G-TEST123456",
        }
        .render(&mut cx)
        .unwrap();
        let html = cx.into_output().into_string();
        assert_eq!(html.matches("<script").count(), 1);
        assert!(html.contains(r#"data-measurement-id="G-TEST123456""#));
        assert!(!html.contains(" src="));
    }
}
