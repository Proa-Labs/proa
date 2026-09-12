// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use super::{cookie, form, pending_read, pending_save, redirect, required, set_cookie};
use crate::{
    app::AppState,
    error::{AppError, Result},
    security::{digest, equal, now, random},
};
use axum::{
    body::Bytes,
    extract::{RawQuery, State},
    http::HeaderMap,
    response::Response,
};
use serde::{Deserialize, Serialize};
#[derive(Serialize, Deserialize)]
struct Login {
    state: String,
    nonce: String,
    verifier: String,
    return_to: String,
}
pub async fn login(State(s): State<AppState>, RawQuery(query): RawQuery) -> Result<Response> {
    s.rate_limit("browser-login", 120)?;
    let p = form(query.unwrap_or_default().as_bytes())?;
    let return_to = p
        .get("return_to")
        .cloned()
        .unwrap_or_else(|| "/issues".into());
    if !return_to.starts_with('/')
        || return_to.starts_with("//")
        || return_to.contains('\\')
        || return_to.chars().any(char::is_control)
    {
        return Err(AppError::Invalid("invalid return path"));
    }
    let flow = random();
    let login = Login {
        state: random(),
        nonce: random(),
        verifier: random(),
        return_to,
    };
    pending_save(&s, &flow, "web-login", &login).await?;
    let mut url =
        url::Url::parse(&format!("{}/authorize", s.config.issuer)).map_err(anyhow::Error::from)?;
    url.query_pairs_mut().extend_pairs([
        ("client_id", "incident-web"),
        ("redirect_uri", s.config.callback().as_str()),
        ("response_type", "code"),
        ("code_challenge_method", "S256"),
        ("code_challenge", digest(&login.verifier).as_str()),
        ("state", login.state.as_str()),
        ("nonce", login.nonce.as_str()),
        ("scope", "openid profile issues:read issues:write"),
        ("resource", s.config.app_origin.as_str()),
    ]);
    let mut r = redirect(url.as_str());
    set_cookie(&mut r, "__Host-login_flow", &flow, 300);
    Ok(r)
}
pub async fn callback(
    State(s): State<AppState>,
    headers: HeaderMap,
    RawQuery(query): RawQuery,
) -> Result<Response> {
    let p = form(query.unwrap_or_default().as_bytes())?;
    let flow = cookie(&headers, "__Host-login_flow").ok_or(AppError::Unauthorized)?;
    let login: Login = pending_read(&s, &flow, "web-login").await?;
    if !equal(required(&p, "state")?, &login.state) || required(&p, "iss")? != s.config.issuer {
        return Err(AppError::Unauthorized);
    }
    if p.contains_key("error") {
        sqlx::query("DELETE FROM pending WHERE id_hash=?")
            .bind(digest(&flow))
            .execute(&s.db)
            .await?;
        return Err(AppError::Forbidden);
    }
    let tokens = s
        .http
        .post(format!("{}/token", s.config.issuer))
        .form(&[
            ("grant_type", "authorization_code"),
            ("code", required(&p, "code")?),
            ("redirect_uri", s.config.callback().as_str()),
            ("client_id", "incident-web"),
            ("client_secret", s.keys.client_secret.as_str()),
            ("code_verifier", login.verifier.as_str()),
            ("resource", s.config.app_origin.as_str()),
        ])
        .send()
        .await
        .map_err(anyhow::Error::from)?
        .error_for_status()
        .map_err(anyhow::Error::from)?
        .json::<super::tokens::TokenResponse>()
        .await
        .map_err(anyhow::Error::from)?;
    let claims = super::tokens::decode(
        &s,
        tokens.id_token.as_deref().ok_or(AppError::Unauthorized)?,
        "incident-web",
        true,
    )?;
    if claims.nonce.as_deref() != Some(&login.nonce) {
        return Err(AppError::Unauthorized);
    }
    super::principal(
        &s,
        &claims.sub,
        &claims.workspace_id,
        &claims.scope,
        "incident-web",
        "",
        "browser",
    )
    .await?;
    let mut tx = s.db.begin_with("BEGIN IMMEDIATE").await?;
    if sqlx::query("DELETE FROM pending WHERE id_hash=? AND expires>?")
        .bind(digest(&flow))
        .bind(now())
        .execute(&mut *tx)
        .await?
        .rows_affected()
        != 1
    {
        return Err(AppError::Unauthorized);
    }
    if let Some(old) = cookie(&headers, "__Host-incident_sid") {
        sqlx::query("DELETE FROM sessions WHERE id_hash=?")
            .bind(digest(&old))
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await?;
    let token =
        super::new_session(&s, "app", &claims.sub, &claims.workspace_id, &claims.scope).await?;
    let mut r = redirect(&login.return_to);
    set_cookie(&mut r, "__Host-incident_sid", &token, 28800);
    set_cookie(&mut r, "__Host-login_flow", "", 0);
    Ok(r)
}
pub async fn logout(
    State(s): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response> {
    super::origin(&headers, &s.config.app_origin)?;
    let p = form(&body)?;
    let session = super::session(&s, &headers, "app").await?;
    super::csrf(&session, required(&p, "csrf")?)?;
    sqlx::query("DELETE FROM sessions WHERE id_hash=?")
        .bind(session.id_hash)
        .execute(&s.db)
        .await?;
    let mut r = redirect("/");
    set_cookie(&mut r, "__Host-incident_sid", "", 0);
    Ok(r)
}
