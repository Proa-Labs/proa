// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use argon2::{
    password_hash::{PasswordHash, SaltString},
    Argon2, PasswordHasher, PasswordVerifier,
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use rand::{rngs::OsRng, RngCore};
use rsa::{
    pkcs8::{DecodePrivateKey, EncodePrivateKey, EncodePublicKey, LineEnding},
    traits::PublicKeyParts,
    RsaPrivateKey, RsaPublicKey,
};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::Write,
    os::unix::fs::{OpenOptionsExt, PermissionsExt},
    path::Path,
};
use subtle::ConstantTimeEq;
pub fn random() -> String {
    let mut b = [0u8; 32];
    OsRng.fill_bytes(&mut b);
    URL_SAFE_NO_PAD.encode(b)
}
pub fn digest(s: &str) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(s.as_bytes()))
}
pub fn equal(a: &str, b: &str) -> bool {
    bool::from(a.as_bytes().ct_eq(b.as_bytes()))
}
pub fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system clock")
        .as_secs() as i64
}
pub fn password_hash(password: &str) -> anyhow::Result<String> {
    Ok(Argon2::default()
        .hash_password(password.as_bytes(), &SaltString::generate(&mut OsRng))
        .map_err(|_| anyhow::anyhow!("password hashing failed"))?
        .to_string())
}
pub fn password_valid(password: &str, hash: &str) -> bool {
    PasswordHash::new(hash).is_ok_and(|h| {
        Argon2::default()
            .verify_password(password.as_bytes(), &h)
            .is_ok()
    })
}
pub fn private_write(path: &Path, bytes: &[u8]) -> anyhow::Result<()> {
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    Ok(())
}
pub struct Keys {
    pub encoding: jsonwebtoken::EncodingKey,
    pub decoding: jsonwebtoken::DecodingKey,
    pub kid: String,
    pub jwks: serde_json::Value,
    pub client_secret: String,
}
impl Keys {
    pub fn load_or_create(dir: &Path) -> anyhow::Result<Self> {
        fs::create_dir_all(dir)?;
        fs::set_permissions(dir, fs::Permissions::from_mode(0o700))?;
        let path = dir.join("signing.pem");
        if !path.exists() {
            let private = RsaPrivateKey::new(&mut OsRng, 3072)?;
            private_write(&path, private.to_pkcs8_pem(LineEnding::LF)?.as_bytes())?;
        }
        let pem = fs::read_to_string(&path)?;
        let private = RsaPrivateKey::from_pkcs8_pem(&pem)?;
        let public = RsaPublicKey::from(&private);
        let public_pem = public.to_public_key_pem(LineEnding::LF)?;
        let kid = digest(&public_pem);
        let secret_path = dir.join("web-client.secret");
        if !secret_path.exists() {
            private_write(&secret_path, random().as_bytes())?;
        }
        Ok(Self {
            encoding: jsonwebtoken::EncodingKey::from_rsa_pem(pem.as_bytes())?,
            decoding: jsonwebtoken::DecodingKey::from_rsa_pem(public_pem.as_bytes())?,
            jwks: serde_json::json!({"keys":[{"kty":"RSA","use":"sig","alg":"RS256","kid":kid,"n":URL_SAFE_NO_PAD.encode(public.n().to_bytes_be()),"e":URL_SAFE_NO_PAD.encode(public.e().to_bytes_be())}]}),
            kid,
            client_secret: fs::read_to_string(secret_path)?,
        })
    }
}
