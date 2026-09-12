// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

pub mod browser;
pub mod provider;
pub mod tokens;
use crate::{
    app::AppState,
    domain::Principal,
    error::{AppError, Result},
    security::{digest, equal, now, random},
};
use axum::{
    http::{header, HeaderMap},
    response::{IntoResponse, Response},
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
#[derive(Clone, Debug, sqlx::FromRow)]
pub struct Session {
    pub id_hash: String,
    pub user_id: String,
    pub workspace_id: String,
    pub scopes: String,
    pub csrf: String,
}
pub fn form(bytes: &[u8]) -> Result<HashMap<String, String>> {
    parse_form(bytes, false)
}
pub fn oauth_form(bytes: &[u8]) -> Result<HashMap<String, String>> {
    parse_form(bytes, true)
}
fn parse_form(bytes: &[u8], repeated_resource: bool) -> Result<HashMap<String, String>> {
    if bytes.len() > 16384 {
        return Err(AppError::Invalid("form too large"));
    }
    let mut p = HashMap::new();
    for (k, v) in url::form_urlencoded::parse(bytes) {
        if repeated_resource && k == "resource" {
            if let Some(previous) = p.get(k.as_ref()) {
                // RFC 8707 permits repeated resource parameters. This issuer supports
                // one audience per grant, including Codex's repeated identical target.
                if previous != v.as_ref() {
                    return Err(AppError::Invalid("invalid_target"));
                }
                continue;
            }
        }
        if p.insert(k.into_owned(), v.into_owned()).is_some() {
            return Err(AppError::Invalid("duplicate parameter"));
        }
    }
    Ok(p)
}
pub fn required<'a>(p: &'a HashMap<String, String>, key: &str) -> Result<&'a str> {
    p.get(key)
        .map(String::as_str)
        .filter(|s| !s.is_empty())
        .ok_or(AppError::Invalid("missing parameter"))
}
pub fn cookie(headers: &HeaderMap, name: &str) -> Option<String> {
    let mut found = None;
    for value in headers.get_all(header::COOKIE) {
        for item in value.to_str().ok()?.split(';') {
            let (k, v) = item.trim().split_once('=')?;
            if k == name {
                if found.is_some() {
                    return None;
                }
                found = Some(v.to_owned());
            }
        }
    }
    found
}
pub fn set_cookie(response: &mut Response, name: &str, value: &str, seconds: i64) {
    let cookie =
        format!("{name}={value}; Path=/; Secure; HttpOnly; SameSite=Lax; Max-Age={seconds}");
    response.headers_mut().append(
        header::SET_COOKIE,
        cookie.parse().expect("generated cookie"),
    );
}
pub fn origin(headers: &HeaderMap, expected: &str) -> Result<()> {
    if headers.get(header::ORIGIN).and_then(|h| h.to_str().ok()) != Some(expected) {
        tracing::warn!(
            origin_present = headers.contains_key(header::ORIGIN),
            "form origin rejected"
        );
        return Err(AppError::Forbidden);
    }
    Ok(())
}
pub fn csrf(session: &Session, value: &str) -> Result<()> {
    if equal(&session.csrf, value) {
        Ok(())
    } else {
        Err(AppError::Forbidden)
    }
}
pub async fn session(s: &AppState, headers: &HeaderMap, kind: &str) -> Result<Session> {
    let name = if kind == "app" {
        "__Host-incident_sid"
    } else {
        "__Host-issuer_sid"
    };
    let token = cookie(headers, name).ok_or(AppError::Unauthorized)?;
    let hash = digest(&token);
    let row=sqlx::query_as("SELECT id_hash,user_id,workspace_id,scopes,csrf FROM sessions WHERE id_hash=? AND kind=? AND expires>? AND last_seen>?").bind(&hash).bind(kind).bind(now()).bind(now()-1800).fetch_optional(&s.db).await?.ok_or(AppError::Unauthorized)?;
    sqlx::query("UPDATE sessions SET last_seen=? WHERE id_hash=?")
        .bind(now())
        .bind(hash)
        .execute(&s.db)
        .await?;
    Ok(row)
}
pub async fn new_session(
    s: &AppState,
    kind: &str,
    user: &str,
    workspace: &str,
    scopes: &str,
) -> Result<String> {
    let token = random();
    let mut tx = s.db.begin_with("BEGIN IMMEDIATE").await?;
    let count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM sessions WHERE user_id=? AND expires>?")
            .bind(user)
            .bind(now())
            .fetch_one(&mut *tx)
            .await?;
    if count >= 100 {
        return Err(AppError::Limited);
    }
    sqlx::query("INSERT INTO sessions VALUES(?,?,?,?,?,?,?,?)")
        .bind(digest(&token))
        .bind(kind)
        .bind(user)
        .bind(workspace)
        .bind(scopes)
        .bind(random())
        .bind(now() + 28800)
        .bind(now())
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(token)
}
pub async fn principal(
    s: &AppState,
    user: &str,
    workspace: &str,
    scopes: &str,
    client: &str,
    credential: &str,
    kind: &str,
) -> Result<Principal> {
    let row:Option<(String,String,String)>=sqlx::query_as("SELECT u.name,w.name,m.role FROM memberships m JOIN users u ON u.id=m.user_id JOIN workspaces w ON w.id=m.workspace_id WHERE m.user_id=? AND m.workspace_id=?").bind(user).bind(workspace).fetch_optional(&s.db).await?;
    let (name, workspace_name, role) = row.ok_or(AppError::Forbidden)?;
    Ok(Principal {
        user_id: user.into(),
        name,
        workspace_id: workspace.into(),
        workspace_name,
        role,
        scopes: scopes.into(),
        client_id: client.into(),
        credential_id: credential.into(),
        kind: kind.into(),
    })
}
pub async fn browser_principal(s: &AppState, h: &HeaderMap) -> Result<(Principal, Session)> {
    let session = session(s, h, "app").await?;
    let p = principal(
        s,
        &session.user_id,
        &session.workspace_id,
        &session.scopes,
        "incident-web",
        &session.id_hash,
        "browser",
    )
    .await?;
    Ok((p, session))
}
pub async fn pending_save<T: Serialize>(
    s: &AppState,
    token: &str,
    kind: &str,
    value: &T,
) -> Result<()> {
    let mut tx = s.db.begin_with("BEGIN IMMEDIATE").await?;
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM pending WHERE expires>?")
        .bind(now())
        .fetch_one(&mut *tx)
        .await?;
    if count >= 1000 {
        return Err(AppError::Limited);
    }
    sqlx::query("INSERT INTO pending VALUES(?,?,?,?)")
        .bind(digest(token))
        .bind(kind)
        .bind(serde_json::to_string(value).map_err(anyhow::Error::from)?)
        .bind(now() + 300)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(())
}
pub async fn pending_read<T: for<'a> Deserialize<'a>>(
    s: &AppState,
    token: &str,
    kind: &str,
) -> Result<T> {
    let body: String =
        sqlx::query_scalar("SELECT body FROM pending WHERE id_hash=? AND kind=? AND expires>?")
            .bind(digest(token))
            .bind(kind)
            .bind(now())
            .fetch_optional(&s.db)
            .await?
            .ok_or(AppError::Invalid(
                "sign-in expired; restart authorization from your app or desktop client",
            ))?;
    serde_json::from_str(&body).map_err(|_| AppError::Invalid("invalid login transaction"))
}
pub fn no_store(mut r: Response) -> Response {
    r.headers_mut()
        .insert(header::CACHE_CONTROL, "no-store".parse().unwrap());
    r.headers_mut()
        // Preserve the Origin of same-origin forms without leaking URLs to callbacks.
        .insert("referrer-policy", "same-origin".parse().unwrap());
    r.headers_mut()
        .insert("x-content-type-options", "nosniff".parse().unwrap());
    r
}
pub fn redirect(url: &str) -> Response {
    no_store(axum::response::Redirect::to(url).into_response())
}
