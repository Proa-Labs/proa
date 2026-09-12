// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

#![allow(dead_code)]
use mcp_incident_desk::{
    app::AppState,
    auth::tokens::TokenResponse,
    config::Config,
    security::{digest, random},
};
use rmcp::{
    model::ClientInfo,
    transport::{
        streamable_http_client::StreamableHttpClientTransportConfig, StreamableHttpClientTransport,
    },
    ServiceExt,
};
use std::{
    collections::HashMap,
    path::PathBuf,
    process::{Command, Stdio},
    sync::OnceLock,
};
pub const PASSWORD: &str = "fixture-password-used-only-in-isolated-tests";
fn tls_material() -> PathBuf {
    static MATERIAL: OnceLock<tempfile::TempDir> = OnceLock::new();
    MATERIAL.get_or_init(||{let d=tempfile::tempdir().unwrap();let run=|args:&[&str]|{assert!(Command::new("openssl").args(args).current_dir(d.path()).stdout(Stdio::null()).stderr(Stdio::null()).status().unwrap().success());};run(&["req","-x509","-newkey","rsa:2048","-nodes","-days","2","-keyout","ca.key","-out","ca.pem","-subj","/CN=Incident Desk Test CA"]);run(&["req","-newkey","rsa:2048","-nodes","-keyout","tls.key","-out","tls.csr","-subj","/CN=localhost"]);std::fs::write(d.path().join("tls.ext"),"subjectAltName=DNS:localhost,IP:127.0.0.1\nextendedKeyUsage=serverAuth\nbasicConstraints=CA:FALSE\n").unwrap();run(&["x509","-req","-in","tls.csr","-CA","ca.pem","-CAkey","ca.key","-CAcreateserial","-out","tls.pem","-days","2","-extfile","tls.ext"]);d}).path().to_owned()
}
pub struct Harness {
    pub state: AppState,
    pub dir: tempfile::TempDir,
    pub app: String,
    pub issuer: String,
    handles: Vec<tokio::task::JoinHandle<std::io::Result<()>>>,
}
impl Harness {
    pub async fn new() -> Self {
        let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
        let dir = tempfile::tempdir().unwrap();
        let private = dir.path().join("private");
        std::fs::create_dir(&private).unwrap();
        for name in ["ca.pem", "tls.pem", "tls.key"] {
            std::fs::copy(tls_material().join(name), private.join(name)).unwrap();
        }
        let app_listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let issuer_listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let app_port = app_listener.local_addr().unwrap().port();
        let issuer_port = issuer_listener.local_addr().unwrap().port();
        let app = format!("https://localhost:{app_port}");
        let issuer = format!("https://127.0.0.1:{issuer_port}");
        let config = Config {
            database_url: format!("sqlite://{}", dir.path().join("test.db").display()),
            app_origin: app.clone(),
            issuer: issuer.clone(),
            private_dir: private.clone(),
            app_port,
            issuer_port,
        };
        let state = AppState::open(config).await.unwrap();
        let hash = mcp_incident_desk::security::password_hash(PASSWORD).unwrap();
        sqlx::query("UPDATE users SET password_hash=?")
            .bind(hash)
            .execute(&state.db)
            .await
            .unwrap();
        let tls = axum_server::tls_rustls::RustlsConfig::from_pem_file(
            private.join("tls.pem"),
            private.join("tls.key"),
        )
        .await
        .unwrap();
        let app_service = mcp_incident_desk::routes::router(state.clone());
        let issuer_service = mcp_incident_desk::routes::issuer(state.clone());
        let t = tls.clone();
        let handles = vec![
            tokio::spawn(async move {
                axum_server::from_tcp_rustls(app_listener, t)
                    .serve(app_service.into_make_service())
                    .await
            }),
            tokio::spawn(async move {
                axum_server::from_tcp_rustls(issuer_listener, tls)
                    .serve(issuer_service.into_make_service())
                    .await
            }),
        ];
        Self {
            state,
            dir,
            app,
            issuer,
            handles,
        }
    }
    pub fn browser(&self) -> reqwest::Client {
        reqwest::Client::builder()
            .add_root_certificate(
                reqwest::Certificate::from_pem(
                    &std::fs::read(self.state.config.private_dir.join("ca.pem")).unwrap(),
                )
                .unwrap(),
            )
            .cookie_store(true)
            .redirect(reqwest::redirect::Policy::none())
            .timeout(std::time::Duration::from_secs(20))
            .build()
            .unwrap()
    }
    pub async fn consent(
        &self,
        b: &reqwest::Client,
        url: &str,
        user: &str,
        workspace: &str,
    ) -> String {
        let page = b.get(url).send().await.unwrap();
        assert!(page.status().is_success());
        // A real browser sends Origin: null with `no-referrer`, even on same-origin
        // form POSTs. Do not let our explicit Origin headers hide that regression.
        assert_eq!(page.headers()["referrer-policy"], "same-origin");
        let body = page.text().await.unwrap();
        let flow = hidden(&body, "flow");
        let page = b
            .post(format!("{}/authorize/login", self.issuer))
            .header("origin", &self.issuer)
            .form(&[
                ("flow", flow.as_str()),
                ("user", user),
                ("password", PASSWORD),
            ])
            .send()
            .await
            .unwrap();
        assert!(page.status().is_success());
        assert_eq!(page.headers()["referrer-policy"], "same-origin");
        assert!(page.text().await.unwrap().contains("Approve access"));
        let consent = b
            .post(format!("{}/consent", self.issuer))
            .header("origin", &self.issuer)
            .form(&[
                ("flow", flow.as_str()),
                ("workspace", workspace),
                ("decision", "approve"),
            ])
            .send()
            .await
            .unwrap();
        assert_eq!(consent.status(), reqwest::StatusCode::SEE_OTHER);
        consent
            .headers()
            .get("location")
            .unwrap()
            .to_str()
            .unwrap()
            .into()
    }
    pub async fn login_browser(&self, user: &str, workspace: &str) -> reqwest::Client {
        let b = self.browser();
        let start = b
            .get(format!("{}/auth/login", self.app))
            .send()
            .await
            .unwrap();
        assert_eq!(start.status(), reqwest::StatusCode::SEE_OTHER);
        let auth = start.headers().get("location").unwrap().to_str().unwrap();
        let callback = self.consent(&b, auth, user, workspace).await;
        let result = b.get(callback).send().await.unwrap();
        assert_eq!(result.status(), reqwest::StatusCode::SEE_OTHER);
        b
    }
    pub async fn code(&self, user: &str, workspace: &str, write: bool) -> (String, String) {
        let b = self.browser();
        let verifier = random();
        let mut url = url::Url::parse(&format!("{}/authorize", self.issuer)).unwrap();
        url.query_pairs_mut().extend_pairs([
            ("client_id", "incident-demo-client"),
            ("redirect_uri", "http://127.0.0.1:5572/callback"),
            ("response_type", "code"),
            ("code_challenge_method", "S256"),
            ("code_challenge", digest(&verifier).as_str()),
            ("state", "test-state-bound-to-client"),
            (
                "scope",
                if write {
                    "issues:read issues:write"
                } else {
                    "issues:read"
                },
            ),
            ("resource", self.state.config.resource().as_str()),
            ("resource", self.state.config.resource().as_str()),
        ]);
        let callback = self.consent(&b, url.as_str(), user, workspace).await;
        let params: HashMap<_, _> = url::Url::parse(&callback)
            .unwrap()
            .query_pairs()
            .into_owned()
            .collect();
        assert_eq!(params["state"], "test-state-bound-to-client");
        assert_eq!(params["iss"], self.issuer);
        (params["code"].clone(), verifier)
    }
    pub async fn exchange(&self, code: &str, verifier: &str) -> reqwest::Response {
        self.browser()
            .post(format!("{}/token", self.issuer))
            .form(&[
                ("resource", self.state.config.resource().as_str()),
                ("grant_type", "authorization_code"),
                ("client_id", "incident-demo-client"),
                ("redirect_uri", "http://127.0.0.1:5572/callback"),
                ("code", code),
                ("code_verifier", verifier),
                ("resource", self.state.config.resource().as_str()),
            ])
            .send()
            .await
            .unwrap()
    }
    pub async fn tokens(&self, user: &str, workspace: &str, write: bool) -> TokenResponse {
        let (c, v) = self.code(user, workspace, write).await;
        let r = self.exchange(&c, &v).await;
        assert!(r.status().is_success());
        r.json().await.unwrap()
    }
    pub async fn sdk(
        &self,
        token: &str,
    ) -> rmcp::service::RunningService<rmcp::RoleClient, ClientInfo> {
        let transport = StreamableHttpClientTransport::with_client(
            self.browser(),
            StreamableHttpClientTransportConfig::with_uri(self.state.config.resource())
                .auth_header(token),
        );
        ClientInfo::default().serve(transport).await.unwrap()
    }
}
impl Drop for Harness {
    fn drop(&mut self) {
        for h in &self.handles {
            h.abort();
        }
    }
}
pub fn hidden(html: &str, name: &str) -> String {
    regex::Regex::new(&format!(
        r#"name="{}" value="([^"]*)""#,
        regex::escape(name)
    ))
    .unwrap()
    .captures(html)
    .unwrap_or_else(|| panic!("missing form input {name}"))[1]
        .to_owned()
}
