// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Installer scripts served straight out of the binary.
//!
//! The bodies are embedded with `include_str!` rather than read from disk so
//! the served copy can never drift from the repo copy and the container image
//! stays self-contained.

use axum::http::header;

/// POSIX shell installer, embedded verbatim from its standalone source.
const INSTALL_SH: &str = include_str!("../../../scripts/install.sh");

/// PowerShell installer, embedded verbatim from its standalone source.
const INSTALL_PS1: &str = include_str!("../../../scripts/install.ps1");

/// Short on purpose: an installer that caches for a day is an installer you
/// cannot fix.
const INSTALLER_CACHE_CONTROL: &str = "public, max-age=300";

pub async fn shell_handler() -> impl axum::response::IntoResponse {
    (
        [
            (header::CONTENT_TYPE, "text/x-shellscript; charset=utf-8"),
            (header::CACHE_CONTROL, INSTALLER_CACHE_CONTROL),
            (header::X_CONTENT_TYPE_OPTIONS, "nosniff"),
        ],
        INSTALL_SH,
    )
}

/// PowerShell's `irm` ignores the content type, and `text/plain` keeps browsers
/// from offering the script as a download.
pub async fn powershell_handler() -> impl axum::response::IntoResponse {
    (
        [
            (header::CONTENT_TYPE, "text/plain; charset=utf-8"),
            (header::CACHE_CONTROL, INSTALLER_CACHE_CONTROL),
            (header::X_CONTENT_TYPE_OPTIONS, "nosniff"),
        ],
        INSTALL_PS1,
    )
}
