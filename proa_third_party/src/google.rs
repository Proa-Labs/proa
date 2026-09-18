// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Google Analytics 4. Consent is controlled by the application; this crate
//! does not display a consent banner or infer consent from a page visit.

use std::future::{Future, Ready};

use proa_core::{
    csp_nonce_attr, DataLoader, MdRender, MdRenderSync, RenderOutcome, StaticRaw, WebContext,
    WebRenderSync, WriteBuf, WriteError,
};
use proa_macros::html_sync;

use crate::ScriptStrategy;

/// Whether the application has already established permission to collect.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum AnalyticsConsent {
    /// Do not load GA or retain events until the application's consent UI grants it.
    #[default]
    Required,
    /// The application permits collection. This does not obtain visitor consent.
    Granted,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("GA4 measurement ID must be G- followed by uppercase ASCII letters or digits")]
pub struct InvalidMeasurementId;

/// A GA4 integration. Construction validates public configuration without
/// allocating; rendering reads the request's CSP nonce from WebContext.
/// Markdown rendering intentionally emits nothing.
#[derive(Debug, Clone, Copy)]
pub struct GoogleAnalytics<'a> {
    measurement_id: &'a str,
    strategy: ScriptStrategy,
    consent: AnalyticsConsent,
    send_page_view: bool,
}

impl<'a> GoogleAnalytics<'a> {
    pub fn new(measurement_id: &'a str) -> Result<Self, InvalidMeasurementId> {
        let measurement_id = measurement_id.trim();
        let suffix = measurement_id
            .strip_prefix("G-")
            .ok_or(InvalidMeasurementId)?;
        if suffix.is_empty()
            || !suffix
                .bytes()
                .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit())
        {
            return Err(InvalidMeasurementId);
        }
        Ok(Self {
            measurement_id,
            strategy: ScriptStrategy::default(),
            consent: AnalyticsConsent::default(),
            send_page_view: true,
        })
    }

    pub const fn measurement_id(self) -> &'a str {
        self.measurement_id
    }

    pub const fn strategy(mut self, strategy: ScriptStrategy) -> Self {
        self.strategy = strategy;
        self
    }

    pub const fn consent(mut self, consent: AnalyticsConsent) -> Self {
        self.consent = consent;
        self
    }

    /// Disable automatic document page views when the app sends them itself.
    pub const fn send_page_view(mut self, enabled: bool) -> Self {
        self.send_page_view = enabled;
        self
    }
}

impl<L: DataLoader, B: WriteBuf> WebRenderSync<L, B> for GoogleAnalytics<'_> {
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let nonce = cx.csp_nonce_cloned();
        let consent = match self.consent {
            AnalyticsConsent::Required => "required",
            AnalyticsConsent::Granted => "granted",
        };
        let page_view = if self.send_page_view { "true" } else { "false" };
        html_sync! {
            <script
                data-proa-google-analytics
                data-measurement-id={self.measurement_id}
                data-strategy={self.strategy.as_str()}
                data-consent={consent}
                data-page-view={page_view}
                nonce={csp_nonce_attr(nonce.as_ref())}
            >
                {StaticRaw(crate::LOADER)}
                {StaticRaw(include_str!("google.js"))}
            </script>
        }
        .render(cx)
    }
}

impl<Loader: ::proa_core::DataLoader, B: WriteBuf> MdRenderSync<Loader, B> for GoogleAnalytics<'_> {
    fn render_md(self, _cx: &mut WebContext<Loader, B>) -> Result<(), WriteError> {
        Ok(())
    }
}

impl<Loader: ::proa_core::DataLoader, B: WriteBuf> MdRender<Loader, B> for GoogleAnalytics<'_> {
    fn render_md(
        self,
        cx: &mut WebContext<Loader, B>,
    ) -> RenderOutcome<impl Future<Output = Result<(), WriteError>>> {
        RenderOutcome::<Ready<Result<(), WriteError>>>::Done(MdRenderSync::render_md(self, cx))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_ids_at_construction() {
        assert_eq!(
            GoogleAnalytics::new(" G-TEST123456 \n")
                .unwrap()
                .measurement_id(),
            "G-TEST123456"
        );
        for invalid in [
            "",
            "G-",
            "GTM-ABC123",
            "G-abc123",
            "G-ABC\"",
            "G-ABC</script>",
        ] {
            assert!(GoogleAnalytics::new(invalid).is_err());
        }
    }

    #[test]
    fn defaults_to_consent_required_and_after_load() {
        let mut cx = WebContext::new();
        GoogleAnalytics::new("G-TEST123456")
            .unwrap()
            .render(&mut cx)
            .unwrap();
        let html = cx.into_output().into_string();
        assert!(html.contains(r#"data-consent="required""#));
        assert!(html.contains(r#"data-strategy="after-load""#));
        assert!(!html.contains(" src="));
        assert_eq!(html.matches("<script").count(), 1);
    }

    #[test]
    fn carries_nonce_and_explicit_options() {
        let mut cx =
            WebContext::new().with_csp_nonce(proa_core::CspNonce::new("YWJjZA==").unwrap());
        GoogleAnalytics::new("G-TEST123456")
            .unwrap()
            .strategy(ScriptStrategy::Idle)
            .consent(AnalyticsConsent::Granted)
            .send_page_view(false)
            .render(&mut cx)
            .unwrap();
        let html = cx.into_output().into_string();
        for attr in [
            r#"nonce="YWJjZA==""#,
            r#"data-consent="granted""#,
            r#"data-strategy="idle""#,
            r#"data-page-view="false""#,
        ] {
            assert!(html.contains(attr), "{attr}");
        }
    }

    #[test]
    fn markdown_ignores_analytics_and_preserves_existing_content() {
        for consent in [AnalyticsConsent::Required, AnalyticsConsent::Granted] {
            let analytics = GoogleAnalytics::new("G-TEST123456")
                .unwrap()
                .consent(consent);
            let mut out = b"Existing Markdown".to_vec();
            let mut cx = WebContext::with_buffer(&mut out);
            MdRenderSync::render_md(analytics, &mut cx).unwrap();
            assert!(matches!(
                MdRender::render_md(analytics, &mut cx),
                RenderOutcome::Done(Ok(()))
            ));
            assert_eq!(out, b"Existing Markdown");
        }
    }
}
