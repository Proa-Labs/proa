// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! External app integration for micro-frontend architectures
//!
//! This module provides support for mounting external applications at different paths,
//! enabling teams to mix Rust SSR/SSG with existing JS/TS frontends, documentation sites,
//! admin panels, etc.

use std::path::PathBuf;

/// Configuration for mounting an external application.
///
/// Supports three rendering strategies:
/// - Static: Pre-rendered HTML (Fumadocs, Docusaurus, VitePress)
/// - SSR: Reverse proxy to another server (Next.js, Remix, separate Rust service)
/// - CSR: Client-side SPA with fallback routing (React, Vue, Angular)
#[derive(Debug, Clone)]
pub struct ExternalApp {
    /// Path where this app is mounted (e.g., "/docs", "/admin")
    pub mount_path: String,

    /// Rendering strategy for this external app
    pub strategy: ExternalAppStrategy,
}

#[derive(Debug, Clone)]
pub enum ExternalAppStrategy {
    /// Serve pre-rendered static files from a directory
    Static {
        /// Directory containing the built static files
        source_dir: PathBuf,

        /// Whether to serve index.html for 404s (for SPA fallback)
        spa_fallback: bool,
    },

    /// Reverse proxy to an upstream server
    Ssr {
        /// Upstream server URL (e.g., "http://localhost:4000")
        upstream: String,

        /// Optional command to run in development mode
        /// Example: "cd apps/admin && pnpm dev"
        dev_command: Option<String>,

        /// Optional health check endpoint to verify upstream is ready
        health_check: Option<String>,

        /// Whether to strip the mount path prefix when proxying
        /// Example: /admin/users → /users (if true) or /admin/users (if false)
        strip_prefix: bool,
    },

    /// Serve a client-side SPA with History API support
    Csr {
        /// Directory containing the built SPA files
        source_dir: PathBuf,

        /// In development, proxy to dev server for HMR
        /// Example: "http://localhost:5173"
        dev_upstream: Option<String>,
    },
}

impl ExternalApp {
    /// Create a new external app configuration
    pub fn new(mount_path: impl Into<String>, strategy: ExternalAppStrategy) -> Self {
        Self {
            mount_path: mount_path.into(),
            strategy,
        }
    }

    /// Create a static external app (e.g., Fumadocs, VitePress)
    pub fn static_app(mount_path: impl Into<String>, source_dir: impl Into<PathBuf>) -> Self {
        Self::new(
            mount_path,
            ExternalAppStrategy::Static {
                source_dir: source_dir.into(),
                spa_fallback: false,
            },
        )
    }

    /// Create a CSR external app (e.g., React, Vue SPA)
    pub fn csr_app(
        mount_path: impl Into<String>,
        source_dir: impl Into<PathBuf>,
        dev_upstream: Option<String>,
    ) -> Self {
        Self::new(
            mount_path,
            ExternalAppStrategy::Csr {
                source_dir: source_dir.into(),
                dev_upstream,
            },
        )
    }

    /// Create an SSR external app (e.g., Next.js, Remix)
    pub fn ssr_app(mount_path: impl Into<String>, upstream: String) -> Self {
        Self::new(
            mount_path,
            ExternalAppStrategy::Ssr {
                upstream,
                dev_command: None,
                health_check: None,
                strip_prefix: false,
            },
        )
    }
}
