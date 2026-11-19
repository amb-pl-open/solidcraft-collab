//! Passwords (argon2id), session cookies, rate limits, and the signed-in-person extractor.

use std::collections::HashMap;
use std::sync::Mutex;

use argon2::password_hash::{rand_core::OsRng, PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use argon2::Argon2;
use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use axum::http::HeaderMap;
use rand::RngCore;
use sha2::{Digest, Sha256};

use crate::api::ApiError;
use crate::db::User;
use crate::AppState;

/// Cookie name over HTTPS: the __Host- prefix stops a sibling subdomain from planting one.
pub const COOKIE_SECURE: &str = "__Host-collab_session";
/// Cookie name for plain-HTTP local testing (browsers refuse __Host- without Secure).
pub const COOKIE_PLAIN: &str = "collab_session";
pub const SESSION_TTL: i64 = 14 * 24 * 3600;
/// Every state-changing API request must carry this header. A cross-site page cannot add a
/// custom header without a CORS preflight, which this server never grants.
pub const CSRF_HEADER: &str = "x-collab";

pub fn cookie_name(secure: bool) -> &'static str {
    if secure {
        COOKIE_SECURE
    } else {
        COOKIE_PLAIN
    }
}

fn hash_sync(pw: &str) -> anyhow::Result<String> {
    let salt = SaltString::generate(&mut OsRng);
    Argon2::default().hash_password(pw.as_bytes(), &salt).map(|h| h.to_string()).map_err(|e| anyhow::anyhow!("hash: {e}"))
}

fn verify_sync(pw: &str, hash: &str) -> bool {
    PasswordHash::new(hash).map(|h| Argon2::default().verify_password(pw.as_bytes(), &h).is_ok()).unwrap_or(false)
}

/// Blocking hash for CLI use.
pub fn hash_password(pw: &str) -> anyhow::Result<String> {
    hash_sync(pw)
}

/// Wait at most 10 s for a hashing slot, then answer "busy" rather than queueing forever.
async fn hash_slot(st: &AppState) -> Result<tokio::sync::SemaphorePermit<'_>, ApiError> {
    match tokio::time::timeout(std::time::Duration::from_secs(10), st.hash_slots.acquire()).await {
        Ok(Ok(p)) => Ok(p),
        Ok(Err(_)) => Err(ApiError::Internal("hash slots closed".into())),
        Err(_) => Err(ApiError::Busy),
    }
}

/// Hash on a blocking thread, at most a few at once (argon2 uses ~19 MiB and real CPU each).
pub async fn hash_password_async(st: &AppState, pw: String) -> Result<String, ApiError> {
    let _permit = hash_slot(st).await?;
    tokio::task::spawn_blocking(move || hash_sync(&pw)).await.map_err(|e| ApiError::Internal(e.to_string()))?.map_err(ApiError::from)
}

pub async fn verify_password_async(st: &AppState, pw: String, hash: String) -> Result<bool, ApiError> {
    let _permit = hash_slot(st).await?;
    tokio::task::spawn_blocking(move || verify_sync(&pw, &hash)).await.map_err(|e| ApiError::Internal(e.to_string()))
}

/// A random token for the cookie, and the hash we store (a stolen database holds no live tokens).
pub fn new_token() -> (String, String) {
    let mut b = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut b);
    let t = hex::encode(b);
    let h = token_hash(&t);
    (t, h)
}

pub fn token_hash(t: &str) -> String {
    hex::encode(Sha256::digest(t.as_bytes()))
}

/// A readable one-time password for admin resets: 4 groups of 4 letters/digits.
pub fn temp_password() -> String {
    const A: &[u8] = b"abcdefghjkmnpqrstuvwxyz23456789";
    let mut r = rand::thread_rng();
    (0..4).map(|_| (0..4).map(|_| A[(r.next_u32() as usize) % A.len()] as char).collect::<String>()).collect::<Vec<_>>().join("-")
}

pub fn password_ok(pw: &str) -> Result<(), &'static str> {
    if pw.chars().count() < 10 {
        return Err("Use at least 10 characters for the password.");
    }
    if pw.len() > 256 {
        return Err("That password is too long.");
    }
    Ok(())
}

pub fn email_ok(e: &str) -> bool {
    let e = e.trim();
    let Some((local, domain)) = e.split_once('@') else { return false };
    !local.is_empty()
        && domain.contains('.')
        && !domain.starts_with('.')
        && !domain.ends_with('.')
        && e.len() <= 254
        && !e.chars().any(|c| c.is_whitespace() || is_hidden_char(c))
}

/// Control and invisible formatting characters (bidi overrides, zero-width, line breaks).
pub fn is_hidden_char(c: char) -> bool {
    c.is_control()
        || matches!(c as u32, 0x200B..=0x200F | 0x202A..=0x202E | 0x2060..=0x2064 | 0x2066..=0x206F | 0xFEFF | 0x00AD | 0x061C | 0x180E)
}

/// A display name (person, team, project): trimmed, 1..=max characters, nothing hidden.
pub fn name_ok(n: &str, max: usize) -> bool {
    let n = n.trim();
    !n.is_empty() && n.chars().count() <= max && !n.chars().any(is_hidden_char)
}

pub fn session_cookie(token: &str, secure: bool, max_age: i64) -> String {
    format!("{}={token}; Path=/; HttpOnly; SameSite=Lax; Max-Age={max_age}{}", cookie_name(secure), if secure { "; Secure" } else { "" })
}

pub fn cookie_token_from(headers: &HeaderMap, secure: bool) -> Option<String> {
    let name = cookie_name(secure);
    headers.get_all(axum::http::header::COOKIE).iter().filter_map(|v| v.to_str().ok()).flat_map(|v| v.split(';')).find_map(|kv| {
        let (k, v) = kv.trim().split_once('=')?;
        (k == name && !v.is_empty()).then(|| v.to_string())
    })
}

/// Bucket an address for rate limits: IPv4 as is, IPv6 by its /64 (one household or host).
fn bucket(ip: std::net::IpAddr) -> String {
    match ip {
        std::net::IpAddr::V4(v4) => v4.to_string(),
        std::net::IpAddr::V6(v6) => match v6.to_ipv4_mapped() {
            Some(v4) => v4.to_string(),
            None => {
                let s = v6.segments();
                format!("{:x}:{:x}:{:x}:{:x}::/64", s[0], s[1], s[2], s[3])
            }
        },
    }
}

/// The client address for rate limits: CF-Connecting-IP when trusted (only behind Cloudflare),
/// else the TCP peer address, else one shared bucket.
pub fn client_key(parts_headers: &HeaderMap, extensions: &axum::http::Extensions, trust_cf: bool) -> String {
    if trust_cf {
        if let Some(ip) = parts_headers.get("cf-connecting-ip").and_then(|v| v.to_str().ok()).and_then(|v| v.trim().parse().ok()) {
            return bucket(ip);
        }
    }
    if let Some(axum::extract::ConnectInfo(addr)) = extensions.get::<axum::extract::ConnectInfo<std::net::SocketAddr>>() {
        return bucket(addr.ip());
    }
    "direct".into()
}

/// Extractor for the rate-limit key of the request.
pub struct ClientKey(pub String);

impl FromRequestParts<AppState> for ClientKey {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, Self::Rejection> {
        Ok(ClientKey(client_key(&parts.headers, &parts.extensions, state.cfg.trust_cf_ip)))
    }
}

/// Sliding-window counters per key, pruned as they age out.
pub struct RateLimit {
    window: i64,
    max: usize,
    hits: Mutex<HashMap<String, Vec<i64>>>,
}

impl RateLimit {
    pub fn new(window: i64, max: usize) -> Self {
        RateLimit { window, max, hits: Mutex::new(HashMap::new()) }
    }

    pub fn blocked(&self, key: &str) -> bool {
        let now = crate::db::now();
        let mut m = self.hits.lock().unwrap_or_else(|p| p.into_inner());
        if m.len() > 10_000 {
            m.retain(|_, v| v.last().is_some_and(|t| now - t < self.window));
        }
        match m.get_mut(&key.to_lowercase()) {
            Some(v) => {
                v.retain(|t| now - t < self.window);
                v.len() >= self.max
            }
            None => false,
        }
    }

    /// Count this attempt and say whether it is over the limit (counting first, so a burst of
    /// parallel requests can't all pass the check before any is recorded).
    pub fn take(&self, key: &str) -> bool {
        let now = crate::db::now();
        let mut m = self.hits.lock().unwrap_or_else(|p| p.into_inner());
        if m.len() > 10_000 {
            m.retain(|_, v| v.last().is_some_and(|t| now - t < self.window));
        }
        let v = m.entry(key.to_lowercase()).or_default();
        v.retain(|t| now - t < self.window);
        if v.len() >= self.max {
            return true;
        }
        v.push(now);
        false
    }

    pub fn hit(&self, key: &str) {
        let now = crate::db::now();
        let mut m = self.hits.lock().unwrap_or_else(|p| p.into_inner());
        let v = m.entry(key.to_lowercase()).or_default();
        v.retain(|t| now - t < self.window);
        v.push(now);
    }

    pub fn clear(&self, key: &str) {
        self.hits.lock().unwrap_or_else(|p| p.into_inner()).remove(&key.to_lowercase());
    }
}

pub struct Limits {
    /// Failed sign-ins per email: 8 in 10 minutes.
    pub login_email: RateLimit,
    /// Failed sign-ins per address: 30 in 10 minutes (stops spraying many emails).
    pub login_ip: RateLimit,
    /// Sign-up requests per address: 5 per hour.
    pub signup_ip: RateLimit,
}

impl Default for Limits {
    fn default() -> Self {
        Limits { login_email: RateLimit::new(600, 8), login_ip: RateLimit::new(600, 30), signup_ip: RateLimit::new(3600, 5) }
    }
}

/// Paths a person with a one-time password may still use.
const CHANGE_PW_ALLOWED: [&str; 4] = ["/api/me", "/api/auth/password", "/api/auth/logout", "/api/config"];

/// The signed-in, approved person making the request. Someone who must change a one-time
/// password can only reach the password-change routes until they do.
pub struct CurrentUser(pub User);

impl FromRequestParts<AppState> for CurrentUser {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, Self::Rejection> {
        let token = cookie_token_from(&parts.headers, state.cfg.secure_cookies).ok_or(ApiError::Unauthorized)?;
        let user = state.db.session_user(&token_hash(&token)).ok_or(ApiError::Unauthorized)?;
        if user.must_change_pw && !CHANGE_PW_ALLOWED.contains(&parts.uri.path()) {
            return Err(ApiError::Forbidden("Choose your own password first (Account page)."));
        }
        Ok(CurrentUser(user))
    }
}

/// A signed-in admin.
pub struct Admin(pub User);

impl FromRequestParts<AppState> for Admin {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, Self::Rejection> {
        let CurrentUser(u) = CurrentUser::from_request_parts(parts, state).await?;
        if u.is_admin {
            Ok(Admin(u))
        } else {
            Err(ApiError::Forbidden("Only an admin can do that."))
        }
    }
}
