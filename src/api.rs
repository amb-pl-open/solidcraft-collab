//! HTTP API. Every project route checks that the person may see the project (team member
//! and project member, or admin); anything else answers 404, so other teams' projects can't
//! even be confirmed to exist.

use axum::body::Bytes;
use axum::extract::{Path, Query, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::auth::{self, Admin, ClientKey, CurrentUser};
use crate::db::{Project, User};
use crate::files;
use crate::AppState;

#[derive(Debug)]
pub enum ApiError {
    Unauthorized,
    Forbidden(&'static str),
    NotFound,
    Bad(String),
    Conflict(Value),
    TooMany,
    Busy,
    Internal(String),
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (code, body) = match self {
            ApiError::Unauthorized => (StatusCode::UNAUTHORIZED, json!({"error": "Please sign in."})),
            ApiError::Forbidden(m) => (StatusCode::FORBIDDEN, json!({"error": m})),
            ApiError::NotFound => (StatusCode::NOT_FOUND, json!({"error": "Not found."})),
            ApiError::Bad(m) => (StatusCode::BAD_REQUEST, json!({"error": m})),
            ApiError::Conflict(v) => (StatusCode::CONFLICT, v),
            ApiError::TooMany => (StatusCode::TOO_MANY_REQUESTS, json!({"error": "Too many attempts. Wait 10 minutes and try again."})),
            ApiError::Busy => (StatusCode::SERVICE_UNAVAILABLE, json!({"error": "The server is busy. Try again in a moment."})),
            ApiError::Internal(m) => {
                tracing::error!("internal error: {m}");
                (StatusCode::INTERNAL_SERVER_ERROR, json!({"error": "Something went wrong on the server. Try again; if it keeps happening, tell your admin."}))
            }
        };
        (code, Json(body)).into_response()
    }
}

impl From<rusqlite::Error> for ApiError {
    fn from(e: rusqlite::Error) -> Self {
        match e {
            rusqlite::Error::SqliteFailure(f, _) if f.code == rusqlite::ErrorCode::ConstraintViolation => ApiError::Bad("That name or email is already in use.".into()),
            e => ApiError::Internal(e.to_string()),
        }
    }
}

impl From<anyhow::Error> for ApiError {
    fn from(e: anyhow::Error) -> Self {
        ApiError::Internal(e.to_string())
    }
}

type R<T> = Result<T, ApiError>;

// ---- permissions -------------------------------------------------------------------------

pub fn can_view(st: &AppState, p: &Project, u: &User) -> bool {
    u.is_admin || (st.db.role_in_team(p.team_id, u.id).is_some() && st.db.is_project_member(p.id, u.id))
}

fn can_edit(st: &AppState, p: &Project, u: &User) -> bool {
    u.is_admin || (can_view(st, p, u) && st.db.role_in_team(p.team_id, u.id).is_some_and(|r| r != "viewer"))
}

fn can_manage(st: &AppState, p: &Project, u: &User) -> bool {
    u.is_admin || p.owner_id == u.id || st.db.role_in_team(p.team_id, u.id).is_some_and(|r| r == "owner")
}

fn project_for(st: &AppState, id: i64, u: &User) -> R<Project> {
    let p = st.db.project(id).ok_or(ApiError::NotFound)?;
    if can_view(st, &p, u) {
        Ok(p)
    } else {
        Err(ApiError::NotFound)
    }
}

fn path_of(raw: &str) -> R<String> {
    files::clean_path(raw).ok_or_else(|| ApiError::Bad("That file name isn't allowed. Use letters, digits, spaces and - _ . only.".into()))
}

/// API routes. JSON routes accept at most 64 KiB; file saves and uploads up to `max_body`.
pub fn router(max_body: usize) -> Router<AppState> {
    Router::new()
        .route("/api/config", get(site_config))
        .route("/api/auth/signup", post(signup))
        .route("/api/auth/login", post(login))
        .route("/api/auth/logout", post(logout))
        .route("/api/auth/password", post(change_password))
        .route("/api/me", get(me))
        .route("/api/admin/users", get(admin_users))
        .route("/api/admin/users/{id}/approve", post(admin_approve))
        .route("/api/admin/users/{id}/reject", post(admin_reject))
        .route("/api/admin/users/{id}/disable", post(admin_disable))
        .route("/api/admin/users/{id}/enable", post(admin_enable))
        .route("/api/admin/users/{id}/reset-password", post(admin_reset))
        .route("/api/admin/users/{id}/admin", post(admin_set_admin))
        .route("/api/admin/teams", get(admin_teams).post(admin_create_team))
        .route("/api/teams", get(my_teams))
        .route("/api/teams/{id}/members", get(team_members).post(team_set_member))
        .route("/api/teams/{id}/members/{uid}", delete(team_remove_member))
        .route("/api/teams/{id}/projects", get(team_projects).post(create_project))
        .route("/api/projects/{id}", get(project_info))
        .route("/api/projects/{id}/members", post(project_add_member))
        .route("/api/projects/{id}/members/{uid}", delete(project_remove_member))
        .route("/api/projects/{id}/files/{*path}", get(read_file).put(write_file).layer(axum::extract::DefaultBodyLimit::max(max_body)))
        .route("/api/projects/{id}/history", get(project_history))
        .route("/api/projects/{id}/restore", post(restore))
        .route("/api/projects/{id}/lock", post(lock).delete(unlock))
        .route("/api/projects/{id}/lock/force", post(force_unlock))
        .route("/api/projects/{id}/comments", get(list_comments).post(add_comment))
        .route("/api/projects/{id}/live", get(crate::live::live))
}

// ---- auth --------------------------------------------------------------------------------

async fn site_config(State(st): State<AppState>) -> Json<Value> {
    Json(json!({"site_name": st.cfg.site_name, "editor": st.cfg.app_dir.is_some(), "cad_import": st.cfg.cli.is_some()}))
}

#[derive(Deserialize)]
struct SignupReq {
    email: String,
    name: String,
    password: String,
}

async fn signup(State(st): State<AppState>, ClientKey(ip): ClientKey, Json(r): Json<SignupReq>) -> R<Json<Value>> {
    if st.limits.signup_ip.take(&ip) {
        return Err(ApiError::TooMany);
    }
    if !auth::email_ok(&r.email) {
        return Err(ApiError::Bad("Enter a valid email address.".into()));
    }
    if !auth::name_ok(&r.name, 60) {
        return Err(ApiError::Bad("Enter your name (up to 60 characters, letters and spaces).".into()));
    }
    auth::password_ok(&r.password).map_err(|m| ApiError::Bad(m.into()))?;
    let pending = Json(json!({"status": "pending", "message": "Thanks. An admin needs to approve your account before you can sign in."}));
    // Same answer, and the same work (a hash), whether or not the email already has an account,
    // so sign-up can't be used to find out who has one.
    let h = auth::hash_password_async(&st, r.password.clone()).await?;
    if st.db.user_by_email_with_hash(&r.email).is_some() {
        return Ok(pending);
    }
    match st.db.create_user(&r.email, r.name.trim(), &h, "pending", false) {
        Ok(id) => st.db.audit(Some(id), "signup", &r.email),
        Err(rusqlite::Error::SqliteFailure(f, _)) if f.code == rusqlite::ErrorCode::ConstraintViolation => {}
        Err(e) => return Err(e.into()),
    }
    Ok(pending)
}

#[derive(Deserialize)]
struct LoginReq {
    email: String,
    password: String,
}

async fn login(State(st): State<AppState>, ClientKey(ip): ClientKey, Json(r): Json<LoginReq>) -> R<Response> {
    // Check sizes before anything is stored or hashed.
    if r.email.len() > 254 || r.password.len() > 256 {
        return Err(ApiError::Bad("That email and password don't match an account.".into()));
    }
    let key = r.email.trim().to_lowercase();
    // Per address only failures count (an office behind one IP shares it); per email every
    // attempt counts before the check, and a success clears it.
    if st.limits.login_ip.blocked(&ip) || st.limits.login_email.take(&key) {
        return Err(ApiError::TooMany);
    }
    let found = st.db.user_by_email_with_hash(&r.email);
    // Verify against a dummy hash when the email is unknown, so timing doesn't reveal accounts.
    let hash = found.as_ref().map(|(_, h)| h.clone()).unwrap_or_else(|| st.dummy_hash.as_str().to_string());
    let ok = auth::verify_password_async(&st, r.password.clone(), hash).await? && found.is_some();
    let Some((user, _)) = found.filter(|_| ok) else {
        st.limits.login_ip.hit(&ip);
        return Err(ApiError::Bad("That email and password don't match an account.".into()));
    };
    match user.status.as_str() {
        "pending" => return Err(ApiError::Forbidden("Your account is waiting for an admin to approve it.")),
        "disabled" => return Err(ApiError::Forbidden("Your account has been switched off. Ask your admin.")),
        _ => {}
    }
    st.limits.login_email.clear(&key);
    let (token, hash) = auth::new_token();
    st.db.create_session(&hash, user.id, auth::SESSION_TTL)?;
    st.db.audit(Some(user.id), "login", "");
    let mut headers = HeaderMap::new();
    headers.insert(header::SET_COOKIE, auth::session_cookie(&token, st.cfg.secure_cookies, auth::SESSION_TTL).parse().map_err(|_| ApiError::Internal("cookie".into()))?);
    Ok((headers, Json(json!({"user": user}))).into_response())
}

async fn logout(State(st): State<AppState>, headers: HeaderMap) -> R<Response> {
    if let Some(t) = auth::cookie_token_from(&headers, st.cfg.secure_cookies) {
        st.db.delete_session(&auth::token_hash(&t));
    }
    let mut h = HeaderMap::new();
    h.insert(header::SET_COOKIE, auth::session_cookie("", st.cfg.secure_cookies, 0).parse().map_err(|_| ApiError::Internal("cookie".into()))?);
    Ok((h, Json(json!({"ok": true}))).into_response())
}

#[derive(Deserialize)]
struct PwReq {
    current: String,
    new: String,
}

async fn change_password(State(st): State<AppState>, CurrentUser(u): CurrentUser, Json(r): Json<PwReq>) -> R<Json<Value>> {
    let (_, h) = st.db.user_by_email_with_hash(&u.email).ok_or(ApiError::Unauthorized)?;
    if r.current.len() > 256 || !auth::verify_password_async(&st, r.current.clone(), h).await? {
        return Err(ApiError::Bad("Your current password is not right.".into()));
    }
    auth::password_ok(&r.new).map_err(|m| ApiError::Bad(m.into()))?;
    if r.new == r.current {
        return Err(ApiError::Bad("Choose a password different from the current one.".into()));
    }
    let nh = auth::hash_password_async(&st, r.new.clone()).await?;
    st.db.set_password(u.id, &nh, false)?;
    st.db.audit(Some(u.id), "password-changed", "");
    Ok(Json(json!({"ok": true, "message": "Password changed. Sign in again with the new one."})))
}

async fn me(State(st): State<AppState>, CurrentUser(u): CurrentUser) -> Json<Value> {
    Json(json!({"user": u, "teams": st.db.teams_of(u.id)}))
}

// ---- admin -------------------------------------------------------------------------------

#[derive(Deserialize)]
struct StatusQ {
    status: Option<String>,
}

async fn admin_users(State(st): State<AppState>, Admin(_): Admin, Query(q): Query<StatusQ>) -> Json<Value> {
    let users = st.db.users(q.status.as_deref());
    let rows: Vec<Value> = users.into_iter().map(|u| json!({"user": u, "teams": st.db.teams_of(u.id)})).collect();
    Json(json!({"users": rows}))
}

#[derive(Deserialize)]
struct ApproveReq {
    team_id: i64,
    role: String,
}

fn role_ok(r: &str) -> R<()> {
    matches!(r, "owner" | "editor" | "viewer").then_some(()).ok_or_else(|| ApiError::Bad("Role must be owner, editor or viewer.".into()))
}

async fn admin_approve(State(st): State<AppState>, Admin(a): Admin, Path(id): Path<i64>, Json(r): Json<ApproveReq>) -> R<Json<Value>> {
    role_ok(&r.role)?;
    let u = st.db.user(id).ok_or(ApiError::NotFound)?;
    if st.db.all_teams().iter().all(|t| t.id != r.team_id) {
        return Err(ApiError::Bad("Pick a team for this person.".into()));
    }
    st.db.set_user_status(id, "active")?;
    st.db.set_membership(r.team_id, id, &r.role)?;
    st.db.audit(Some(a.id), "approve", &format!("{} team {} as {}", u.email, r.team_id, r.role));
    Ok(Json(json!({"ok": true})))
}

async fn admin_reject(State(st): State<AppState>, Admin(a): Admin, Path(id): Path<i64>) -> R<Json<Value>> {
    let u = st.db.user(id).ok_or(ApiError::NotFound)?;
    if st.db.delete_user(id)? == 0 {
        return Err(ApiError::Bad("Only accounts still waiting for approval can be rejected. Switch this one off instead.".into()));
    }
    st.db.audit(Some(a.id), "reject", &u.email);
    Ok(Json(json!({"ok": true})))
}

async fn admin_disable(State(st): State<AppState>, Admin(a): Admin, Path(id): Path<i64>) -> R<Json<Value>> {
    if id == a.id {
        return Err(ApiError::Bad("You can't switch off your own account.".into()));
    }
    let u = st.db.user(id).ok_or(ApiError::NotFound)?;
    st.db.set_user_status(id, "disabled")?;
    st.db.audit(Some(a.id), "disable", &u.email);
    Ok(Json(json!({"ok": true})))
}

async fn admin_enable(State(st): State<AppState>, Admin(a): Admin, Path(id): Path<i64>) -> R<Json<Value>> {
    let u = st.db.user(id).ok_or(ApiError::NotFound)?;
    st.db.set_user_status(id, "active")?;
    st.db.audit(Some(a.id), "enable", &u.email);
    Ok(Json(json!({"ok": true})))
}

async fn admin_reset(State(st): State<AppState>, Admin(a): Admin, Path(id): Path<i64>) -> R<Json<Value>> {
    let u = st.db.user(id).ok_or(ApiError::NotFound)?;
    let pw = auth::temp_password();
    let h = auth::hash_password_async(&st, pw.clone()).await?;
    st.db.set_password(id, &h, true)?;
    st.db.audit(Some(a.id), "reset-password", &u.email);
    Ok(Json(json!({"temporary_password": pw, "message": "Give this one-time password to the person; they should change it after signing in."})))
}

#[derive(Deserialize)]
struct AdminFlag {
    admin: bool,
}

async fn admin_set_admin(State(st): State<AppState>, Admin(a): Admin, Path(id): Path<i64>, Json(r): Json<AdminFlag>) -> R<Json<Value>> {
    if !r.admin && id == a.id && st.db.admin_count() <= 1 {
        return Err(ApiError::Bad("You are the last admin; make someone else an admin first.".into()));
    }
    st.db.set_admin(id, r.admin)?;
    st.db.audit(Some(a.id), "set-admin", &format!("{id} {}", r.admin));
    Ok(Json(json!({"ok": true})))
}

async fn admin_teams(State(st): State<AppState>, Admin(_): Admin) -> Json<Value> {
    Json(json!({"teams": st.db.all_teams()}))
}

#[derive(Deserialize)]
struct NameReq {
    name: String,
}

async fn admin_create_team(State(st): State<AppState>, Admin(a): Admin, Json(r): Json<NameReq>) -> R<Json<Value>> {
    let name = r.name.trim();
    if !auth::name_ok(name, 60) {
        return Err(ApiError::Bad("Give the team a name (up to 60 characters).".into()));
    }
    let id = st.db.create_team(name)?;
    st.db.audit(Some(a.id), "create-team", name);
    Ok(Json(json!({"id": id})))
}

// ---- teams -------------------------------------------------------------------------------

async fn my_teams(State(st): State<AppState>, CurrentUser(u): CurrentUser) -> Json<Value> {
    let teams = if u.is_admin {
        let mine = st.db.teams_of(u.id);
        st.db.all_teams().into_iter().map(|mut t| { t.role = mine.iter().find(|m| m.id == t.id).and_then(|m| m.role.clone()).or(Some("admin".into())); t }).collect()
    } else {
        st.db.teams_of(u.id)
    };
    Json(json!({"teams": teams}))
}

fn team_role(st: &AppState, team: i64, u: &User) -> R<String> {
    if u.is_admin {
        return Ok(st.db.role_in_team(team, u.id).unwrap_or_else(|| "owner".into()));
    }
    st.db.role_in_team(team, u.id).ok_or(ApiError::NotFound)
}

async fn team_members(State(st): State<AppState>, CurrentUser(u): CurrentUser, Path(team): Path<i64>) -> R<Json<Value>> {
    let my_role = team_role(&st, team, &u)?;
    let show_email = u.is_admin || my_role == "owner";
    let rows: Vec<Value> = st
        .db
        .team_members(team)
        .into_iter()
        .map(|(m, role)| json!({"id": m.id, "name": m.name, "email": if show_email { Some(m.email) } else { None }, "color": m.color, "role": role, "status": m.status}))
        .collect();
    Ok(Json(json!({"members": rows})))
}

#[derive(Deserialize)]
struct MemberReq {
    user_id: i64,
    role: String,
}

async fn team_set_member(State(st): State<AppState>, CurrentUser(u): CurrentUser, Path(team): Path<i64>, Json(r): Json<MemberReq>) -> R<Json<Value>> {
    if team_role(&st, team, &u)? != "owner" {
        return Err(ApiError::Forbidden("Only a team owner or admin can change members."));
    }
    role_ok(&r.role)?;
    let m = st.db.user(r.user_id).ok_or(ApiError::NotFound)?;
    if m.status != "active" {
        return Err(ApiError::Bad("That person's account isn't approved yet.".into()));
    }
    // Team owners may only bring in people they already share a team with; admins anyone.
    if !u.is_admin {
        let mine: Vec<i64> = st.db.teams_of(u.id).into_iter().map(|t| t.id).collect();
        let shares = st.db.teams_of(r.user_id).iter().any(|t| mine.contains(&t.id));
        if !shares {
            return Err(ApiError::NotFound);
        }
    }
    st.db.set_membership(team, r.user_id, &r.role)?;
    st.db.audit(Some(u.id), "set-member", &format!("team {team} user {} {}", r.user_id, r.role));
    Ok(Json(json!({"ok": true})))
}

async fn team_remove_member(State(st): State<AppState>, CurrentUser(u): CurrentUser, Path((team, uid)): Path<(i64, i64)>) -> R<Json<Value>> {
    if team_role(&st, team, &u)? != "owner" {
        return Err(ApiError::Forbidden("Only a team owner or admin can change members."));
    }
    st.db.remove_membership(team, uid)?;
    st.db.audit(Some(u.id), "remove-member", &format!("team {team} user {uid}"));
    Ok(Json(json!({"ok": true})))
}

// ---- projects ----------------------------------------------------------------------------

async fn team_projects(State(st): State<AppState>, CurrentUser(u): CurrentUser, Path(team): Path<i64>) -> R<Json<Value>> {
    team_role(&st, team, &u)?;
    let list = if u.is_admin { st.db.visible_projects(team, u.id).into_iter().chain(all_team_projects(&st, team, u.id)).collect::<Vec<_>>() } else { st.db.visible_projects(team, u.id) };
    let mut seen = std::collections::HashSet::new();
    let rows: Vec<Value> = list
        .into_iter()
        .filter(|p| seen.insert(p.id))
        .map(|p| {
            let owner = st.db.user(p.owner_id).map(|o| o.name).unwrap_or_default();
            let locks = st.db.locks(p.id);
            let people = st.hub.presence_of(p.id);
            json!({"project": p, "owner_name": owner, "locks": locks, "people": people})
        })
        .collect();
    Ok(Json(json!({"projects": rows})))
}

/// Admins see every project of a team (for support), not only ones they belong to.
fn all_team_projects(st: &AppState, team: i64, _admin: i64) -> Vec<Project> {
    st.db.team_members(team).iter().flat_map(|(m, _)| st.db.visible_projects(team, m.id)).collect()
}

#[derive(Deserialize)]
struct NewProject {
    name: String,
    kind: String,
}

async fn create_project(State(st): State<AppState>, CurrentUser(u): CurrentUser, Path(team): Path<i64>, Json(r): Json<NewProject>) -> R<Json<Value>> {
    let role = team_role(&st, team, &u)?;
    if role == "viewer" {
        return Err(ApiError::Forbidden("Viewers can't create projects."));
    }
    let name = r.name.trim();
    if !auth::name_ok(name, 80) {
        return Err(ApiError::Bad("Give the project a name (up to 80 characters).".into()));
    }
    if !matches!(r.kind.as_str(), "personal" | "joint") {
        return Err(ApiError::Bad("A project is personal or joint.".into()));
    }
    let id = st.db.create_project(team, name, &r.kind, u.id)?;
    files::init_project(&files::project_dir(&st.cfg.projects_dir(), id)).await?;
    st.db.audit(Some(u.id), "create-project", &format!("{id} {name} {}", r.kind));
    Ok(Json(json!({"id": id})))
}

async fn project_info(State(st): State<AppState>, CurrentUser(u): CurrentUser, Path(id): Path<i64>) -> R<Json<Value>> {
    let p = project_for(&st, id, &u)?;
    let dir = files::project_dir(&st.cfg.projects_dir(), id);
    let members: Vec<Value> = st.db.project_members(id).into_iter().map(|m| json!({"id": m.id, "name": m.name, "color": m.color})).collect();
    Ok(Json(json!({
        "project": p,
        "team": st.db.all_teams().into_iter().find(|t| t.id == p.team_id),
        "files": files::list_files(&dir),
        "locks": st.db.locks(id),
        "people": st.hub.presence_of(id),
        "members": members,
        "version": files::current_version(&dir).await,
        "can_edit": can_edit(&st, &p, &u),
        "can_manage": can_manage(&st, &p, &u),
        "me": {"id": u.id, "name": u.name, "color": u.color},
    })))
}

#[derive(Deserialize)]
struct UserIdReq {
    user_id: i64,
}

async fn project_add_member(State(st): State<AppState>, CurrentUser(u): CurrentUser, Path(id): Path<i64>, Json(r): Json<UserIdReq>) -> R<Json<Value>> {
    let p = project_for(&st, id, &u)?;
    if !can_manage(&st, &p, &u) {
        return Err(ApiError::Forbidden("Only the project owner, a team owner or an admin can share this project."));
    }
    if st.db.role_in_team(p.team_id, r.user_id).is_none() {
        return Err(ApiError::Bad("That person isn't in this project's team.".into()));
    }
    st.db.add_project_member(id, r.user_id)?;
    st.db.audit(Some(u.id), "project-add-member", &format!("{id} {}", r.user_id));
    Ok(Json(json!({"ok": true})))
}

async fn project_remove_member(State(st): State<AppState>, CurrentUser(u): CurrentUser, Path((id, uid)): Path<(i64, i64)>) -> R<Json<Value>> {
    let p = project_for(&st, id, &u)?;
    if !can_manage(&st, &p, &u) {
        return Err(ApiError::Forbidden("Only the project owner, a team owner or an admin can change who sees this project."));
    }
    if uid == p.owner_id {
        return Err(ApiError::Bad("The project's owner always keeps access.".into()));
    }
    st.db.remove_project_member(id, uid)?;
    st.db.audit(Some(u.id), "project-remove-member", &format!("{id} {uid}"));
    Ok(Json(json!({"ok": true})))
}

// ---- files -------------------------------------------------------------------------------

#[derive(Deserialize)]
struct VersionQ {
    version: Option<String>,
}

async fn read_file(State(st): State<AppState>, CurrentUser(u): CurrentUser, Path((id, raw)): Path<(i64, String)>, Query(q): Query<VersionQ>) -> R<Response> {
    project_for(&st, id, &u)?;
    let path = path_of(&raw)?;
    let dir = files::project_dir(&st.cfg.projects_dir(), id);
    let bytes = match q.version {
        Some(v) => files::read_at(&dir, &path, &v).await.map_err(|_| ApiError::NotFound)?,
        None => tokio::fs::read(dir.join(&path)).await.map_err(|_| ApiError::NotFound)?,
    };
    let ctype = if path.ends_with(".json") { "application/json" } else { "application/octet-stream" };
    Ok(([(header::CONTENT_TYPE, ctype), (header::CACHE_CONTROL, "no-store")], bytes).into_response())
}

#[derive(Deserialize)]
struct WriteQ {
    message: Option<String>,
}

/// Save a file as a new version. Saving an existing file needs its edit lock; a new file
/// can be added by any editor. Uploading a CAD file also makes a design from it.
async fn write_file(State(st): State<AppState>, CurrentUser(u): CurrentUser, Path((id, raw)): Path<(i64, String)>, Query(q): Query<WriteQ>, body: Bytes) -> R<Json<Value>> {
    let p = project_for(&st, id, &u)?;
    if !can_edit(&st, &p, &u) {
        return Err(ApiError::Forbidden("You can view this project but not change it."));
    }
    let path = path_of(&raw)?;
    let dir = files::project_dir(&st.cfg.projects_dir(), id);
    let target = dir.join(&path);
    let guard = st.project_lock(id);
    let _serial = guard.lock().await;
    {
        let d = dir.clone();
        let used = tokio::task::spawn_blocking(move || files::dir_size(&d)).await.unwrap_or(0);
        if used + body.len() as u64 > st.cfg.project_quota {
            return Err(ApiError::Bad("This project is full. Remove old files or ask your admin for more space.".into()));
        }
    }
    let is_new = !target.exists();
    if !is_new {
        match st.db.lock_of(id, &path) {
            Some(l) if l.user_id == u.id => {}
            Some(l) => return Err(ApiError::Conflict(json!({"error": format!("{} is editing this file. Your changes were not saved.", l.user_name), "lock": l}))),
            None => return Err(ApiError::Conflict(json!({"error": "Open the file for editing before saving it; someone may have changed it meanwhile."}))),
        }
    }
    if files::kind_of(&path) == "design" && path.ends_with(".json") && serde_json::from_slice::<Value>(&body).is_err() {
        return Err(ApiError::Bad("That design isn't valid JSON.".into()));
    }
    if let Some(parent) = target.parent() {
        tokio::fs::create_dir_all(parent).await.map_err(|e| ApiError::Internal(e.to_string()))?;
    }
    if is_new {
        // Create exclusively: if someone else created it a moment ago, don't replace theirs.
        let t = target.clone();
        let created = tokio::task::spawn_blocking(move || std::fs::OpenOptions::new().write(true).create_new(true).open(&t)).await.map_err(|e| ApiError::Internal(e.to_string()))?;
        if let Err(e) = created {
            return if e.kind() == std::io::ErrorKind::AlreadyExists {
                Err(ApiError::Conflict(json!({"error": "A file with that name was just added by someone else. Pick another name."})))
            } else {
                Err(ApiError::Internal(e.to_string()))
            };
        }
    }
    tokio::fs::write(&target, &body).await.map_err(|e| ApiError::Internal(e.to_string()))?;
    drop(body);
    let message = q.message.map(|m| m.chars().filter(|c| !auth::is_hidden_char(*c)).take(200).collect::<String>()).filter(|m| !m.trim().is_empty()).unwrap_or_else(|| format!("Saved {path}"));
    let version = files::commit(&dir, &[&path], &message, &u.name, &u.email).await?;
    drop(_serial);
    st.hub.publish(id, json!({"type": "saved", "path": path, "version": version, "by": {"id": u.id, "name": u.name, "color": u.color}}));
    // A CAD upload gets its design made in the background, one at a time, from the file on disk
    // (the upload itself is already saved and its memory freed).
    let converting = files::kind_of(&path) == "cad" && st.cfg.cli.is_some() && crate::convert::queue_job(&st, id, path.clone(), u.clone());
    Ok(Json(json!({"version": version, "converting": converting, "queue_full": files::kind_of(&path) == "cad" && st.cfg.cli.is_some() && !converting})))
}

#[derive(Deserialize)]
struct HistQ {
    path: Option<String>,
}

async fn project_history(State(st): State<AppState>, CurrentUser(u): CurrentUser, Path(id): Path<i64>, Query(q): Query<HistQ>) -> R<Json<Value>> {
    project_for(&st, id, &u)?;
    let path = q.path.as_deref().map(path_of).transpose()?;
    let dir = files::project_dir(&st.cfg.projects_dir(), id);
    Ok(Json(json!({"versions": files::history(&dir, path.as_deref(), 200).await?})))
}

#[derive(Deserialize)]
struct RestoreReq {
    path: String,
    version: String,
}

async fn restore(State(st): State<AppState>, CurrentUser(u): CurrentUser, Path(id): Path<i64>, Json(r): Json<RestoreReq>) -> R<Json<Value>> {
    let p = project_for(&st, id, &u)?;
    if !can_edit(&st, &p, &u) {
        return Err(ApiError::Forbidden("You can view this project but not change it."));
    }
    let path = path_of(&r.path)?;
    let dir = files::project_dir(&st.cfg.projects_dir(), id);
    let guard = st.project_lock(id);
    let _serial = guard.lock().await;
    if let Some(l) = st.db.lock_of(id, &path) {
        if l.user_id != u.id {
            return Err(ApiError::Conflict(json!({"error": format!("{} is editing this file; restore it once they're done.", l.user_name), "lock": l})));
        }
    }
    let bytes = files::read_at(&dir, &path, &r.version).await.map_err(|_| ApiError::NotFound)?;
    tokio::fs::write(dir.join(&path), bytes).await.map_err(|e| ApiError::Internal(e.to_string()))?;
    let version = files::commit(&dir, &[&path], &format!("Restored {path} to version {}", r.version), &u.name, &u.email).await?;
    st.hub.publish(id, json!({"type": "saved", "path": path, "version": version, "by": {"id": u.id, "name": u.name, "color": u.color}, "restored_from": r.version}));
    Ok(Json(json!({"version": version})))
}

// ---- locks -------------------------------------------------------------------------------

#[derive(Deserialize)]
struct PathReq {
    path: String,
}

/// Take (or keep, as a heartbeat) the edit lock on a file.
async fn lock(State(st): State<AppState>, CurrentUser(u): CurrentUser, Path(id): Path<i64>, Json(r): Json<PathReq>) -> R<Json<Value>> {
    let p = project_for(&st, id, &u)?;
    if !can_edit(&st, &p, &u) {
        return Err(ApiError::Forbidden("You can view this project but not change it."));
    }
    let path = path_of(&r.path)?;
    let before = st.db.lock_of(id, &path);
    let holder = st.db.acquire_lock(id, &path, u.id, st.cfg.lock_stale_secs)?;
    let granted = holder.user_id == u.id;
    if granted && before.map(|b| b.user_id) != Some(u.id) {
        st.hub.publish(id, json!({"type": "locks", "locks": st.db.locks(id)}));
    }
    Ok(Json(json!({"granted": granted, "lock": holder})))
}

async fn unlock(State(st): State<AppState>, CurrentUser(u): CurrentUser, Path(id): Path<i64>, Json(r): Json<PathReq>) -> R<Json<Value>> {
    project_for(&st, id, &u)?;
    let path = path_of(&r.path)?;
    if st.db.release_lock(id, &path, Some(u.id))? > 0 {
        st.hub.publish(id, json!({"type": "locks", "locks": st.db.locks(id)}));
    }
    Ok(Json(json!({"ok": true})))
}

async fn force_unlock(State(st): State<AppState>, CurrentUser(u): CurrentUser, Path(id): Path<i64>, Json(r): Json<PathReq>) -> R<Json<Value>> {
    let p = project_for(&st, id, &u)?;
    if !can_manage(&st, &p, &u) {
        return Err(ApiError::Forbidden("Only the project owner, a team owner or an admin can free someone else's file."));
    }
    let path = path_of(&r.path)?;
    let was = st.db.lock_of(id, &path);
    st.db.release_lock(id, &path, None)?;
    st.db.audit(Some(u.id), "force-unlock", &format!("{id} {path} from {:?}", was.map(|w| w.user_name)));
    st.hub.publish(id, json!({"type": "locks", "locks": st.db.locks(id), "forced": {"path": path, "by": u.name}}));
    Ok(Json(json!({"ok": true})))
}

// ---- comments ----------------------------------------------------------------------------

async fn list_comments(State(st): State<AppState>, CurrentUser(u): CurrentUser, Path(id): Path<i64>, Query(q): Query<PathReq>) -> R<Json<Value>> {
    project_for(&st, id, &u)?;
    let path = path_of(&q.path)?;
    Ok(Json(json!({"comments": st.db.comments(id, &path)})))
}

#[derive(Deserialize)]
struct CommentReq {
    path: String,
    body: String,
}

async fn add_comment(State(st): State<AppState>, CurrentUser(u): CurrentUser, Path(id): Path<i64>, Json(r): Json<CommentReq>) -> R<Json<Value>> {
    project_for(&st, id, &u)?;
    let path = path_of(&r.path)?;
    let body = r.body.trim();
    if body.is_empty() || body.chars().count() > 4000 {
        return Err(ApiError::Bad("Write a comment (up to 4000 characters).".into()));
    }
    let cid = st.db.add_comment(id, &path, u.id, body)?;
    st.hub.publish(id, json!({"type": "comment", "path": path, "id": cid, "by": {"id": u.id, "name": u.name, "color": u.color}, "body": body}));
    Ok(Json(json!({"id": cid})))
}
