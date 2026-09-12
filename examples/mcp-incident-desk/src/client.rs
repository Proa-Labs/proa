// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! OAuth reference client and desktop stdio bridge. Credentials stay in this process.
use crate::{
    auth::tokens::TokenResponse,
    security::{digest, random},
};
use rmcp::{
    model::*,
    service::RunningService,
    transport::{
        streamable_http_client::StreamableHttpClientTransportConfig, StreamableHttpClientTransport,
    },
    RoleClient, ServiceExt,
};
use std::{
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::sync::Mutex;
struct Credentials {
    tokens: TokenResponse,
    expires: Instant,
    client_id: String,
    token_endpoint: String,
}
#[derive(Clone)]
pub struct OAuthClient {
    pub http: reqwest::Client,
    pub resource: String,
    credentials: Arc<Mutex<Option<Credentials>>>,
    pub bridge: bool,
    pub write: bool,
}
fn endpoint(metadata: &serde_json::Value, key: &str, issuer: &url::Url) -> anyhow::Result<String> {
    let value = metadata[key]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("missing OAuth endpoint"))?;
    let url = url::Url::parse(value)?;
    anyhow::ensure!(
        url.origin() == issuer.origin() && url.scheme() == "https",
        "untrusted OAuth endpoint"
    );
    Ok(value.into())
}
impl OAuthClient {
    pub fn new(
        resource: String,
        ca: Option<&std::path::Path>,
        bridge: bool,
        write: bool,
    ) -> anyhow::Result<Self> {
        let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
        let mut builder = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(Duration::from_secs(30));
        if let Some(path) = ca {
            builder = builder
                .add_root_certificate(reqwest::Certificate::from_pem(&std::fs::read(path)?)?);
        }
        anyhow::ensure!(
            url::Url::parse(&resource)?.scheme() == "https",
            "MCP resource must use HTTPS"
        );
        Ok(Self {
            http: builder.build()?,
            resource,
            credentials: Default::default(),
            bridge,
            write,
        })
    }
    async fn authorize(&self) -> anyhow::Result<Credentials> {
        let challenge = self.http.get(&self.resource).send().await?;
        anyhow::ensure!(
            challenge.status() == reqwest::StatusCode::UNAUTHORIZED,
            "expected an OAuth challenge"
        );
        let header = challenge
            .headers()
            .get("www-authenticate")
            .and_then(|v| v.to_str().ok())
            .ok_or_else(|| anyhow::anyhow!("missing auth challenge"))?;
        let metadata_url = header
            .split("resource_metadata=\"")
            .nth(1)
            .and_then(|v| v.split('"').next())
            .ok_or_else(|| anyhow::anyhow!("missing resource metadata URL"))?;
        anyhow::ensure!(
            url::Url::parse(metadata_url)?.origin() == url::Url::parse(&self.resource)?.origin(),
            "resource discovery left the configured origin"
        );
        let metadata: serde_json::Value = self
            .http
            .get(metadata_url)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        anyhow::ensure!(
            metadata["resource"].as_str() == Some(&self.resource),
            "resource metadata mismatch"
        );
        let issuer = url::Url::parse(
            metadata["authorization_servers"][0]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("missing issuer"))?,
        )?;
        anyhow::ensure!(
            issuer.scheme() == "https" && issuer.path() == "/",
            "issuer must be an HTTPS origin"
        );
        let config: serde_json::Value = self
            .http
            .get(issuer.join("/.well-known/oauth-authorization-server")?)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        anyhow::ensure!(
            config["issuer"]
                .as_str()
                .map(url::Url::parse)
                .transpose()?
                .as_ref()
                == Some(&issuer),
            "issuer metadata mismatch"
        );
        let authorization_endpoint = endpoint(&config, "authorization_endpoint", &issuer)?;
        let token_endpoint = endpoint(&config, "token_endpoint", &issuer)?;
        let listener = tokio::net::TcpListener::bind(if self.bridge {
            "127.0.0.1:0"
        } else {
            "127.0.0.1:5572"
        })
        .await?;
        let callback = format!(
            "http://127.0.0.1:{}/callback",
            listener.local_addr()?.port()
        );
        let client_id = if self.bridge {
            let registration:serde_json::Value=self.http.post(endpoint(&config,"registration_endpoint",&issuer)?).json(&serde_json::json!({"client_name":"Incident Desk desktop OAuth bridge","redirect_uris":[callback],"token_endpoint_auth_method":"none","grant_types":["authorization_code","refresh_token"],"response_types":["code"]})).send().await?.error_for_status()?.json().await?;
            registration["client_id"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("registration omitted client_id"))?
                .to_owned()
        } else {
            "incident-demo-client".into()
        };
        let state = random();
        let verifier = random();
        let mut url = url::Url::parse(&authorization_endpoint)?;
        url.query_pairs_mut().extend_pairs([
            ("client_id", client_id.as_str()),
            ("redirect_uri", callback.as_str()),
            ("response_type", "code"),
            ("code_challenge_method", "S256"),
            ("code_challenge", digest(&verifier).as_str()),
            ("state", state.as_str()),
            (
                "scope",
                if self.write {
                    "issues:read issues:write"
                } else {
                    "issues:read"
                },
            ),
            ("resource", self.resource.as_str()),
        ]);
        let (send, receive) = tokio::sync::oneshot::channel();
        let callback_state = Callback {
            expected_state: state,
            issuer: config["issuer"].as_str().unwrap().into(),
            sender: Arc::new(Mutex::new(Some(send))),
        };
        let app = axum::Router::new()
            .route("/callback", axum::routing::get(receive_callback))
            .with_state(callback_state);
        let server = tokio::spawn(async move { axum::serve(listener, app).await });
        eprintln!("Incident Desk: finish sign-in and consent in your browser.");
        // stdout is the JSON-RPC channel in bridge mode; never let a child write to it.
        let opened = std::process::Command::new("open")
            .arg(url.as_str())
            .stdout(std::process::Stdio::null())
            .status()?;
        anyhow::ensure!(opened.success(), "could not open browser");
        let received = tokio::time::timeout(Duration::from_secs(300), receive).await;
        server.abort();
        let code = received???;
        let tokens: TokenResponse = self
            .http
            .post(&token_endpoint)
            .form(&[
                ("grant_type", "authorization_code"),
                ("client_id", client_id.as_str()),
                ("code", code.as_str()),
                ("redirect_uri", callback.as_str()),
                ("code_verifier", verifier.as_str()),
                ("resource", self.resource.as_str()),
            ])
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        let expires = Instant::now() + Duration::from_secs(tokens.expires_in.max(0) as u64);
        Ok(Credentials {
            tokens,
            expires,
            client_id,
            token_endpoint,
        })
    }
    pub async fn token(&self) -> anyhow::Result<String> {
        let mut guard = self.credentials.lock().await;
        if guard.is_none() {
            *guard = Some(self.authorize().await?);
        }
        let c = guard.as_mut().unwrap();
        if c.expires <= Instant::now() + Duration::from_secs(30) {
            let tokens: TokenResponse = self
                .http
                .post(&c.token_endpoint)
                .form(&[
                    ("grant_type", "refresh_token"),
                    ("client_id", c.client_id.as_str()),
                    ("refresh_token", c.tokens.refresh_token.as_str()),
                    ("resource", self.resource.as_str()),
                ])
                .send()
                .await?
                .error_for_status()?
                .json()
                .await?;
            c.expires = Instant::now() + Duration::from_secs(tokens.expires_in.max(0) as u64);
            c.tokens = tokens;
        }
        Ok(c.tokens.access_token.clone())
    }
    pub async fn connect(&self) -> anyhow::Result<RunningService<RoleClient, ClientInfo>> {
        let token = self.token().await?;
        let transport = StreamableHttpClientTransport::with_client(
            self.http.clone(),
            StreamableHttpClientTransportConfig::with_uri(self.resource.clone()).auth_header(token),
        );
        Ok(ClientInfo::default().serve(transport).await?)
    }
}
#[derive(Clone)]
struct Callback {
    expected_state: String,
    issuer: String,
    sender: Arc<Mutex<Option<tokio::sync::oneshot::Sender<anyhow::Result<String>>>>>,
}
async fn receive_callback(
    axum::extract::State(state): axum::extract::State<Callback>,
    axum::extract::RawQuery(query): axum::extract::RawQuery,
) -> axum::response::Response {
    use axum::response::IntoResponse;
    let p = crate::auth::form(query.unwrap_or_default().as_bytes()).unwrap_or_default();
    if !p
        .get("state")
        .is_some_and(|s| crate::security::equal(s, &state.expected_state))
        || p.get("iss") != Some(&state.issuer)
    {
        return axum::http::StatusCode::BAD_REQUEST.into_response();
    }
    let result = if p.contains_key("error") {
        Err(anyhow::anyhow!("authorization denied"))
    } else {
        p.get("code")
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("missing authorization code"))
    };
    if let Some(send) = state.sender.lock().await.take() {
        let _ = send.send(result);
    }
    axum::response::Html("<html><head><meta name=referrer content=no-referrer></head><body><h1>Connection authorized</h1><p>Return to your desktop app.</p></body></html>").into_response()
}
#[derive(Clone)]
pub struct DesktopBridge {
    pub client: OAuthClient,
}
impl rmcp::ServerHandler for DesktopBridge {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
    }
    async fn list_tools(
        &self,
        _: Option<PaginatedRequestParams>,
        _: rmcp::service::RequestContext<rmcp::RoleServer>,
    ) -> Result<ListToolsResult, rmcp::ErrorData> {
        // Static schemas allow desktop initialization before the first browser sign-in.
        Ok(ListToolsResult {
            tools: crate::mcp::tool_definitions(),
            ..Default::default()
        })
    }
    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        _: rmcp::service::RequestContext<rmcp::RoleServer>,
    ) -> Result<CallToolResponse, rmcp::ErrorData> {
        let result = async {
            let upstream = self.client.connect().await?;
            let result = upstream.call_tool(request).await?;
            upstream.cancel().await?;
            Ok::<_, anyhow::Error>(result)
        }
        .await;
        Ok(match result {
            Ok(r) => r.into(),
            Err(err) => {
                // The desktop log is the only place this cause is visible.
                eprintln!("Incident Desk: tool call failed: {err:#}");
                CallToolResult::error(vec![ContentBlock::text("Connection failed or authorization expired/revoked. Check the local server and reconnect to authorize again.")]).into()
            }
        })
    }
}
