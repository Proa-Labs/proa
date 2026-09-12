// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use super::{
    cookie, form, no_store, oauth_form, origin, pending_read, pending_save, redirect, required,
    session, set_cookie,
};
use crate::{
    app::AppState,
    error::{AppError, Result},
    security::{digest, now, password_valid, random},
};
use axum::{
    body::Bytes,
    extract::{RawQuery, State},
    http::HeaderMap,
    response::{IntoResponse, Response},
    Json,
};
use serde::{Deserialize, Serialize};
#[derive(Clone, Serialize, Deserialize)]
pub struct Authorization {
    pub client_id: String,
    pub client_name: String,
    pub redirect_uri: String,
    pub resource: String,
    pub scopes: String,
    pub challenge: String,
    pub state: String,
    pub nonce: Option<String>,
}
#[derive(Deserialize)]
pub struct Registration {
    pub client_name: Option<String>,
    pub redirect_uris: Vec<String>,
    pub token_endpoint_auth_method: Option<String>,
    pub grant_types: Option<Vec<String>>,
    pub response_types: Option<Vec<String>>,
}
pub async fn metadata(State(s): State<AppState>) -> Json<serde_json::Value> {
    Json(
        serde_json::json!({"issuer":s.config.issuer,"authorization_endpoint":format!("{}/authorize",s.config.issuer),"token_endpoint":format!("{}/token",s.config.issuer),"registration_endpoint":format!("{}/register",s.config.issuer),"jwks_uri":format!("{}/jwks.json",s.config.issuer),"revocation_endpoint":format!("{}/revoke",s.config.issuer),"response_types_supported":["code"],"grant_types_supported":["authorization_code","refresh_token"],"code_challenge_methods_supported":["S256"],"scopes_supported":["openid","profile","issues:read","issues:write"],"token_endpoint_auth_methods_supported":["none","client_secret_post"],"subject_types_supported":["public"],"id_token_signing_alg_values_supported":["RS256"],"authorization_response_iss_parameter_supported":true}),
    )
}
pub async fn jwks(State(s): State<AppState>) -> Json<serde_json::Value> {
    Json(s.keys.jwks.clone())
}
pub async fn resource(State(s): State<AppState>) -> Json<serde_json::Value> {
    Json(
        serde_json::json!({"resource":s.config.resource(),"authorization_servers":[s.config.issuer],"scopes_supported":["issues:read","issues:write"],"bearer_methods_supported":["header"],"resource_name":"Incident Desk"}),
    )
}
pub fn valid_redirect(value: &str) -> bool {
    url::Url::parse(value).is_ok_and(|u| {
        u.fragment().is_none()
            && u.username().is_empty()
            && u.password().is_none()
            && u.host_str().is_some()
            && (u.scheme() == "https"
                || (u.scheme() == "http"
                    && matches!(u.host_str(), Some("127.0.0.1" | "localhost" | "[::1]"))))
    })
}
pub async fn register(State(s): State<AppState>, Json(r): Json<Registration>) -> Result<Response> {
    s.rate_limit("register", 20)?;
    if r.redirect_uris.is_empty()
        || r.redirect_uris.len() > 5
        || r.redirect_uris
            .iter()
            .any(|u| u.len() > 2048 || !valid_redirect(u))
        || r.token_endpoint_auth_method
            .as_deref()
            .is_some_and(|a| a != "none")
        || r.grant_types.as_ref().is_some_and(|g| {
            g.iter()
                .any(|v| v != "authorization_code" && v != "refresh_token")
        })
        || r.response_types
            .as_ref()
            .is_some_and(|g| g.iter().any(|v| v != "code"))
    {
        return Err(AppError::Invalid("invalid_client_metadata"));
    }
    let name = r.client_name.unwrap_or_else(|| "MCP desktop client".into());
    if name.len() > 100 {
        return Err(AppError::Invalid("client name too long"));
    }
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM clients")
        .fetch_one(&s.db)
        .await?;
    if count >= 1000 {
        return Err(AppError::Limited);
    }
    let id = random();
    sqlx::query("INSERT INTO clients VALUES(?,?,?,NULL,?)")
        .bind(&id)
        .bind(&name)
        .bind(serde_json::to_string(&r.redirect_uris).map_err(anyhow::Error::from)?)
        .bind(now())
        .execute(&s.db)
        .await?;
    Ok(no_store((axum::http::StatusCode::CREATED,Json(serde_json::json!({"client_id":id,"client_name":name,"redirect_uris":r.redirect_uris,"token_endpoint_auth_method":"none","grant_types":["authorization_code","refresh_token"],"response_types":["code"]}))).into_response()))
}
pub async fn authorize(
    State(s): State<AppState>,
    headers: HeaderMap,
    RawQuery(query): RawQuery,
) -> Result<Response> {
    s.rate_limit("authorize", 120)?;
    let p = oauth_form(query.unwrap_or_default().as_bytes())?;
    let client = required(&p, "client_id")?;
    let redirect_uri = required(&p, "redirect_uri")?;
    let row: Option<(String, String)> =
        sqlx::query_as("SELECT name,redirects FROM clients WHERE id=?")
            .bind(client)
            .fetch_optional(&s.db)
            .await?;
    let (name, redirects) = row.ok_or(AppError::Invalid("unknown client; register first"))?;
    let allowed: Vec<String> = serde_json::from_str(&redirects).map_err(anyhow::Error::from)?;
    if !allowed.iter().any(|v| v == redirect_uri)
        || required(&p, "response_type")? != "code"
        || required(&p, "code_challenge_method")? != "S256"
    {
        return Err(AppError::Invalid("invalid authorization request"));
    }
    let challenge = required(&p, "code_challenge")?;
    if challenge.len() != 43
        || !challenge
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"-_".contains(&c))
    {
        return Err(AppError::Invalid("invalid S256 challenge"));
    }
    let scopes = p.get("scope").map(String::as_str).unwrap_or("issues:read");
    if !scopes.split_whitespace().any(|v| v == "issues:read")
        || scopes
            .split_whitespace()
            .any(|v| !matches!(v, "issues:read" | "issues:write" | "openid" | "profile"))
    {
        return Err(AppError::Invalid("invalid_scope"));
    }
    let resource = p
        .get("resource")
        .cloned()
        .unwrap_or_else(|| s.config.resource());
    if resource != s.config.resource()
        && !(client == "incident-web" && resource == s.config.app_origin)
    {
        return Err(AppError::Invalid("invalid_target"));
    }
    let state = required(&p, "state")?;
    if state.len() > 1024 {
        return Err(AppError::Invalid("state too long"));
    }
    let auth = Authorization {
        client_id: client.into(),
        client_name: name,
        redirect_uri: redirect_uri.into(),
        resource,
        scopes: scopes.into(),
        challenge: challenge.into(),
        state: state.into(),
        nonce: p.get("nonce").cloned(),
    };
    let flow = random();
    pending_save(&s, &flow, "authorize", &auth).await?;
    let existing = session(&s, &headers, "issuer").await.ok();
    let mut response = provider_page(&s, &auth, &flow, existing.as_ref(), None).await?;
    set_cookie(&mut response, "__Host-issuer_flow", &flow, 300);
    Ok(response)
}
async fn provider_page(
    s: &AppState,
    a: &Authorization,
    flow: &str,
    session: Option<&super::Session>,
    error: Option<&str>,
) -> Result<Response> {
    let user = if let Some(session) = session {
        let workspaces:Vec<(String,String,String)>=sqlx::query_as("SELECT w.id,w.name,m.role FROM memberships m JOIN workspaces w ON w.id=m.workspace_id WHERE m.user_id=?").bind(&session.user_id).fetch_all(&s.db).await?;
        Some((session.user_id.clone(), workspaces))
    } else {
        None
    };
    crate::pages::provider(s, a, flow, user.as_ref(), error)
}
pub async fn login(State(s): State<AppState>, headers: HeaderMap, body: Bytes) -> Result<Response> {
    origin(&headers, &s.config.issuer)?;
    let p = form(&body)?;
    let flow = required(&p, "flow")?;
    // Check expiry before the short-lived cookie: expired forms should report
    // an expired transaction, not an unrelated workspace permission failure.
    let a: Authorization = pending_read(&s, flow, "authorize").await?;
    if cookie(&headers, "__Host-issuer_flow").as_deref() != Some(flow) {
        tracing::warn!(
            reason = "flow_cookie_mismatch",
            "authorization login rejected"
        );
        return Err(AppError::Forbidden);
    }
    s.rate_limit("login", 20)?;
    let user = required(&p, "user")?.to_lowercase();
    let password = required(&p, "password")?.to_owned();
    let hash: Option<String> = sqlx::query_scalar("SELECT password_hash FROM users WHERE id=?")
        .bind(&user)
        .fetch_optional(&s.db)
        .await?;
    let valid = if let Some(hash) = hash {
        tokio::task::spawn_blocking(move || password_valid(&password, &hash))
            .await
            .map_err(anyhow::Error::from)?
    } else {
        false
    };
    if !valid {
        return provider_page(&s, &a, flow, None, Some("Account or password incorrect.")).await;
    }
    let workspace: String = sqlx::query_scalar(
        "SELECT workspace_id FROM memberships WHERE user_id=? ORDER BY workspace_id LIMIT 1",
    )
    .bind(&user)
    .fetch_one(&s.db)
    .await?;
    if let Some(old) = cookie(&headers, "__Host-issuer_sid") {
        sqlx::query("DELETE FROM sessions WHERE id_hash=? AND kind='issuer'")
            .bind(digest(&old))
            .execute(&s.db)
            .await?;
    }
    let token = super::new_session(&s, "issuer", &user, &workspace, "openid profile").await?;
    let session = super::Session {
        id_hash: digest(&token),
        user_id: user,
        workspace_id: workspace,
        scopes: "openid profile".into(),
        csrf: random(),
    };
    let mut r = provider_page(&s, &a, flow, Some(&session), None).await?;
    set_cookie(&mut r, "__Host-issuer_sid", &token, 28800);
    Ok(r)
}
pub async fn consent(
    State(s): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response> {
    origin(&headers, &s.config.issuer)?;
    let p = form(&body)?;
    let flow = required(&p, "flow")?;
    let a: Authorization = pending_read(&s, flow, "authorize").await?;
    if cookie(&headers, "__Host-issuer_flow").as_deref() != Some(flow) {
        tracing::warn!(
            reason = "flow_cookie_mismatch",
            "authorization consent rejected"
        );
        return Err(AppError::Forbidden);
    }
    let session = session(&s, &headers, "issuer").await?;
    let workspace = required(&p, "workspace")?;
    let mut tx = s.db.begin_with("BEGIN IMMEDIATE").await?;
    let role: String =
        sqlx::query_scalar("SELECT role FROM memberships WHERE user_id=? AND workspace_id=?")
            .bind(&session.user_id)
            .bind(workspace)
            .fetch_optional(&mut *tx)
            .await?
            .ok_or(AppError::Forbidden)?;
    if sqlx::query("DELETE FROM pending WHERE id_hash=? AND expires>?")
        .bind(digest(flow))
        .bind(now())
        .execute(&mut *tx)
        .await?
        .rows_affected()
        != 1
    {
        return Err(AppError::Invalid("consent expired"));
    }
    let mut callback = url::Url::parse(&a.redirect_uri).map_err(anyhow::Error::from)?;
    callback
        .query_pairs_mut()
        .append_pair("state", &a.state)
        .append_pair("iss", &s.config.issuer);
    if required(&p, "decision")? == "deny" {
        callback
            .query_pairs_mut()
            .append_pair("error", "access_denied");
    } else if required(&p, "decision")? == "approve" {
        let scopes = a
            .scopes
            .split_whitespace()
            .filter(|v| *v != "issues:write" || role == "maintainer")
            .collect::<Vec<_>>()
            .join(" ");
        let count: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM grants WHERE user_id=? AND revoked=0 AND expires>?",
        )
        .bind(&session.user_id)
        .bind(now())
        .fetch_one(&mut *tx)
        .await?;
        if count >= 100 {
            return Err(AppError::Limited);
        }
        let grant = random();
        sqlx::query("INSERT INTO grants VALUES(?,?,?,?,?,?,?,0)")
            .bind(&grant)
            .bind(&session.user_id)
            .bind(workspace)
            .bind(&a.client_id)
            .bind(scopes)
            .bind(&a.resource)
            .bind(now() + 28800)
            .execute(&mut *tx)
            .await?;
        let code = random();
        sqlx::query("INSERT INTO codes VALUES(?,?,?,?,?,?,0)")
            .bind(digest(&code))
            .bind(grant)
            .bind(a.redirect_uri)
            .bind(a.challenge)
            .bind(a.nonce)
            .bind(now() + 60)
            .execute(&mut *tx)
            .await?;
        callback.query_pairs_mut().append_pair("code", &code);
    } else {
        return Err(AppError::Invalid("invalid consent decision"));
    }
    tx.commit().await?;
    let mut r = redirect(callback.as_str());
    set_cookie(&mut r, "__Host-issuer_flow", "", 0);
    Ok(r)
}
pub async fn token(State(s): State<AppState>, body: Bytes) -> Response {
    let result = async { super::tokens::exchange(&s, oauth_form(&body)?).await }.await;
    no_store(match result{Ok(tokens)=>Json(tokens).into_response(),Err(e)=>(axum::http::StatusCode::BAD_REQUEST,Json(serde_json::json!({"error":if matches!(e,AppError::Unauthorized){"invalid_client"}else{"invalid_grant"},"error_description":e.to_string()}))).into_response()})
}
pub async fn revoke(State(s): State<AppState>, body: Bytes) -> Result<Response> {
    let p = form(&body)?;
    let client = required(&p, "client_id")?;
    let hash: Option<Option<String>> =
        sqlx::query_scalar("SELECT secret_hash FROM clients WHERE id=?")
            .bind(client)
            .fetch_optional(&s.db)
            .await?;
    let Some(hash) = hash else {
        return Err(AppError::Unauthorized);
    };
    // A client registered without a secret is public and authenticates by id alone.
    if let Some(hash) = hash {
        if !crate::security::equal(&hash, &digest(required(&p, "client_secret")?)) {
            return Err(AppError::Unauthorized);
        }
    }
    let token = required(&p, "token")?;
    let grant = if let Ok(c) = super::tokens::decode(&s, token, &s.config.resource(), false) {
        Some(c.grant_id)
    } else {
        sqlx::query_scalar::<_, String>("SELECT grant_id FROM refresh_tokens WHERE hash=?")
            .bind(digest(token))
            .fetch_optional(&s.db)
            .await?
    };
    if let Some(g) = grant {
        sqlx::query("UPDATE grants SET revoked=1 WHERE id=? AND client_id=?")
            .bind(g)
            .bind(client)
            .execute(&s.db)
            .await?;
    }
    Ok(no_store(axum::http::StatusCode::OK.into_response()))
}
