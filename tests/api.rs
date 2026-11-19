//! End-to-end API tests: sign-up and approval, team isolation, roles, edit locks, versions.

use axum::body::Body;
use axum::http::{header, Method, Request, StatusCode};
use http_body_util::BodyExt;
use serde_json::{json, Value};
use solidcraft_collab::{app, auth, config::Config, AppState};
use tower::ServiceExt;

struct T {
    app: axum::Router,
    st: AppState,
    _dir: tempfile::TempDir,
}

impl T {
    fn new() -> T {
        let dir = tempfile::tempdir().unwrap();
        let mut cfg = Config::from_env();
        cfg.data_dir = dir.path().to_path_buf();
        cfg.app_dir = None;
        cfg.cli = None;
        cfg.secure_cookies = false;
        cfg.trust_cf_ip = true;
        let st = AppState::new(cfg).unwrap();
        T { app: app(st.clone()), st, _dir: dir }
    }

    async fn req(&self, method: Method, uri: &str, cookie: Option<&str>, body: Option<Value>) -> (StatusCode, Value, Option<String>) {
        let mut b = Request::builder().method(method.clone()).uri(uri).header("x-collab", "1");
        if let Some(c) = cookie {
            b = b.header(header::COOKIE, format!("{}={c}", auth::COOKIE_PLAIN));
        }
        let req = match body {
            Some(v) => b.header(header::CONTENT_TYPE, "application/json").body(Body::from(v.to_string())).unwrap(),
            None => b.body(Body::empty()).unwrap(),
        };
        let res = self.app.clone().oneshot(req).await.unwrap();
        let status = res.status();
        let set = res.headers().get(header::SET_COOKIE).and_then(|v| v.to_str().ok()).and_then(|v| v.split(';').next()).and_then(|kv| kv.split_once('=')).map(|(_, t)| t.to_string());
        let bytes = res.into_body().collect().await.unwrap().to_bytes();
        let v = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
        (status, v, set)
    }

    async fn raw_put(&self, uri: &str, cookie: &str, body: &str) -> (StatusCode, Value) {
        let req = Request::builder().method(Method::PUT).uri(uri).header("x-collab", "1").header(header::COOKIE, format!("{}={cookie}", auth::COOKIE_PLAIN)).body(Body::from(body.to_string())).unwrap();
        let res = self.app.clone().oneshot(req).await.unwrap();
        let s = res.status();
        let bytes = res.into_body().collect().await.unwrap().to_bytes();
        (s, serde_json::from_slice(&bytes).unwrap_or(Value::Null))
    }

    async fn raw_get(&self, uri: &str, cookie: &str) -> (StatusCode, String) {
        let req = Request::builder().uri(uri).header(header::COOKIE, format!("{}={cookie}", auth::COOKIE_PLAIN)).body(Body::empty()).unwrap();
        let res = self.app.clone().oneshot(req).await.unwrap();
        let s = res.status();
        let bytes = res.into_body().collect().await.unwrap().to_bytes();
        (s, String::from_utf8_lossy(&bytes).into_owned())
    }

    fn admin(&self) -> i64 {
        self.st.db.create_user("admin@example.com", "Admin", &auth::hash_password("admin-password-1").unwrap(), "active", true).unwrap()
    }

    async fn login(&self, email: &str, pw: &str) -> (StatusCode, Option<String>) {
        let (s, _, c) = self.req(Method::POST, "/api/auth/login", None, Some(json!({"email": email, "password": pw}))).await;
        (s, c)
    }

    /// Sign up, approve into `team` with `role`, sign in; returns (user id, cookie).
    async fn person(&self, admin: &str, email: &str, name: &str, team: i64, role: &str) -> (i64, String) {
        let (s, _, _) = self.req(Method::POST, "/api/auth/signup", None, Some(json!({"email": email, "name": name, "password": "a-long-password"}))).await;
        assert_eq!(s, StatusCode::OK);
        let id = self.st.db.user_by_email_with_hash(email).unwrap().0.id;
        let (s, _, _) = self.req(Method::POST, &format!("/api/admin/users/{id}/approve"), Some(admin), Some(json!({"team_id": team, "role": role}))).await;
        assert_eq!(s, StatusCode::OK);
        let (s, c) = self.login(email, "a-long-password").await;
        assert_eq!(s, StatusCode::OK);
        (id, c.unwrap())
    }
}

#[tokio::test]
async fn signup_needs_admin_approval() {
    let t = T::new();
    t.admin();
    let (s, v, _) = t.req(Method::POST, "/api/auth/signup", None, Some(json!({"email": "alex.personal@example.com", "name": "Alex", "password": "a-long-password"}))).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v["status"], "pending");
    let (s, _) = t.login("alex.personal@example.com", "a-long-password").await;
    assert_eq!(s, StatusCode::FORBIDDEN, "a pending account can't sign in");

    let (_, admin) = t.login("admin@example.com", "admin-password-1").await;
    let admin = admin.unwrap();
    let (_, v, _) = t.req(Method::GET, "/api/admin/users?status=pending", Some(&admin), None).await;
    assert_eq!(v["users"].as_array().unwrap().len(), 1);
    let team = t.st.db.create_team("Acme").unwrap();
    let id = v["users"][0]["user"]["id"].as_i64().unwrap();
    let (s, _, _) = t.req(Method::POST, &format!("/api/admin/users/{id}/approve"), Some(&admin), Some(json!({"team_id": team, "role": "editor"}))).await;
    assert_eq!(s, StatusCode::OK);
    let (s, c) = t.login("ALEX.personal@example.com", "a-long-password").await;
    assert_eq!(s, StatusCode::OK, "email is case-insensitive");
    let (_, me, _) = t.req(Method::GET, "/api/me", Some(&c.unwrap()), None).await;
    assert_eq!(me["teams"][0]["role"], "editor");
}

#[tokio::test]
async fn weak_input_is_refused() {
    let t = T::new();
    let (s, _, _) = t.req(Method::POST, "/api/auth/signup", None, Some(json!({"email": "x@y.com", "name": "X", "password": "short"}))).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    let (s, _, _) = t.req(Method::POST, "/api/auth/signup", None, Some(json!({"email": "not-an-email", "name": "X", "password": "a-long-password"}))).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    let (s, _, _) = t.req(Method::GET, "/api/me", None, None).await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn non_admin_cannot_use_admin_routes() {
    let t = T::new();
    t.admin();
    let (_, admin) = t.login("admin@example.com", "admin-password-1").await;
    let team = t.st.db.create_team("A").unwrap();
    let (_, c) = t.person(&admin.unwrap(), "e@x.com", "E", team, "owner").await;
    let (s, _, _) = t.req(Method::GET, "/api/admin/users", Some(&c), None).await;
    assert_eq!(s, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn teams_cannot_see_each_other() {
    let t = T::new();
    t.admin();
    let (_, admin) = t.login("admin@example.com", "admin-password-1").await;
    let admin = admin.unwrap();
    let a = t.st.db.create_team("Team A").unwrap();
    let b = t.st.db.create_team("Team B").unwrap();
    let (_, alice) = t.person(&admin, "alice@a.com", "Alice", a, "editor").await;
    let (_, bob) = t.person(&admin, "bob@b.com", "Bob", b, "owner").await;

    let (s, v, _) = t.req(Method::POST, &format!("/api/teams/{a}/projects"), Some(&alice), Some(json!({"name": "530 fan", "kind": "joint"}))).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let pid = v["id"].as_i64().unwrap();
    let (s, _) = t.raw_put(&format!("/api/projects/{pid}/files/hub.json"), &alice, r#"{"format":"solidcraft/51"}"#).await;
    assert_eq!(s, StatusCode::OK);

    for uri in [format!("/api/projects/{pid}"), format!("/api/projects/{pid}/history"), format!("/api/teams/{a}/projects"), format!("/api/teams/{a}/members")] {
        let (s, _, _) = t.req(Method::GET, &uri, Some(&bob), None).await;
        assert_eq!(s, StatusCode::NOT_FOUND, "{uri} leaked to another team");
    }
    let (s, _) = t.raw_get(&format!("/api/projects/{pid}/files/hub.json"), &bob).await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    let (s, _) = t.raw_put(&format!("/api/projects/{pid}/files/evil.json"), &bob, "{}").await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    let (s, _, _) = t.req(Method::POST, &format!("/api/projects/{pid}/lock"), Some(&bob), Some(json!({"path": "hub.json"}))).await;
    assert_eq!(s, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn personal_projects_are_private_until_shared() {
    let t = T::new();
    t.admin();
    let (_, admin) = t.login("admin@example.com", "admin-password-1").await;
    let admin = admin.unwrap();
    let a = t.st.db.create_team("A").unwrap();
    let (_, sam) = t.person(&admin, "sam@x.com", "Sam", a, "editor").await;
    let (alex_id, alex) = t.person(&admin, "alex@x.com", "Alex", a, "editor").await;
    let (_, v, _) = t.req(Method::POST, &format!("/api/teams/{a}/projects"), Some(&sam), Some(json!({"name": "experiments", "kind": "personal"}))).await;
    let pid = v["id"].as_i64().unwrap();
    let (s, _, _) = t.req(Method::GET, &format!("/api/projects/{pid}"), Some(&alex), None).await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    let (s, _, _) = t.req(Method::POST, &format!("/api/projects/{pid}/members"), Some(&sam), Some(json!({"user_id": alex_id}))).await;
    assert_eq!(s, StatusCode::OK);
    let (s, _, _) = t.req(Method::GET, &format!("/api/projects/{pid}"), Some(&alex), None).await;
    assert_eq!(s, StatusCode::OK);
}

#[tokio::test]
async fn locks_let_people_edit_different_parts_at_once() {
    let t = T::new();
    t.admin();
    let (_, admin) = t.login("admin@example.com", "admin-password-1").await;
    let admin = admin.unwrap();
    let a = t.st.db.create_team("A").unwrap();
    let (_, sam) = t.person(&admin, "sam@x.com", "Sam", a, "editor").await;
    let (_, alex) = t.person(&admin, "alex@x.com", "Alex", a, "editor").await;
    let (_, viewer) = t.person(&admin, "v@x.com", "Vee", a, "viewer").await;
    let (_, v, _) = t.req(Method::POST, &format!("/api/teams/{a}/projects"), Some(&sam), Some(json!({"name": "fan", "kind": "joint"}))).await;
    let pid = v["id"].as_i64().unwrap();
    t.raw_put(&format!("/api/projects/{pid}/files/hub.json"), &sam, r#"{"v":1}"#).await;
    t.raw_put(&format!("/api/projects/{pid}/files/blade.json"), &alex, r#"{"v":1}"#).await;

    let lock = |c: String, p: &'static str| {
        let t = &t;
        async move { t.req(Method::POST, &format!("/api/projects/{pid}/lock"), Some(&c), Some(json!({"path": p}))).await.1 }
    };
    assert_eq!(lock(sam.clone(), "hub.json").await["granted"], true);
    assert_eq!(lock(alex.clone(), "blade.json").await["granted"], true, "a different part is free");
    let other = lock(alex.clone(), "hub.json").await;
    assert_eq!(other["granted"], false);
    assert_eq!(other["lock"]["user_name"], "Sam", "shows who holds it");

    let (s, v) = t.raw_put(&format!("/api/projects/{pid}/files/hub.json"), &alex, r#"{"v":2}"#).await;
    assert_eq!(s, StatusCode::CONFLICT, "can't save a part someone else is editing: {v}");
    let (s, _) = t.raw_put(&format!("/api/projects/{pid}/files/hub.json"), &sam, r#"{"v":2}"#).await;
    assert_eq!(s, StatusCode::OK);
    let (s, _) = t.raw_put(&format!("/api/projects/{pid}/files/blade.json"), &alex, r#"{"v":2}"#).await;
    assert_eq!(s, StatusCode::OK);

    let (s, _) = t.raw_put(&format!("/api/projects/{pid}/files/new.json"), &viewer, "{}").await;
    assert_eq!(s, StatusCode::FORBIDDEN, "viewers can't save");

    t.req(Method::DELETE, &format!("/api/projects/{pid}/lock"), Some(&sam), Some(json!({"path": "hub.json"}))).await;
    assert_eq!(lock(alex.clone(), "hub.json").await["granted"], true, "freed for the next person");

    let (_, info, _) = t.req(Method::GET, &format!("/api/projects/{pid}"), Some(&sam), None).await;
    assert_eq!(info["locks"].as_array().unwrap().len(), 2);
}

#[tokio::test]
async fn every_save_is_a_version_that_can_be_restored() {
    let t = T::new();
    t.admin();
    let (_, admin) = t.login("admin@example.com", "admin-password-1").await;
    let a = t.st.db.create_team("A").unwrap();
    let (_, c) = t.person(&admin.unwrap(), "sam@x.com", "Sam", a, "editor").await;
    let (_, v, _) = t.req(Method::POST, &format!("/api/teams/{a}/projects"), Some(&c), Some(json!({"name": "fan", "kind": "joint"}))).await;
    let pid = v["id"].as_i64().unwrap();
    let (_, v1) = t.raw_put(&format!("/api/projects/{pid}/files/hub.json?message=first"), &c, r#"{"v":1}"#).await;
    t.req(Method::POST, &format!("/api/projects/{pid}/lock"), Some(&c), Some(json!({"path": "hub.json"}))).await;
    t.raw_put(&format!("/api/projects/{pid}/files/hub.json"), &c, r#"{"v":2}"#).await;
    let (_, h, _) = t.req(Method::GET, &format!("/api/projects/{pid}/history?path=hub.json"), Some(&c), None).await;
    let versions = h["versions"].as_array().unwrap();
    assert_eq!(versions.len(), 2);
    assert_eq!(versions[0]["author"], "Sam");
    assert_eq!(versions[1]["message"], "first");

    let first = v1["version"].as_str().unwrap();
    let (_, old) = t.raw_get(&format!("/api/projects/{pid}/files/hub.json?version={first}"), &c).await;
    assert_eq!(old, r#"{"v":1}"#);
    let (s, _, _) = t.req(Method::POST, &format!("/api/projects/{pid}/restore"), Some(&c), Some(json!({"path": "hub.json", "version": first}))).await;
    assert_eq!(s, StatusCode::OK);
    let (_, now) = t.raw_get(&format!("/api/projects/{pid}/files/hub.json"), &c).await;
    assert_eq!(now, r#"{"v":1}"#);
}

#[tokio::test]
async fn unsafe_file_names_are_refused() {
    let t = T::new();
    t.admin();
    let (_, admin) = t.login("admin@example.com", "admin-password-1").await;
    let a = t.st.db.create_team("A").unwrap();
    let (_, c) = t.person(&admin.unwrap(), "sam@x.com", "Sam", a, "editor").await;
    let (_, v, _) = t.req(Method::POST, &format!("/api/teams/{a}/projects"), Some(&c), Some(json!({"name": "fan", "kind": "joint"}))).await;
    let pid = v["id"].as_i64().unwrap();
    for bad in ["..%2F..%2Fcollab.sqlite", ".git%2Fconfig", "a%2F..%2F..%2Fx", "%2Fetc%2Fpasswd"] {
        let (s, _) = t.raw_put(&format!("/api/projects/{pid}/files/{bad}"), &c, "x").await;
        assert_ne!(s, StatusCode::OK, "{bad} was accepted");
        let (s, _) = t.raw_get(&format!("/api/projects/{pid}/files/{bad}"), &c).await;
        assert_ne!(s, StatusCode::OK, "{bad} was readable");
    }
    assert!(solidcraft_collab::files::clean_path("parts/hub v2.json").is_some());
    assert!(solidcraft_collab::files::clean_path("../x").is_none());
}

#[tokio::test]
async fn repeated_wrong_passwords_pause_sign_in() {
    let t = T::new();
    t.admin();
    for _ in 0..8 {
        let (s, _) = t.login("admin@example.com", "wrong-password").await;
        assert_eq!(s, StatusCode::BAD_REQUEST);
    }
    let (s, _) = t.login("admin@example.com", "admin-password-1").await;
    assert_eq!(s, StatusCode::TOO_MANY_REQUESTS, "even the right password waits");
}


// ---- security fixes (review of 2026-11-08) ----------------------------------------------

async fn setup_team(t: &T) -> (String, i64) {
    t.admin();
    let (_, admin) = t.login("admin@example.com", "admin-password-1").await;
    (admin.unwrap(), t.st.db.create_team("A").unwrap())
}

#[tokio::test]
async fn state_changes_need_the_csrf_header() {
    let t = T::new();
    let req = Request::builder().method(Method::POST).uri("/api/auth/signup").header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(json!({"email": "x@y.com", "name": "X", "password": "a-long-password"}).to_string())).unwrap();
    let res = t.app.clone().oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::FORBIDDEN);
    let req = Request::builder().method(Method::POST).uri("/api/auth/logout").body(Body::empty()).unwrap();
    assert_eq!(t.app.clone().oneshot(req).await.unwrap().status(), StatusCode::FORBIDDEN, "logout can't be forced cross-site");
}

#[tokio::test]
async fn one_time_password_only_reaches_password_change() {
    let t = T::new();
    let (admin, team) = setup_team(&t).await;
    let (id, _) = t.person(&admin, "alex@x.com", "Alex", team, "editor").await;
    let (_, r, _) = t.req(Method::POST, &format!("/api/admin/users/{id}/reset-password"), Some(&admin), None).await;
    let temp = r["temporary_password"].as_str().unwrap().to_string();
    let (s, c) = t.login("alex@x.com", &temp).await;
    assert_eq!(s, StatusCode::OK);
    let c = c.unwrap();
    let (s, _, _) = t.req(Method::GET, "/api/teams", Some(&c), None).await;
    assert_eq!(s, StatusCode::FORBIDDEN, "must change the password first");
    let (s, _, _) = t.req(Method::GET, &format!("/api/teams/{team}/projects"), Some(&c), None).await;
    assert_eq!(s, StatusCode::FORBIDDEN);
    let (s, _, _) = t.req(Method::GET, "/api/me", Some(&c), None).await;
    assert_eq!(s, StatusCode::OK);
    let (s, v, _) = t.req(Method::POST, "/api/auth/password", Some(&c), Some(json!({"current": temp, "new": "my-own-password"}))).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let (_, c2) = t.login("alex@x.com", "my-own-password").await;
    let (s, _, _) = t.req(Method::GET, "/api/teams", Some(&c2.unwrap()), None).await;
    assert_eq!(s, StatusCode::OK);
}

#[tokio::test]
async fn signup_does_not_reveal_existing_accounts() {
    let t = T::new();
    t.admin();
    let body = |e: &str| json!({"email": e, "name": "Someone", "password": "a-long-password"});
    let (s1, v1, _) = t.req(Method::POST, "/api/auth/signup", None, Some(body("admin@example.com"))).await;
    let (s2, v2, _) = t.req(Method::POST, "/api/auth/signup", None, Some(body("new@example.com"))).await;
    assert_eq!((s1, &v1), (s2, &v2), "same answer for a taken and a free email");
    assert!(t.st.db.user_by_email_with_hash("admin@example.com").unwrap().0.is_admin, "the existing account is untouched");
}

#[tokio::test]
async fn signup_and_sign_in_are_limited_per_address() {
    let t = T::new();
    t.admin();
    let send = |path: &'static str, body: Value| {
        let app = t.app.clone();
        async move {
            let req = Request::builder().method(Method::POST).uri(path).header("x-collab", "1").header("cf-connecting-ip", "203.0.113.9")
                .header(header::CONTENT_TYPE, "application/json").body(Body::from(body.to_string())).unwrap();
            app.oneshot(req).await.unwrap().status()
        }
    };
    for i in 0..5 {
        assert_eq!(send("/api/auth/signup", json!({"email": format!("s{i}@x.com"), "name": "S", "password": "a-long-password"})).await, StatusCode::OK);
    }
    assert_eq!(send("/api/auth/signup", json!({"email": "s9@x.com", "name": "S", "password": "a-long-password"})).await, StatusCode::TOO_MANY_REQUESTS);
    // Password spraying: many emails from one address.
    let mut last = StatusCode::OK;
    for i in 0..31 {
        last = send("/api/auth/login", json!({"email": format!("victim{i}@x.com"), "password": "guess-guess-1"})).await;
    }
    assert_eq!(last, StatusCode::TOO_MANY_REQUESTS);
}

#[tokio::test]
async fn team_owner_cannot_pull_in_outsiders_or_see_emails_as_editor() {
    let t = T::new();
    let (admin, a) = setup_team(&t).await;
    let b = t.st.db.create_team("B").unwrap();
    let (_, owner_a) = t.person(&admin, "owner@a.com", "OwnerA", a, "owner").await;
    let (outsider, _) = t.person(&admin, "out@b.com", "Outsider", b, "editor").await;
    let (s, _, _) = t.req(Method::POST, &format!("/api/teams/{a}/members"), Some(&owner_a), Some(json!({"user_id": outsider, "role": "viewer"}))).await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    let (_, editor_a) = t.person(&admin, "ed@a.com", "EdA", a, "editor").await;
    let (_, v, _) = t.req(Method::GET, &format!("/api/teams/{a}/members"), Some(&editor_a), None).await;
    assert!(v["members"].as_array().unwrap().iter().all(|m| m["email"].is_null()), "editors don't see emails: {v}");
    let (_, v, _) = t.req(Method::GET, &format!("/api/teams/{a}/members"), Some(&owner_a), None).await;
    assert!(v["members"].as_array().unwrap().iter().all(|m| m["email"].is_string()), "owners do");
}

#[tokio::test]
async fn concurrent_saves_all_commit_and_names_are_literal() {
    let t = T::new();
    let (admin, a) = setup_team(&t).await;
    let (_, c) = t.person(&admin, "sam@x.com", "Sam", a, "editor").await;
    let (_, v, _) = t.req(Method::POST, &format!("/api/teams/{a}/projects"), Some(&c), Some(json!({"name": "fan", "kind": "joint"}))).await;
    let pid = v["id"].as_i64().unwrap();
    let futs: Vec<_> = (0..8).map(|i| { let c = c.clone(); let t = &t; async move { t.raw_put(&format!("/api/projects/{pid}/files/part{i}.json"), &c, "{}").await.0 } }).collect();
    let results = futures_util::future::join_all(futs).await;
    assert!(results.iter().all(|s| *s == StatusCode::OK), "{results:?}");
    let (_, h, _) = t.req(Method::GET, &format!("/api/projects/{pid}/history"), Some(&c), None).await;
    assert_eq!(h["versions"].as_array().unwrap().len(), 9, "8 saves + project created");
    assert!(h["versions"][0].get("email").is_none(), "no emails in history");
    let (s, _) = t.raw_put(&format!("/api/projects/{pid}/files/%5Bp%5Dart1.json"), &c, "{}").await;
    assert_eq!(s, StatusCode::OK);
    let (_, h, _) = t.req(Method::GET, &format!("/api/projects/{pid}/history?path=part1.json"), Some(&c), None).await;
    assert_eq!(h["versions"].as_array().unwrap().len(), 1, "a name like [p]art1.json is not a pattern");
}

#[tokio::test]
async fn two_new_files_with_one_name_never_overwrite() {
    let t = T::new();
    let (admin, a) = setup_team(&t).await;
    let (_, c1) = t.person(&admin, "one@x.com", "One", a, "editor").await;
    let (_, c2) = t.person(&admin, "two@x.com", "Two", a, "editor").await;
    let (_, v, _) = t.req(Method::POST, &format!("/api/teams/{a}/projects"), Some(&c1), Some(json!({"name": "fan", "kind": "joint"}))).await;
    let pid = v["id"].as_i64().unwrap();
    let u = format!("/api/projects/{pid}/files/same.json");
    let (a1, a2) = tokio::join!(t.raw_put(&u, &c1, r#"{"by":1}"#), t.raw_put(&u, &c2, r#"{"by":2}"#));
    let codes = [a1.0, a2.0];
    assert!(codes.contains(&StatusCode::OK) && codes.contains(&StatusCode::CONFLICT), "{codes:?}");
}

#[tokio::test]
async fn uploads_larger_than_two_megabytes_work_and_quota_holds() {
    let dir = tempfile::tempdir().unwrap();
    let mut cfg = Config::from_env();
    cfg.data_dir = dir.path().to_path_buf();
    cfg.app_dir = None;
    cfg.cli = None;
    cfg.secure_cookies = false;
    cfg.project_quota = 6 * 1024 * 1024;
    let st = AppState::new(cfg).unwrap();
    let t = T { app: app(st.clone()), st, _dir: dir };
    let (admin, a) = setup_team(&t).await;
    let (_, c) = t.person(&admin, "sam@x.com", "Sam", a, "editor").await;
    let (_, v, _) = t.req(Method::POST, &format!("/api/teams/{a}/projects"), Some(&c), Some(json!({"name": "fan", "kind": "joint"}))).await;
    let pid = v["id"].as_i64().unwrap();
    let big = "x".repeat(3 * 1024 * 1024);
    let (s, v) = t.raw_put(&format!("/api/projects/{pid}/files/big.stl"), &c, &big).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let (s, _) = t.raw_put(&format!("/api/projects/{pid}/files/big2.stl"), &c, &big).await;
    assert_eq!(s, StatusCode::BAD_REQUEST, "project quota");
}

#[tokio::test]
async fn pages_carry_security_headers_and_secure_cookie_is_host_prefixed() {
    let dir = tempfile::tempdir().unwrap();
    let mut cfg = Config::from_env();
    cfg.data_dir = dir.path().to_path_buf();
    cfg.app_dir = None;
    cfg.cli = None;
    cfg.secure_cookies = true;
    let st = AppState::new(cfg).unwrap();
    st.db.create_user("admin@example.com", "Admin", &auth::hash_password("admin-password-1").unwrap(), "active", true).unwrap();
    let app = app(st);
    let res = app.clone().oneshot(Request::builder().uri("/").body(Body::empty()).unwrap()).await.unwrap();
    let csp = res.headers().get(header::CONTENT_SECURITY_POLICY).unwrap().to_str().unwrap();
    assert!(csp.contains("script-src 'self'") && !csp.contains("unsafe-eval"), "{csp}");
    assert!(res.headers().contains_key(header::STRICT_TRANSPORT_SECURITY));
    let req = Request::builder().method(Method::POST).uri("/api/auth/login").header("x-collab", "1").header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(json!({"email": "admin@example.com", "password": "admin-password-1"}).to_string())).unwrap();
    let res = app.oneshot(req).await.unwrap();
    let cookie = res.headers().get(header::SET_COOKIE).unwrap().to_str().unwrap();
    assert!(cookie.starts_with("__Host-collab_session=") && cookie.contains("Secure") && cookie.contains("Path=/") && !cookie.contains("Domain"), "{cookie}");
}

#[tokio::test]
async fn live_channel_requires_this_sites_origin() {
    let t = T::new();
    let mut h = axum::http::HeaderMap::new();
    h.insert(header::HOST, "team.example.com".parse().unwrap());
    assert!(!solidcraft_collab::live::origin_ok(&t.st, &h), "no Origin");
    h.insert(header::ORIGIN, "https://crm.example.com".parse().unwrap());
    assert!(!solidcraft_collab::live::origin_ok(&t.st, &h), "sibling site");
    h.insert(header::ORIGIN, "https://team.example.com".parse().unwrap());
    assert!(solidcraft_collab::live::origin_ok(&t.st, &h));
}

#[test]
fn names_and_paths_refuse_hidden_characters() {
    use solidcraft_collab::files::clean_path;
    for bad in ["../x", ".git/config", "a/../b", "/etc/passwd", "a:b.json", "evil\u{202e}gpj.json", "a\nb", ".hidden"] {
        assert!(clean_path(bad).is_none(), "{bad:?}");
    }
    for good in ["-x.json", "parts/hub v2.json", "Fan (2).json", "blade_A-3.step"] {
        assert!(clean_path(good).is_some(), "{good:?}");
    }
    assert!(!auth::name_ok("Alex\u{202e}", 60));
    assert!(!auth::name_ok("Ra\nvi", 60));
    assert!(auth::name_ok("Meena K", 60));
}

// ---- second review (N1–N4) ---------------------------------------------------------------

#[tokio::test]
async fn huge_json_bodies_are_refused_before_use() {
    let t = T::new();
    let big = "a".repeat(1024 * 1024);
    let (s, _, _) = t.req(Method::POST, "/api/auth/login", None, Some(json!({"email": format!("{big}@x.com"), "password": "x"}))).await;
    assert_eq!(s, StatusCode::PAYLOAD_TOO_LARGE);
    let (s, _, _) = t.req(Method::POST, "/api/auth/login", None, Some(json!({"email": format!("{}@x.com", "a".repeat(300)), "password": "x"}))).await;
    assert_eq!(s, StatusCode::BAD_REQUEST, "over-long email refused before any limiter or hash");
}

/// A deliberately slow fake converter (sleeps, writes nothing), without the sandbox.
fn slow_cli(dir: &std::path::Path) -> std::path::PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let p = dir.join("slow-cli.sh");
    std::fs::write(&p, "#!/bin/sh\nsleep 4\nexit 1\n").unwrap();
    std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
    p
}

#[tokio::test]
async fn uploads_return_at_once_while_a_slow_conversion_runs() {
    let dir = tempfile::tempdir().unwrap();
    let mut cfg = Config::from_env();
    cfg.data_dir = dir.path().join("data");
    cfg.app_dir = None;
    cfg.cli = Some(slow_cli(dir.path()));
    cfg.convert_sandbox = "none".into();
    cfg.secure_cookies = false;
    let st = AppState::new(cfg).unwrap();
    let t = T { app: app(st.clone()), st, _dir: dir };
    let (admin, a) = setup_team(&t).await;
    let (_, c) = t.person(&admin, "sam@x.com", "Sam", a, "editor").await;
    let (_, v, _) = t.req(Method::POST, &format!("/api/teams/{a}/projects"), Some(&c), Some(json!({"name": "fan", "kind": "joint"}))).await;
    let pid = v["id"].as_i64().unwrap();
    let started = std::time::Instant::now();
    let (s, v) = t.raw_put(&format!("/api/projects/{pid}/files/one.step"), &c, "ISO-10303-21;").await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(v["converting"], true);
    let (s, _) = t.raw_put(&format!("/api/projects/{pid}/files/note.json"), &c, "{}").await;
    assert_eq!(s, StatusCode::OK);
    assert!(started.elapsed() < std::time::Duration::from_secs(2), "saves didn't wait for the conversion: {:?}", started.elapsed());
    // Fill the queue: further CAD uploads are still saved, just without a design.
    let mut full = false;
    for i in 0..10 {
        let (s, v) = t.raw_put(&format!("/api/projects/{pid}/files/q{i}.step"), &c, "ISO-10303-21;").await;
        assert_eq!(s, StatusCode::OK);
        full |= v["queue_full"] == true;
    }
    assert!(full, "the conversion queue is bounded");
}

/// On a machine with a user systemd: a converter that tries to reach the user's systemd/D-Bus
/// from inside the sandbox must fail. Run with COLLAB_TEST_SANDBOX=1.
#[tokio::test]
async fn sandboxed_converter_cannot_reach_user_systemd() {
    if std::env::var("COLLAB_TEST_SANDBOX").is_err() {
        eprintln!("skipped (set COLLAB_TEST_SANDBOX=1 on a host with systemd --user)");
        return;
    }
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir_in(std::env::var("HOME").unwrap()).unwrap();
    let probe = dir.path().join("probe.sh");
    // Args: run --in IN --out OUT --quiet. Writes a "design" only if the escape works.
    std::fs::write(&probe, "#!/bin/sh\nif busctl --user list >/dev/null 2>&1 || systemctl --user show-environment >/dev/null 2>&1; then echo '{\"format\":\"solidcraft/1\",\"escaped\":true}' > \"$5\"; fi\n").unwrap();
    std::fs::set_permissions(&probe, std::fs::Permissions::from_mode(0o755)).unwrap();
    let mut cfg = Config::from_env();
    cfg.data_dir = dir.path().join("data");
    cfg.convert_sandbox = "systemd".into();
    std::fs::create_dir_all(cfg.data_dir.join("projects/p1")).unwrap();
    let proj = cfg.data_dir.join("projects/p1");
    std::fs::write(proj.join("x.step"), "ISO-10303-21;").unwrap();
    let r = solidcraft_collab::convert::cad_to_design(&cfg, &probe, &proj, "x.step").await;
    assert!(r.is_err(), "the sandbox let the converter reach systemd: {r:?}");
    // Control: the same probe without the sandbox does reach it.
    cfg.convert_sandbox = "none".into();
    let r = solidcraft_collab::convert::cad_to_design(&cfg, &probe, &proj, "x.step").await;
    assert!(r.is_ok(), "control run should escape (proves the probe works): {r:?}");
}
