//! solidcraft-collab: a multi-user add-on for SolidCraft.
//!
//! Accounts are approved by an admin (any email, no outside identity provider). People belong
//! to teams with a role; teams hold personal and joint projects; every project is a folder with
//! full version history. Each file has its own edit lock, so several people work on different
//! parts of one project at once, and a live channel shows who is in which file and what they
//! are working on. The SolidCraft browser build runs unchanged in `?embed=editor` mode inside
//! the editor page, which opens and saves designs through SolidCraft's documented browser API.

pub mod api;
pub mod auth;
pub mod config;
pub mod convert;
pub mod db;
pub mod files;
pub mod live;

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::extract::{DefaultBodyLimit, Request, State};
use axum::http::{header, HeaderValue, Method, StatusCode};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::Router;
use base64::Engine;
use sha2::{Digest, Sha256};
use tower_http::services::{ServeDir, ServeFile};
use tower_http::set_header::SetResponseHeaderLayer;

#[derive(Clone)]
pub struct AppState {
    pub cfg: Arc<config::Config>,
    pub db: Arc<db::Db>,
    pub hub: Arc<live::Hub>,
    pub limits: Arc<auth::Limits>,
    pub dummy_hash: Arc<String>,
    /// At most 2 password hashes at once (memory- and CPU-hard by design).
    pub hash_slots: Arc<tokio::sync::Semaphore>,
    /// One CAD conversion at a time.
    pub convert_slot: Arc<tokio::sync::Semaphore>,
    /// Conversions queued or running.
    pub convert_pending: Arc<std::sync::atomic::AtomicUsize>,
    /// Serialises writes and commits within one project.
    project_locks: Arc<Mutex<HashMap<i64, Arc<tokio::sync::Mutex<()>>>>>,
}

impl AppState {
    pub fn new(cfg: config::Config) -> anyhow::Result<Self> {
        std::fs::create_dir_all(cfg.projects_dir())?;
        let _ = std::fs::remove_dir_all(cfg.data_dir.join("jobs"));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&cfg.data_dir, std::fs::Permissions::from_mode(0o700));
        }
        let db = db::Db::open(&cfg.db_path())?;
        Ok(AppState {
            cfg: Arc::new(cfg),
            db: Arc::new(db),
            hub: Arc::new(live::Hub::default()),
            limits: Arc::new(auth::Limits::default()),
            dummy_hash: Arc::new(auth::hash_password("not-a-real-password-just-for-timing")?),
            hash_slots: Arc::new(tokio::sync::Semaphore::new(2)),
            convert_slot: Arc::new(tokio::sync::Semaphore::new(1)),
            convert_pending: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
            project_locks: Arc::new(Mutex::new(HashMap::new())),
        })
    }

    pub fn project_lock(&self, project: i64) -> Arc<tokio::sync::Mutex<()>> {
        self.project_locks.lock().unwrap_or_else(|p| p.into_inner()).entry(project).or_default().clone()
    }
}

/// The web pages shipped with the add-on (no build step).
pub fn static_dir() -> std::path::PathBuf {
    std::env::var("COLLAB_STATIC_DIR").map(Into::into).unwrap_or_else(|_| std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("static"))
}

/// Largest JSON request body (sign-in, comments, settings). File saves have their own limit.
pub const JSON_BODY_LIMIT: usize = 64 * 1024;
/// CAD conversions waiting at most; further uploads are saved without a design.
pub const CONVERT_QUEUE_MAX: usize = 6;

/// Our own pages: scripts only from files on this site.
const PAGE_CSP: &str = "default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data:; connect-src 'self'; frame-src 'self'; frame-ancestors 'self'; object-src 'none'; base-uri 'none'; form-action 'self'";

/// CSP for the embedded SolidCraft app: its own files, WebAssembly, and only the inline scripts
/// its index.html ships (pinned by hash at start-up).
fn app_csp(app_dir: &std::path::Path) -> String {
    let html = std::fs::read_to_string(app_dir.join("index.html")).unwrap_or_default();
    let mut hashes = Vec::new();
    let mut rest = html.as_str();
    while let Some(start) = rest.find("<script") {
        let after = &rest[start..];
        let Some(open_end) = after.find('>') else { break };
        let tag = &after[..open_end];
        let body_start = open_end + 1;
        let Some(close) = after[body_start..].find("</script>") else { break };
        let body = &after[body_start..body_start + close];
        if !tag.contains("src=") && !body.trim().is_empty() {
            hashes.push(format!("'sha256-{}'", base64::engine::general_purpose::STANDARD.encode(Sha256::digest(body.as_bytes()))));
        }
        rest = &after[body_start + close..];
    }
    format!(
        "default-src 'self'; script-src 'self' 'wasm-unsafe-eval' {}; style-src 'self' 'unsafe-inline'; img-src 'self' data: blob:; font-src 'self' data:; connect-src 'self'; worker-src 'self' blob:; frame-ancestors 'self'; object-src 'none'; base-uri 'self'; form-action 'none'",
        hashes.join(" ")
    )
}

/// State-changing API requests must carry the X-Collab header (cross-site forms and fetches
/// can't add it without a CORS preflight, which is never granted).
async fn csrf_guard(request: Request, next: Next) -> Response {
    let unsafe_method = !matches!(*request.method(), Method::GET | Method::HEAD | Method::OPTIONS);
    if unsafe_method && request.uri().path().starts_with("/api/") && !request.headers().contains_key(auth::CSRF_HEADER) {
        return (StatusCode::FORBIDDEN, axum::Json(serde_json::json!({"error": "Request refused (missing X-Collab header)."}))).into_response();
    }
    next.run(request).await
}

async fn page_headers(State(st): State<AppState>, request: Request, next: Next) -> Response {
    let is_app = request.uri().path().starts_with("/app/") || request.uri().path() == "/app";
    let mut res = next.run(request).await;
    let h = res.headers_mut();
    if !is_app {
        h.insert(header::CONTENT_SECURITY_POLICY, HeaderValue::from_static(PAGE_CSP));
    }
    if st.cfg.secure_cookies {
        h.insert(header::STRICT_TRANSPORT_SECURITY, HeaderValue::from_static("max-age=31536000"));
    }
    res
}

pub fn app(state: AppState) -> Router {
    let st = static_dir();
    let mut r = api::router(state.cfg.max_body).with_state(state.clone()).layer(DefaultBodyLimit::max(JSON_BODY_LIMIT)).layer(tower_http::limit::RequestBodyLimitLayer::new(state.cfg.max_body));
    if let Some(app_dir) = &state.cfg.app_dir {
        // Same origin as the editor page, so the page can reach the embedded app's API.
        let files = ServeDir::new(app_dir).fallback(ServeFile::new(app_dir.join("index.html")));
        let csp = if state.cfg.app_csp { HeaderValue::from_str(&app_csp(app_dir)).ok() } else { None };
        r = match csp {
            Some(v) => r.nest_service("/app", tower::ServiceBuilder::new().layer(SetResponseHeaderLayer::overriding(header::CONTENT_SECURITY_POLICY, v)).service(files)),
            None => r.nest_service("/app", files),
        };
    }
    r.fallback_service(ServeDir::new(&st).fallback(ServeFile::new(st.join("index.html"))))
        .layer(middleware::from_fn(csrf_guard))
        .layer(middleware::from_fn_with_state(state.clone(), page_headers))
        .layer(SetResponseHeaderLayer::if_not_present(header::X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff")))
        .layer(SetResponseHeaderLayer::if_not_present(header::REFERRER_POLICY, HeaderValue::from_static("same-origin")))
        .layer(SetResponseHeaderLayer::if_not_present(header::X_FRAME_OPTIONS, HeaderValue::from_static("SAMEORIGIN")))
}

/// Free locks whose holders went quiet, and tell the projects.
pub fn spawn_lock_reaper(state: AppState) {
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(Duration::from_secs(20));
        loop {
            tick.tick().await;
            let freed = state.db.expire_locks(state.cfg.lock_stale_secs);
            let mut projects: Vec<i64> = freed.iter().map(|(p, _)| *p).collect();
            projects.dedup();
            for p in projects {
                state.hub.publish(p, serde_json::json!({"type": "locks", "locks": state.db.locks(p)}));
            }
        }
    });
}
