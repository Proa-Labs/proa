// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use argon2::{
    password_hash::{phc::PasswordHash, PasswordHasher, PasswordVerifier},
    Argon2,
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use rand::{rngs::SysRng, TryRng};
use rsa::{
    pkcs8::{DecodePrivateKey, EncodePrivateKey, EncodePublicKey, LineEnding},
    traits::PublicKeyParts,
    PrivateKey, PublicKey,
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
    SysRng
        .try_fill_bytes(&mut b)
        .expect("operating system randomness");
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
        .hash_password(password.as_bytes())
        .map_err(|_| anyhow::anyhow!("password hashing failed"))?
        .to_string())
}
pub fn password_valid(password: &str, hash: &str) -> bool {
    PasswordHash::try_from(hash).is_ok_and(|h| {
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
            let private = PrivateKey::new(&mut SysRng, 3072)?;
            private_write(&path, private.to_pkcs8_pem(LineEnding::LF)?.as_bytes())?;
        }
        let pem = fs::read_to_string(&path)?;
        let private = PrivateKey::from_pkcs8_pem(&pem)?;
        let public = PublicKey::from(&private);
        let public_pem = public.to_public_key_pem(LineEnding::LF)?;
        let kid = digest(&public_pem);
        let secret_path = dir.join("web-client.secret");
        if !secret_path.exists() {
            private_write(&secret_path, random().as_bytes())?;
        }
        Ok(Self {
            encoding: jsonwebtoken::EncodingKey::from_rsa_pem(pem.as_bytes())?,
            decoding: jsonwebtoken::DecodingKey::from_rsa_pem(public_pem.as_bytes())?,
            jwks: serde_json::json!({"keys":[{"kty":"RSA","use":"sig","alg":"RS256","kid":kid,"n":URL_SAFE_NO_PAD.encode(public.n_bytes()),"e":URL_SAFE_NO_PAD.encode(public.e_bytes())}]}),
            kid,
            client_secret: fs::read_to_string(secret_path)?,
        })
    }
}
