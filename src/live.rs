//! Live presence and events per project, over a WebSocket: who is in the project, which file
//! each person has open, whether they are editing it, and what they are working on in it.
//!
//! Safety: the Origin must be this site (stops sibling sites that share the cookie from
//! listening in); messages are capped at 8 KiB and 5 per second; access is re-checked every
//! 30 seconds, so someone removed, disabled or signed out is disconnected.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path, State};
use axum::http::{header, HeaderMap};
use axum::response::Response;
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::sync::broadcast;

use crate::api::{can_view, ApiError};
use crate::auth::{self, CurrentUser};
use crate::AppState;

#[derive(Debug, Clone, Serialize)]
pub struct Presence {
    pub conn: u64,
    pub user_id: i64,
    pub name: String,
    pub color: String,
    /// The file open, if any.
    pub path: Option<String>,
    /// "editing" or "viewing" (or "browsing" with no file open).
    pub mode: String,
    /// What they are on inside the file, e.g. "Extrude 3". Free text, short.
    pub detail: Option<String>,
    pub since: i64,
}

#[derive(Debug, Deserialize)]
struct ClientMsg {
    #[serde(rename = "type")]
    kind: String,
    path: Option<String>,
    mode: Option<String>,
    detail: Option<String>,
}

pub struct Hub {
    next: AtomicU64,
    channels: Mutex<HashMap<i64, broadcast::Sender<String>>>,
    presence: Mutex<HashMap<i64, HashMap<u64, Presence>>>,
}

impl Default for Hub {
    fn default() -> Self {
        Hub { next: AtomicU64::new(1), channels: Mutex::new(HashMap::new()), presence: Mutex::new(HashMap::new()) }
    }
}

impl Hub {
    fn channel(&self, project: i64) -> broadcast::Sender<String> {
        self.channels.lock().unwrap_or_else(|p| p.into_inner()).entry(project).or_insert_with(|| broadcast::channel(256).0).clone()
    }

    /// Send an event to everyone in the project.
    pub fn publish(&self, project: i64, event: Value) {
        let _ = self.channel(project).send(event.to_string());
    }

    pub fn presence_of(&self, project: i64) -> Vec<Presence> {
        let m = self.presence.lock().unwrap_or_else(|p| p.into_inner());
        let mut v: Vec<Presence> = m.get(&project).map(|x| x.values().cloned().collect()).unwrap_or_default();
        v.sort_by_key(|p| p.since);
        v
    }

    fn set_presence(&self, project: i64, p: Presence) {
        self.presence.lock().unwrap_or_else(|e| e.into_inner()).entry(project).or_default().insert(p.conn, p);
        self.publish(project, json!({"type": "presence", "people": self.presence_of(project)}));
    }

    fn drop_presence(&self, project: i64, conn: u64) {
        if let Some(m) = self.presence.lock().unwrap_or_else(|e| e.into_inner()).get_mut(&project) {
            m.remove(&conn);
        }
        self.publish(project, json!({"type": "presence", "people": self.presence_of(project)}));
    }
}

fn short(s: Option<String>, n: usize) -> Option<String> {
    s.map(|t| t.chars().filter(|c| !auth::is_hidden_char(*c)).take(n).collect::<String>()).filter(|t| !t.trim().is_empty())
}

/// Is the WebSocket's Origin this site?
pub fn origin_ok(st: &AppState, headers: &HeaderMap) -> bool {
    let Some(origin) = headers.get(header::ORIGIN).and_then(|v| v.to_str().ok()) else { return false };
    match &st.cfg.public_origin {
        Some(expected) => origin == expected,
        None => {
            let host = headers.get(header::HOST).and_then(|v| v.to_str().ok()).unwrap_or("");
            origin.split_once("://").map(|(_, h)| h) == Some(host) && !host.is_empty()
        }
    }
}

pub async fn live(ws: WebSocketUpgrade, State(st): State<AppState>, headers: HeaderMap, CurrentUser(user): CurrentUser, Path(project_id): Path<i64>) -> Result<Response, ApiError> {
    if !origin_ok(&st, &headers) {
        return Err(ApiError::Forbidden("Live updates are only available from this site."));
    }
    let project = st.db.project(project_id).ok_or(ApiError::NotFound)?;
    if !can_view(&st, &project, &user) {
        return Err(ApiError::NotFound);
    }
    let token_hash = auth::cookie_token_from(&headers, st.cfg.secure_cookies).map(|t| auth::token_hash(&t)).ok_or(ApiError::Unauthorized)?;
    Ok(ws.max_message_size(8 * 1024).max_frame_size(8 * 1024).on_upgrade(move |socket| session(socket, st, user, project_id, token_hash)))
}

async fn session(socket: WebSocket, st: AppState, user: crate::db::User, project: i64, token_hash: String) {
    let conn = st.hub.next.fetch_add(1, Ordering::Relaxed);
    let mut rx = st.hub.channel(project).subscribe();
    let (mut tx, mut inbound) = socket.split();

    let me = Presence { conn, user_id: user.id, name: user.name.clone(), color: user.color.clone(), path: None, mode: "browsing".into(), detail: None, since: crate::db::now() };
    let hello = json!({"type": "hello", "conn": conn, "people": st.hub.presence_of(project), "locks": st.db.locks(project)});
    if tx.send(Message::Text(hello.to_string().into())).await.is_err() {
        return;
    }
    st.hub.set_presence(project, me.clone());

    // Outbound: project events, plus a periodic access re-check.
    let st_out = st.clone();
    let mut out = tokio::spawn(async move {
        let mut check = tokio::time::interval(Duration::from_secs(30));
        check.tick().await;
        loop {
            tokio::select! {
                msg = rx.recv() => match msg {
                    Ok(text) => if tx.send(Message::Text(text.into())).await.is_err() { break },
                    Err(broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(_) => break,
                },
                _ = check.tick() => {
                    let still = st_out.db.session_user(&token_hash).zip(st_out.db.project(project)).is_some_and(|(u, p)| can_view(&st_out, &p, &u));
                    if !still {
                        let _ = tx.send(Message::Close(None)).await;
                        break;
                    }
                }
            }
        }
    });

    let st2 = st.clone();
    let mut current = me;
    let mut inp = tokio::spawn(async move {
        let mut window_start = Instant::now();
        let mut count = 0u32;
        while let Some(Ok(msg)) = inbound.next().await {
            let Message::Text(t) = msg else {
                if matches!(msg, Message::Close(_)) {
                    break;
                }
                continue;
            };
            if window_start.elapsed() >= Duration::from_secs(1) {
                window_start = Instant::now();
                count = 0;
            }
            count += 1;
            if count > 5 {
                continue; // throttled: drop the update
            }
            let Ok(m) = serde_json::from_str::<ClientMsg>(&t) else { continue };
            if m.kind == "presence" {
                current.path = m.path.as_deref().and_then(crate::files::clean_path);
                let holds_lock = current.path.as_deref().and_then(|p| st2.db.lock_of(project, p)).is_some_and(|l| l.user_id == current.user_id);
                current.mode = match m.mode.as_deref() {
                    Some("editing") if holds_lock => "editing".into(),
                    Some("viewing") if current.path.is_some() => "viewing".into(),
                    _ => "browsing".into(),
                };
                current.detail = short(m.detail, 80);
                st2.hub.set_presence(project, current.clone());
            }
        }
    });

    tokio::select! {
        _ = &mut out => inp.abort(),
        _ = &mut inp => out.abort(),
    }
    st.hub.drop_presence(project, conn);
}
