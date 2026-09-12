// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Installer scripts served straight out of the binary.
//!
//! The bodies are embedded with `include_str!` rather than read from disk so
//! the served copy can never drift from the repo copy and the container image
//! stays self-contained.

use axum::http::header;

/// Body of `curl -fsSL https://proa.so/install.sh | sh`.
const INSTALL_SH: &str = include_str!("../../../scripts/install.sh");

/// Body of `irm https://proa.so/install.ps1 | iex`.
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

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::to_bytes;
    use axum::response::IntoResponse;

    async fn body_of(response: axum::response::Response) -> String {
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        String::from_utf8(body.to_vec()).unwrap()
    }

    #[tokio::test]
    async fn shell_installer_is_served_as_a_cacheable_script() {
        let response = shell_handler().await.into_response();
        assert_eq!(response.status(), axum::http::StatusCode::OK);
        assert_eq!(
            response.headers().get(header::CONTENT_TYPE).unwrap(),
            "text/x-shellscript; charset=utf-8"
        );
        assert_eq!(
            response.headers().get(header::CACHE_CONTROL).unwrap(),
            "public, max-age=300"
        );
        assert_eq!(
            response
                .headers()
                .get(header::X_CONTENT_TYPE_OPTIONS)
                .unwrap(),
            "nosniff"
        );
    }

    #[tokio::test]
    async fn powershell_installer_is_served_as_plain_text() {
        let response = powershell_handler().await.into_response();
        assert_eq!(response.status(), axum::http::StatusCode::OK);
        assert_eq!(
            response.headers().get(header::CONTENT_TYPE).unwrap(),
            "text/plain; charset=utf-8"
        );
        assert_eq!(
            response.headers().get(header::CACHE_CONTROL).unwrap(),
            "public, max-age=300"
        );
        assert_eq!(
            response
                .headers()
                .get(header::X_CONTENT_TYPE_OPTIONS)
                .unwrap(),
            "nosniff"
        );
    }

    /// A pipe that truncates mid-download must not leave a runnable script.
    /// Deferring all work to a `main` invoked on the last line means a partial
    /// body defines functions and then dies without doing anything.
    #[tokio::test]
    async fn shell_installer_runs_nothing_until_its_final_line() {
        let body = body_of(shell_handler().await.into_response()).await;
        assert!(
            body.starts_with("#!"),
            "install.sh must open with a shebang, got: {:?}",
            body.lines().next()
        );
        let last = body
            .lines()
            .rev()
            .find(|line| !line.trim().is_empty())
            .expect("install.sh must have a non-empty line");
        assert_eq!(
            last.trim(),
            "main \"$@\"",
            "install.sh must end by invoking main so a truncated download is inert"
        );
    }

    /// CRLF line endings make `sh` fail with a baffling error.
    #[tokio::test]
    async fn shell_installer_has_no_crlf_line_endings() {
        let body = body_of(shell_handler().await.into_response()).await;
        assert!(
            !body.contains('\r'),
            "install.sh must use LF line endings only"
        );
    }

    #[tokio::test]
    async fn installers_are_non_empty_and_point_at_the_repository() {
        let shell = body_of(shell_handler().await.into_response()).await;
        let powershell = body_of(powershell_handler().await.into_response()).await;

        assert!(!shell.trim().is_empty(), "install.sh must not be empty");
        assert!(
            !powershell.trim().is_empty(),
            "install.ps1 must not be empty"
        );
        assert!(
            shell.contains("Proa-Labs/proa"),
            "install.sh must reference the Proa-Labs/proa repository"
        );
        assert!(
            powershell.contains("Proa-Labs/proa"),
            "install.ps1 must reference the Proa-Labs/proa repository"
        );
    }

    #[tokio::test]
    async fn shell_installer_pins_and_verifies_tailwind_v4() {
        let shell = body_of(shell_handler().await.into_response()).await;

        assert!(shell.contains("TAILWIND_VERSION=\"4.3.3\""));
        assert!(shell.contains("--with-tailwind"));
        assert!(shell.contains("--no-tailwind"));
        assert!(shell.contains("--tailwind-only"));
        assert!(shell.contains("tailwindlabs/tailwindcss/releases/download"));
        assert!(shell.contains("cdf646702987a743464dff4d9c60fd4480d1c1e73dd819a9a67f1078815dce9d"));
    }
}
