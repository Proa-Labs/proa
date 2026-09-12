// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Ergonomic builder API for rendering React islands with the `html_sync!` macro.
use crate::island::react_island_render_with_error;
use proa_core::{ctx::Ctx, WebRenderSync, WriteError};
use serde::Serialize;
use serde_json::Value as JsonValue;
use std::collections::HashMap;
/// Hydration strategy for an island.
#[derive(Clone, Debug, Default)]
pub enum IslandStrategy {
    /// Hydrate immediately during boot.
    #[default]
    Immediate,
    /// Hydrate during the next idle period.
    Idle,
    /// Hydrate once the island becomes visible.
    Visible,
    /// Hydrate after a specific DOM event.
    Interaction { event: String },
}
/// Error handling configuration for an island.
#[derive(Clone, Debug)]
pub struct ErrorBoundaryConfig {
    /// Number of times to retry hydration on failure (default: 3)
    pub retry_count: u32,
    /// Delay in milliseconds between retries (uses exponential backoff)
    pub retry_delay_ms: u32,
    /// HTML content to display if hydration fails after all retries
    pub error_fallback: Option<String>,
    /// Whether to report errors to the server
    pub report_errors: bool,
}
impl Default for ErrorBoundaryConfig {
    fn default() -> Self {
        Self {
            retry_count: 3,
            retry_delay_ms: 1000,
            error_fallback: None,
            report_errors: true,
        }
    }
}
impl ErrorBoundaryConfig {
    /// Create a new error boundary configuration with default values.
    pub fn new() -> Self {
        Self::default()
    }
    /// Set the number of retry attempts.
    pub fn with_retry_count(mut self, count: u32) -> Self {
        self.retry_count = count;
        self
    }
    /// Set the delay between retries in milliseconds.
    pub fn with_retry_delay(mut self, delay_ms: u32) -> Self {
        self.retry_delay_ms = delay_ms;
        self
    }
    /// Set the HTML fallback content to display on error.
    pub fn with_error_fallback(mut self, html: impl Into<String>) -> Self {
        self.error_fallback = Some(html.into());
        self
    }
    /// Enable or disable error reporting to the server.
    pub fn with_error_reporting(mut self, enabled: bool) -> Self {
        self.report_errors = enabled;
        self
    }
}
/// Boxed fallback renderer — type-erased so the builder stays one concrete type.
type FallbackFn = Box<dyn FnOnce(&mut Ctx) -> Result<(), WriteError> + Send>;
/// Builder for rendering a React island component.
///
/// # Example
///
/// ```ignore
/// use proa_react::Island;
/// use proa_macros::html_sync;
///
/// // Plain text fallback
/// Island::new("Counter").fallback("Loading...")
///
/// // html_sync! fallback
/// Island::new("Chart").fallback(html_sync! {
///     <div class="skeleton" style="height:200px" />
/// })
/// ```
pub struct Island {
    /// Name of the island (matches manifest entry)
    name: String,
    /// Stable ID for this island instance
    id: Option<String>,
    /// Props to serialize and pass to the React component
    props: HashMap<String, JsonValue>,
    /// Pre-serialized JSON props (from typed IslandDef)
    props_json_override: Option<String>,
    /// Hydration strategy
    strategy: IslandStrategy,
    /// Optional SSR fallback — any `WebRender<::proa_core::NoLoader, Vec<u8>>` (text, html_sync!, components)
    fallback: Option<FallbackFn>,
    /// Error boundary configuration
    error_boundary: Option<ErrorBoundaryConfig>,
    /// Inline style for the island container (used for size hints)
    style: Option<String>,
    /// Whether this island should be preserved across Turbo Drive navigations
    permanent: bool,
}
impl Island {
    /// Create a new island with the given name.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            id: None,
            props: HashMap::new(),
            props_json_override: None,
            strategy: IslandStrategy::default(),
            fallback: None,
            error_boundary: None,
            style: None,
            permanent: false,
        }
    }
    /// Set a stable ID for this island instance.
    ///
    /// If not provided, a default ID will be generated based on the island name.
    pub fn id(mut self, id: impl Into<String>) -> Self {
        self.id = Some(id.into());
        self
    }
    /// Add a prop to pass to the React component.
    ///
    /// The value must be serializable to JSON.
    pub fn prop<T: Serialize>(mut self, key: impl Into<String>, value: T) -> Self {
        if let Ok(json_value) = serde_json::to_value(value) {
            self.props.insert(key.into(), json_value);
        }
        self
    }
    /// Add multiple props from an iterator.
    pub fn props<I, K, V>(mut self, props: I) -> Self
    where
        I: IntoIterator<Item = (K, V)>,
        K: Into<String>,
        V: Serialize,
    {
        for (key, value) in props {
            if let Ok(json_value) = serde_json::to_value(value) {
                self.props.insert(key.into(), json_value);
            }
        }
        self
    }
    /// Set the hydration strategy.
    pub fn strategy(mut self, strategy: IslandStrategy) -> Self {
        self.strategy = strategy;
        self
    }
    /// Set SSR fallback content rendered inside the island container while
    /// the React component loads.
    ///
    /// Accepts anything that implements `WebRender<::proa_core::NoLoader, Vec<u8>>`: plain `&str`,
    /// `html_sync!` blocks, or full component structs.
    ///
    /// ```ignore
    /// // Plain text
    /// .fallback("Loading...")
    ///
    /// // html_sync! block
    /// .fallback(html_sync! { <div class="skeleton" style="height:200px" /> })
    ///
    /// // Component
    /// .fallback(Spinner { size: SpinnerSize::Large })
    /// ```
    pub fn fallback(mut self, content: impl WebRenderSync + Send + 'static) -> Self {
        self.fallback = Some(Box::new(move |cx: &mut Ctx| content.render(cx)));
        self
    }
    /// Configure error boundary for this island.
    ///
    /// # Example
    ///
    /// ```ignore
    /// Island::new("UserProfile")
    ///     .error_boundary(
    ///         ErrorBoundaryConfig::new()
    ///             .with_retry_count(3)
    ///             .with_error_fallback("<div class='error'>Failed to load profile</div>")
    ///     )
    /// ```
    pub fn error_boundary(mut self, config: ErrorBoundaryConfig) -> Self {
        self.error_boundary = Some(config);
        self
    }
    /// Configure error boundary with a simple error message.
    ///
    /// This is a convenience method that creates a default error boundary
    /// with the specified error fallback HTML.
    pub fn on_error(mut self, fallback_html: impl Into<String>) -> Self {
        self.error_boundary = Some(ErrorBoundaryConfig::new().with_error_fallback(fallback_html));
        self
    }
    /// Set a minimum height for the island container to reserve space
    /// and prevent layout shift while the component loads.
    ///
    /// After hydration, the min-height is automatically removed via CSS.
    pub fn size_hint(mut self, min_height_px: u32) -> Self {
        self.style = Some(format!("min-height:{}px", min_height_px));
        self
    }
    /// Set an inline style on the island container div.
    pub fn style(mut self, style: impl Into<String>) -> Self {
        self.style = Some(style.into());
        self
    }
    /// Mark this island as permanent so it survives Turbo Drive navigations.
    ///
    /// Permanent islands retain their React state across page transitions.
    /// The container div will receive a `data-turbo-permanent` attribute.
    pub fn permanent(mut self) -> Self {
        self.permanent = true;
        self
    }
    /// Set props from a serializable struct, bypassing the HashMap.
    ///
    /// Used by `IslandDef::island()` to pass typed props directly.
    pub fn props_from_serialize<P: Serialize>(mut self, props: &P) -> Self {
        self.props_json_override =
            Some(serde_json::to_string(props).unwrap_or_else(|_| "{}".into()));
        self
    }
    /// Render the island to the given context.
    ///
    /// This generates:
    /// 1. A `<div>` with `data-island` attributes
    /// 2. A `<script type="application/json">` tag with props
    ///
    /// The island is recorded in the request-owned render artifact ledger.
    pub fn render(self, cx: &mut Ctx) -> Result<(), WriteError> {
        cx.react_island_used(&self.name);
        let island_id = self
            .id
            // proa-perf-ok: an owned DOM id is required for an unnamed React island
            .unwrap_or_else(|| {
                proa_core::perf_ok(format!("{}-{}", self.name, cx.next_react_island_id()))
            });
        let props_json = self.props_json_override.unwrap_or_else(|| {
            // proa-perf-ok: React hydration requires one owned JSON payload
            proa_core::perf_ok(serde_json::to_string(&self.props).unwrap_or_else(|_| "{}".into()))
        });
        let error_config_ref = self.error_boundary.as_ref();
        let fallback = self.fallback;
        let style = self.style.as_deref();
        let permanent = self.permanent;
        react_island_render_with_error(
            cx,
            &self.name,
            &island_id,
            &props_json,
            |cx| {
                if let Some(fallback_fn) = fallback {
                    fallback_fn(cx)?;
                }
                Ok(())
            },
            error_config_ref,
            style,
            permanent,
        )
    }
}
impl WebRenderSync for Island {
    fn render(self, cx: &mut Ctx) -> Result<(), WriteError> {
        Island::render(self, cx)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn island_renders_with_props() {
        let mut cx = Ctx::new();
        Island::new("Counter")
            .id("counter-1")
            .prop("initialCount", 5)
            .prop("label", "Clicks")
            .render(&mut cx)
            .unwrap();
        let html = String::from_utf8(cx.into_output().into_vec()).unwrap();
        assert!(html.contains(r#"data-island="Counter""#));
        assert!(html.contains(r#"id="counter-1""#));
        assert!(html.contains(r#""initialCount":5"#));
        assert!(html.contains(r#""label":"Clicks""#));
    }
    #[test]
    fn island_with_fallback() {
        let mut cx = Ctx::new();
        Island::new("Skeleton")
            .id("skeleton-1")
            .fallback("Loading...")
            .render(&mut cx)
            .unwrap();
        let html = String::from_utf8(cx.into_output().into_vec()).unwrap();
        assert!(html.contains("Loading..."));
    }
    #[test]
    fn island_with_size_hint() {
        let mut cx = Ctx::new();
        Island::new("Counter")
            .id("counter-hint")
            .size_hint(120)
            .fallback("Loading...")
            .render(&mut cx)
            .unwrap();
        let html = String::from_utf8(cx.into_output().into_vec()).unwrap();
        assert!(html.contains(r#"style="min-height:120px""#));
        assert!(html.contains("Loading..."));
    }
    #[test]
    fn island_auto_generates_id() {
        let mut cx = Ctx::new();
        Island::new("Counter").render(&mut cx).unwrap();
        Island::new("Counter").render(&mut cx).unwrap();
        let html = String::from_utf8(cx.into_output().into_vec()).unwrap();
        assert!(html.contains(r#"id="Counter-0""#));
        assert!(html.contains(r#"id="Counter-1""#));
    }

    #[test]
    fn island_artifacts_and_ids_are_request_owned() {
        let mut first = Ctx::new();
        let mut second = Ctx::new();

        Island::new("First").render(&mut first).unwrap();
        Island::new("Second").render(&mut second).unwrap();
        Island::new("FirstAgain").render(&mut first).unwrap();

        let first_artifacts = first.render_artifacts_snapshot();
        let second_artifacts = second.render_artifacts_snapshot();
        let first_html = first.into_output().into_string();
        let second_html = second.into_output().into_string();

        assert_eq!(
            first_artifacts.react_islands,
            vec!["First".to_owned(), "FirstAgain".to_owned()]
        );
        assert_eq!(second_artifacts.react_islands, vec!["Second".to_owned()]);
        assert!(first_html.contains(r#"id="First-0""#));
        assert!(first_html.contains(r#"id="FirstAgain-1""#));
        assert!(second_html.contains(r#"id="Second-0""#));
    }
}
