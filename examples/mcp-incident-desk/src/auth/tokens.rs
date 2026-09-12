// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    app::AppState,
    domain::Principal,
    error::{AppError, Result},
    security::{digest, equal, now, random},
};
use jsonwebtoken::{Algorithm, Header, Validation};
use serde::{Deserialize, Serialize};
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Claims {
    pub iss: String,
    pub sub: String,
    pub aud: String,
    pub client_id: String,
    pub scope: String,
    pub iat: i64,
    pub exp: i64,
    pub jti: String,
    pub grant_id: String,
    pub workspace_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub nonce: Option<String>,
    pub name: String,
}
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct Grant {
    pub id: String,
    pub user_id: String,
    pub workspace_id: String,
    pub client_id: String,
    pub scopes: String,
    pub resource: String,
    pub expires: i64,
    pub revoked: i64,
}
#[derive(Debug, Serialize, Deserialize)]
pub struct TokenResponse {
    pub access_token: String,
    pub token_type: String,
    pub expires_in: i64,
    pub scope: String,
    pub refresh_token: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id_token: Option<String>,
}
pub fn encode(
    s: &AppState,
    g: &Grant,
    name: &str,
    nonce: Option<String>,
    id: bool,
) -> Result<String> {
    let mut header = Header::new(Algorithm::RS256);
    header.kid = Some(s.keys.kid.clone());
    header.typ = Some(if id { "JWT" } else { "at+jwt" }.into());
    let claims = Claims {
        iss: s.config.issuer.clone(),
        sub: g.user_id.clone(),
        aud: if id {
            g.client_id.clone()
        } else {
            g.resource.clone()
        },
        client_id: g.client_id.clone(),
        scope: g.scopes.clone(),
        iat: now(),
        exp: now() + 300,
        jti: random(),
        grant_id: g.id.clone(),
        workspace_id: g.workspace_id.clone(),
        nonce,
        name: name.into(),
    };
    jsonwebtoken::encode(&header, &claims, &s.keys.encoding)
        .map_err(|_| AppError::Invalid("token signing failed"))
}
pub fn decode(s: &AppState, token: &str, audience: &str, id: bool) -> Result<Claims> {
    let h = jsonwebtoken::decode_header(token).map_err(|_| AppError::Unauthorized)?;
    if h.kid.as_deref() != Some(&s.keys.kid)
        || h.alg != Algorithm::RS256
        || h.typ.as_deref() != Some(if id { "JWT" } else { "at+jwt" })
    {
        return Err(AppError::Unauthorized);
    }
    let mut v = Validation::new(Algorithm::RS256);
    v.set_issuer(&[&s.config.issuer]);
    v.set_audience(&[audience]);
    v.set_required_spec_claims(&["iss", "sub", "aud", "exp", "iat"]);
    v.leeway = 5;
    let claims = jsonwebtoken::decode::<Claims>(token, &s.keys.decoding, &v)
        .map_err(|_| AppError::Unauthorized)?
        .claims;
    if claims.iat > now() + 5 || claims.exp < now() - 5 || claims.sub.is_empty() {
        return Err(AppError::Unauthorized);
    }
    Ok(claims)
}
pub async fn authenticate(s: &AppState, token: &str) -> Result<Principal> {
    let c = decode(s, token, &s.config.resource(), false)?;
    let g: Grant = sqlx::query_as("SELECT * FROM grants WHERE id=? AND revoked=0 AND expires>?")
        .bind(&c.grant_id)
        .bind(now())
        .fetch_optional(&s.db)
        .await?
        .ok_or(AppError::Unauthorized)?;
    if g.user_id != c.sub
        || g.workspace_id != c.workspace_id
        || g.client_id != c.client_id
        || g.scopes != c.scope
        || g.resource != c.aud
    {
        return Err(AppError::Unauthorized);
    }
    super::principal(
        s,
        &g.user_id,
        &g.workspace_id,
        &g.scopes,
        &g.client_id,
        &g.id,
        "mcp",
    )
    .await
}
pub async fn exchange(
    s: &AppState,
    p: std::collections::HashMap<String, String>,
) -> Result<TokenResponse> {
    s.rate_limit("token", 120)?;
    let client = super::required(&p, "client_id")?;
    let secret: Option<Option<String>> =
        sqlx::query_scalar("SELECT secret_hash FROM clients WHERE id=?")
            .bind(client)
            .fetch_optional(&s.db)
            .await?;
    let Some(secret) = secret else {
        return Err(AppError::Unauthorized);
    };
    // A client registered without a secret is public and authenticates by id alone.
    if let Some(h) = secret {
        if !equal(&h, &digest(super::required(&p, "client_secret")?)) {
            return Err(AppError::Unauthorized);
        }
    }
    let mut tx = s.db.begin_with("BEGIN IMMEDIATE").await?;
    let grant_type = super::required(&p, "grant_type")?;
    let nonce;
    let grant_id = match grant_type {
        "authorization_code" => {
            let code = super::required(&p, "code")?;
            let row: Option<(String, String, String, Option<String>, i64, i64)> = sqlx::query_as(
                "SELECT grant_id,redirect,challenge,nonce,expires,consumed FROM codes WHERE hash=?",
            )
            .bind(digest(code))
            .fetch_optional(&mut *tx)
            .await?;
            let (g, redirect, challenge, n, expires, consumed) =
                row.ok_or(AppError::Invalid("invalid_grant"))?;
            let verifier = super::required(&p, "code_verifier")?;
            if expires <= now()
                || consumed != 0
                || redirect != super::required(&p, "redirect_uri")?
                || !(43..=128).contains(&verifier.len())
                || !verifier
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"-._~".contains(&b))
                || !equal(&challenge, &digest(verifier))
            {
                return Err(AppError::Invalid("invalid_grant"));
            }
            nonce = n;
            g
        }
        "refresh_token" => {
            nonce = None;
            let row: Option<(String, i64)> =
                sqlx::query_as("SELECT grant_id,consumed FROM refresh_tokens WHERE hash=?")
                    .bind(digest(super::required(&p, "refresh_token")?))
                    .fetch_optional(&mut *tx)
                    .await?;
            let (g, consumed) = row.ok_or(AppError::Invalid("invalid_grant"))?;
            let owner: String = sqlx::query_scalar("SELECT client_id FROM grants WHERE id=?")
                .bind(&g)
                .fetch_one(&mut *tx)
                .await?;
            if owner != client {
                return Err(AppError::Invalid("invalid_grant"));
            }
            if consumed != 0 {
                sqlx::query("UPDATE grants SET revoked=1 WHERE id=?")
                    .bind(&g)
                    .execute(&mut *tx)
                    .await?;
                tx.commit().await?;
                return Err(AppError::Invalid("invalid_grant"));
            }
            g
        }
        _ => return Err(AppError::Invalid("unsupported_grant_type")),
    };
    let g: Grant = sqlx::query_as("SELECT * FROM grants WHERE id=? AND revoked=0 AND expires>?")
        .bind(&grant_id)
        .bind(now())
        .fetch_optional(&mut *tx)
        .await?
        .ok_or(AppError::Invalid("invalid_grant"))?;
    if g.client_id != client
        || p.get("resource").is_some_and(|r| r != &g.resource)
        || p.get("scope").is_some_and(|r| r != &g.scopes)
    {
        return Err(AppError::Invalid("invalid_grant"));
    }
    let row:Option<(String,String)>=sqlx::query_as("SELECT u.name,m.role FROM memberships m JOIN users u ON u.id=m.user_id WHERE m.user_id=? AND m.workspace_id=?").bind(&g.user_id).bind(&g.workspace_id).fetch_optional(&mut *tx).await?;
    let (name, role) = row.ok_or(AppError::Invalid("invalid_grant"))?;
    if g.scopes.split_whitespace().any(|v| v == "issues:write") && role != "maintainer" {
        return Err(AppError::Invalid("invalid_grant"));
    }
    if grant_type == "authorization_code" {
        sqlx::query("UPDATE codes SET consumed=1 WHERE hash=? AND consumed=0")
            .bind(digest(super::required(&p, "code")?))
            .execute(&mut *tx)
            .await?;
    } else {
        sqlx::query("UPDATE refresh_tokens SET consumed=1 WHERE hash=? AND consumed=0")
            .bind(digest(super::required(&p, "refresh_token")?))
            .execute(&mut *tx)
            .await?;
    }
    let refresh = random();
    sqlx::query("INSERT INTO refresh_tokens VALUES(?,?,0)")
        .bind(digest(&refresh))
        .bind(&g.id)
        .execute(&mut *tx)
        .await?;
    let response = TokenResponse {
        access_token: encode(s, &g, &name, None, false)?,
        token_type: "Bearer".into(),
        expires_in: 300,
        scope: g.scopes.clone(),
        refresh_token: refresh,
        id_token: if grant_type == "authorization_code"
            && g.scopes.split_whitespace().any(|v| v == "openid")
        {
            Some(encode(s, &g, &name, nonce, true)?)
        } else {
            None
        },
    };
    tx.commit().await?;
    Ok(response)
}
