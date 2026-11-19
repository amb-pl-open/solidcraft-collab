//! SQLite storage for people, teams, projects, locks and comments. Design files themselves
//! live on disk in one git repository per project (see `files.rs`).

use std::path::Path;
use std::sync::Mutex;

use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;

pub fn now() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

const SCHEMA: &str = r#"
PRAGMA foreign_keys = ON;
PRAGMA journal_mode = WAL;
CREATE TABLE IF NOT EXISTS users (
  id INTEGER PRIMARY KEY,
  email TEXT NOT NULL UNIQUE COLLATE NOCASE,
  name TEXT NOT NULL,
  pw_hash TEXT NOT NULL,
  status TEXT NOT NULL CHECK (status IN ('pending','active','disabled')),
  is_admin INTEGER NOT NULL DEFAULT 0,
  must_change_pw INTEGER NOT NULL DEFAULT 0,
  color TEXT NOT NULL,
  created_at INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS sessions (
  token_hash TEXT PRIMARY KEY,
  user_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  expires_at INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS teams (
  id INTEGER PRIMARY KEY,
  name TEXT NOT NULL UNIQUE,
  created_at INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS memberships (
  team_id INTEGER NOT NULL REFERENCES teams(id) ON DELETE CASCADE,
  user_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  role TEXT NOT NULL CHECK (role IN ('owner','editor','viewer')),
  PRIMARY KEY (team_id, user_id)
);
CREATE TABLE IF NOT EXISTS projects (
  id INTEGER PRIMARY KEY,
  team_id INTEGER NOT NULL REFERENCES teams(id) ON DELETE CASCADE,
  name TEXT NOT NULL,
  kind TEXT NOT NULL CHECK (kind IN ('personal','joint')),
  owner_id INTEGER NOT NULL REFERENCES users(id),
  created_at INTEGER NOT NULL,
  UNIQUE (team_id, name)
);
CREATE TABLE IF NOT EXISTS project_members (
  project_id INTEGER NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
  user_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  PRIMARY KEY (project_id, user_id)
);
CREATE TABLE IF NOT EXISTS locks (
  project_id INTEGER NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
  path TEXT NOT NULL,
  user_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  acquired_at INTEGER NOT NULL,
  heartbeat_at INTEGER NOT NULL,
  PRIMARY KEY (project_id, path)
);
CREATE TABLE IF NOT EXISTS comments (
  id INTEGER PRIMARY KEY,
  project_id INTEGER NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
  path TEXT NOT NULL,
  user_id INTEGER NOT NULL REFERENCES users(id),
  body TEXT NOT NULL,
  created_at INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS audit (
  id INTEGER PRIMARY KEY,
  user_id INTEGER,
  action TEXT NOT NULL,
  detail TEXT NOT NULL,
  at INTEGER NOT NULL
);
"#;

/// Avatar colours, picked in turn for new people. Distinct, readable on light and dark.
const COLORS: [&str; 10] = ["#2563eb", "#db2777", "#16a34a", "#ea580c", "#7c3aed", "#0891b2", "#ca8a04", "#dc2626", "#4f46e5", "#0d9488"];

#[derive(Debug, Clone, Serialize)]
pub struct User {
    pub id: i64,
    pub email: String,
    pub name: String,
    pub status: String,
    pub is_admin: bool,
    pub must_change_pw: bool,
    pub color: String,
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct Team {
    pub id: i64,
    pub name: String,
    pub role: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Project {
    pub id: i64,
    pub team_id: i64,
    pub name: String,
    pub kind: String,
    pub owner_id: i64,
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct Lock {
    pub path: String,
    pub user_id: i64,
    pub user_name: String,
    pub color: String,
    pub acquired_at: i64,
    pub heartbeat_at: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct Comment {
    pub id: i64,
    pub path: String,
    pub user_id: i64,
    pub user_name: String,
    pub color: String,
    pub body: String,
    pub created_at: i64,
}

pub struct Db {
    conn: Mutex<Connection>,
}

fn user_from_row(r: &rusqlite::Row) -> rusqlite::Result<User> {
    Ok(User {
        id: r.get(0)?,
        email: r.get(1)?,
        name: r.get(2)?,
        status: r.get(3)?,
        is_admin: r.get::<_, i64>(4)? != 0,
        must_change_pw: r.get::<_, i64>(5)? != 0,
        color: r.get(6)?,
        created_at: r.get(7)?,
    })
}
const USER_COLS: &str = "id, email, name, status, is_admin, must_change_pw, color, created_at";

impl Db {
    pub fn open(path: &Path) -> anyhow::Result<Db> {
        let conn = Connection::open(path)?;
        conn.execute_batch(SCHEMA)?;
        Ok(Db { conn: Mutex::new(conn) })
    }

    fn c(&self) -> std::sync::MutexGuard<'_, Connection> {
        self.conn.lock().unwrap_or_else(|p| p.into_inner())
    }

    pub fn audit(&self, user_id: Option<i64>, action: &str, detail: &str) {
        let _ = self.c().execute("INSERT INTO audit (user_id, action, detail, at) VALUES (?1, ?2, ?3, ?4)", params![user_id, action, detail, now()]);
    }

    // ---- users -------------------------------------------------------------------------

    pub fn create_user(&self, email: &str, name: &str, pw_hash: &str, status: &str, is_admin: bool) -> rusqlite::Result<i64> {
        let c = self.c();
        let n: i64 = c.query_row("SELECT COUNT(*) FROM users", [], |r| r.get(0))?;
        let color = COLORS[(n as usize) % COLORS.len()];
        c.execute(
            "INSERT INTO users (email, name, pw_hash, status, is_admin, color, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![email.trim(), name.trim(), pw_hash, status, is_admin as i64, color, now()],
        )?;
        Ok(c.last_insert_rowid())
    }

    pub fn user(&self, id: i64) -> Option<User> {
        self.c().query_row(&format!("SELECT {USER_COLS} FROM users WHERE id = ?1"), [id], user_from_row).optional().ok().flatten()
    }

    pub fn user_by_email_with_hash(&self, email: &str) -> Option<(User, String)> {
        self.c()
            .query_row(&format!("SELECT {USER_COLS}, pw_hash FROM users WHERE email = ?1"), [email.trim()], |r| Ok((user_from_row(r)?, r.get(8)?)))
            .optional()
            .ok()
            .flatten()
    }

    pub fn users(&self, status: Option<&str>) -> Vec<User> {
        let c = self.c();
        let sql = match status {
            Some(_) => format!("SELECT {USER_COLS} FROM users WHERE status = ?1 ORDER BY created_at"),
            None => format!("SELECT {USER_COLS} FROM users ORDER BY created_at"),
        };
        let mut st = match c.prepare(&sql) {
            Ok(s) => s,
            Err(_) => return vec![],
        };
        let rows = match status {
            Some(s) => st.query_map([s], user_from_row).map(|r| r.filter_map(Result::ok).collect()),
            None => st.query_map([], user_from_row).map(|r| r.filter_map(Result::ok).collect()),
        };
        rows.unwrap_or_default()
    }

    pub fn set_user_status(&self, id: i64, status: &str) -> rusqlite::Result<usize> {
        self.c().execute("UPDATE users SET status = ?2 WHERE id = ?1", params![id, status])
    }

    pub fn set_admin(&self, id: i64, admin: bool) -> rusqlite::Result<usize> {
        self.c().execute("UPDATE users SET is_admin = ?2 WHERE id = ?1", params![id, admin as i64])
    }

    pub fn set_password(&self, id: i64, pw_hash: &str, must_change: bool) -> rusqlite::Result<usize> {
        let c = self.c();
        c.execute("DELETE FROM sessions WHERE user_id = ?1", [id])?;
        c.execute("UPDATE users SET pw_hash = ?2, must_change_pw = ?3 WHERE id = ?1", params![id, pw_hash, must_change as i64])
    }

    pub fn delete_user(&self, id: i64) -> rusqlite::Result<usize> {
        self.c().execute("DELETE FROM users WHERE id = ?1 AND status = 'pending'", [id])
    }

    pub fn admin_count(&self) -> i64 {
        self.c().query_row("SELECT COUNT(*) FROM users WHERE is_admin = 1 AND status = 'active'", [], |r| r.get(0)).unwrap_or(0)
    }

    // ---- sessions ----------------------------------------------------------------------

    pub fn create_session(&self, token_hash: &str, user_id: i64, ttl_secs: i64) -> rusqlite::Result<usize> {
        let c = self.c();
        c.execute("DELETE FROM sessions WHERE expires_at < ?1", [now()])?;
        c.execute("INSERT INTO sessions (token_hash, user_id, expires_at) VALUES (?1, ?2, ?3)", params![token_hash, user_id, now() + ttl_secs])
    }

    pub fn session_user(&self, token_hash: &str) -> Option<User> {
        self.c()
            .query_row(
                &format!("SELECT u.{} FROM sessions s JOIN users u ON u.id = s.user_id WHERE s.token_hash = ?1 AND s.expires_at > ?2 AND u.status = 'active'", USER_COLS.replace(", ", ", u.")),
                params![token_hash, now()],
                user_from_row,
            )
            .optional()
            .ok()
            .flatten()
    }

    pub fn delete_session(&self, token_hash: &str) {
        let _ = self.c().execute("DELETE FROM sessions WHERE token_hash = ?1", [token_hash]);
    }

    // ---- teams -------------------------------------------------------------------------

    pub fn create_team(&self, name: &str) -> rusqlite::Result<i64> {
        let c = self.c();
        c.execute("INSERT INTO teams (name, created_at) VALUES (?1, ?2)", params![name.trim(), now()])?;
        Ok(c.last_insert_rowid())
    }

    pub fn all_teams(&self) -> Vec<Team> {
        let c = self.c();
        let mut st = match c.prepare("SELECT id, name FROM teams ORDER BY name") {
            Ok(s) => s,
            Err(_) => return vec![],
        };
        st.query_map([], |r| Ok(Team { id: r.get(0)?, name: r.get(1)?, role: None })).map(|r| r.filter_map(Result::ok).collect()).unwrap_or_default()
    }

    pub fn teams_of(&self, user_id: i64) -> Vec<Team> {
        let c = self.c();
        let mut st = match c.prepare("SELECT t.id, t.name, m.role FROM teams t JOIN memberships m ON m.team_id = t.id WHERE m.user_id = ?1 ORDER BY t.name") {
            Ok(s) => s,
            Err(_) => return vec![],
        };
        st.query_map([user_id], |r| Ok(Team { id: r.get(0)?, name: r.get(1)?, role: Some(r.get(2)?) })).map(|r| r.filter_map(Result::ok).collect()).unwrap_or_default()
    }

    pub fn role_in_team(&self, team_id: i64, user_id: i64) -> Option<String> {
        self.c().query_row("SELECT role FROM memberships WHERE team_id = ?1 AND user_id = ?2", params![team_id, user_id], |r| r.get(0)).optional().ok().flatten()
    }

    pub fn set_membership(&self, team_id: i64, user_id: i64, role: &str) -> rusqlite::Result<usize> {
        self.c().execute(
            "INSERT INTO memberships (team_id, user_id, role) VALUES (?1, ?2, ?3) ON CONFLICT(team_id, user_id) DO UPDATE SET role = excluded.role",
            params![team_id, user_id, role],
        )
    }

    pub fn remove_membership(&self, team_id: i64, user_id: i64) -> rusqlite::Result<usize> {
        let c = self.c();
        c.execute("DELETE FROM project_members WHERE user_id = ?2 AND project_id IN (SELECT id FROM projects WHERE team_id = ?1)", params![team_id, user_id])?;
        c.execute("DELETE FROM memberships WHERE team_id = ?1 AND user_id = ?2", params![team_id, user_id])
    }

    pub fn team_members(&self, team_id: i64) -> Vec<(User, String)> {
        let c = self.c();
        let sql = format!("SELECT u.{}, m.role FROM memberships m JOIN users u ON u.id = m.user_id WHERE m.team_id = ?1 ORDER BY u.name", USER_COLS.replace(", ", ", u."));
        let mut st = match c.prepare(&sql) {
            Ok(s) => s,
            Err(_) => return vec![],
        };
        st.query_map([team_id], |r| Ok((user_from_row(r)?, r.get(8)?))).map(|r| r.filter_map(Result::ok).collect()).unwrap_or_default()
    }

    // ---- projects ----------------------------------------------------------------------

    pub fn create_project(&self, team_id: i64, name: &str, kind: &str, owner_id: i64) -> rusqlite::Result<i64> {
        let c = self.c();
        c.execute(
            "INSERT INTO projects (team_id, name, kind, owner_id, created_at) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![team_id, name.trim(), kind, owner_id, now()],
        )?;
        let id = c.last_insert_rowid();
        c.execute("INSERT INTO project_members (project_id, user_id) VALUES (?1, ?2)", params![id, owner_id])?;
        if kind == "joint" {
            c.execute("INSERT OR IGNORE INTO project_members (project_id, user_id) SELECT ?1, user_id FROM memberships WHERE team_id = ?2", params![id, team_id])?;
        }
        Ok(id)
    }

    pub fn project(&self, id: i64) -> Option<Project> {
        self.c()
            .query_row("SELECT id, team_id, name, kind, owner_id, created_at FROM projects WHERE id = ?1", [id], |r| {
                Ok(Project { id: r.get(0)?, team_id: r.get(1)?, name: r.get(2)?, kind: r.get(3)?, owner_id: r.get(4)?, created_at: r.get(5)? })
            })
            .optional()
            .ok()
            .flatten()
    }

    /// Projects of a team that this person may open.
    pub fn visible_projects(&self, team_id: i64, user_id: i64) -> Vec<Project> {
        let c = self.c();
        let mut st = match c.prepare(
            "SELECT p.id, p.team_id, p.name, p.kind, p.owner_id, p.created_at FROM projects p JOIN project_members pm ON pm.project_id = p.id
             WHERE p.team_id = ?1 AND pm.user_id = ?2 ORDER BY p.kind DESC, p.name",
        ) {
            Ok(s) => s,
            Err(_) => return vec![],
        };
        st.query_map(params![team_id, user_id], |r| {
            Ok(Project { id: r.get(0)?, team_id: r.get(1)?, name: r.get(2)?, kind: r.get(3)?, owner_id: r.get(4)?, created_at: r.get(5)? })
        })
        .map(|r| r.filter_map(Result::ok).collect())
        .unwrap_or_default()
    }

    pub fn is_project_member(&self, project_id: i64, user_id: i64) -> bool {
        self.c()
            .query_row("SELECT 1 FROM project_members WHERE project_id = ?1 AND user_id = ?2", params![project_id, user_id], |_| Ok(()))
            .optional()
            .ok()
            .flatten()
            .is_some()
    }

    pub fn project_members(&self, project_id: i64) -> Vec<User> {
        let c = self.c();
        let sql = format!("SELECT u.{} FROM project_members pm JOIN users u ON u.id = pm.user_id WHERE pm.project_id = ?1 ORDER BY u.name", USER_COLS.replace(", ", ", u."));
        let mut st = match c.prepare(&sql) {
            Ok(s) => s,
            Err(_) => return vec![],
        };
        st.query_map([project_id], user_from_row).map(|r| r.filter_map(Result::ok).collect()).unwrap_or_default()
    }

    pub fn add_project_member(&self, project_id: i64, user_id: i64) -> rusqlite::Result<usize> {
        self.c().execute("INSERT OR IGNORE INTO project_members (project_id, user_id) VALUES (?1, ?2)", params![project_id, user_id])
    }

    pub fn remove_project_member(&self, project_id: i64, user_id: i64) -> rusqlite::Result<usize> {
        self.c().execute("DELETE FROM project_members WHERE project_id = ?1 AND user_id = ?2", params![project_id, user_id])
    }

    // ---- locks -------------------------------------------------------------------------

    /// Take the edit lock on a file. Returns the holder: this person when granted, someone
    /// else when the file is already taken (a stale lock is taken over).
    pub fn acquire_lock(&self, project_id: i64, path: &str, user_id: i64, stale_secs: i64) -> rusqlite::Result<Lock> {
        let c = self.c();
        let t = now();
        c.execute("DELETE FROM locks WHERE project_id = ?1 AND path = ?2 AND heartbeat_at < ?3", params![project_id, path, t - stale_secs])?;
        c.execute(
            "INSERT OR IGNORE INTO locks (project_id, path, user_id, acquired_at, heartbeat_at) VALUES (?1, ?2, ?3, ?4, ?4)",
            params![project_id, path, user_id, t],
        )?;
        c.execute("UPDATE locks SET heartbeat_at = ?4 WHERE project_id = ?1 AND path = ?2 AND user_id = ?3", params![project_id, path, user_id, t])?;
        drop(c);
        self.lock_of(project_id, path).ok_or(rusqlite::Error::QueryReturnedNoRows)
    }

    pub fn lock_of(&self, project_id: i64, path: &str) -> Option<Lock> {
        self.c()
            .query_row(
                "SELECT l.path, l.user_id, u.name, u.color, l.acquired_at, l.heartbeat_at FROM locks l JOIN users u ON u.id = l.user_id WHERE l.project_id = ?1 AND l.path = ?2",
                params![project_id, path],
                |r| Ok(Lock { path: r.get(0)?, user_id: r.get(1)?, user_name: r.get(2)?, color: r.get(3)?, acquired_at: r.get(4)?, heartbeat_at: r.get(5)? }),
            )
            .optional()
            .ok()
            .flatten()
    }

    pub fn locks(&self, project_id: i64) -> Vec<Lock> {
        let c = self.c();
        let mut st = match c.prepare(
            "SELECT l.path, l.user_id, u.name, u.color, l.acquired_at, l.heartbeat_at FROM locks l JOIN users u ON u.id = l.user_id WHERE l.project_id = ?1 ORDER BY l.path",
        ) {
            Ok(s) => s,
            Err(_) => return vec![],
        };
        st.query_map([project_id], |r| Ok(Lock { path: r.get(0)?, user_id: r.get(1)?, user_name: r.get(2)?, color: r.get(3)?, acquired_at: r.get(4)?, heartbeat_at: r.get(5)? }))
            .map(|r| r.filter_map(Result::ok).collect())
            .unwrap_or_default()
    }

    pub fn release_lock(&self, project_id: i64, path: &str, user_id: Option<i64>) -> rusqlite::Result<usize> {
        match user_id {
            Some(u) => self.c().execute("DELETE FROM locks WHERE project_id = ?1 AND path = ?2 AND user_id = ?3", params![project_id, path, u]),
            None => self.c().execute("DELETE FROM locks WHERE project_id = ?1 AND path = ?2", params![project_id, path]),
        }
    }

    /// Free every lock whose holder stopped sending heartbeats. Returns (project, path) freed.
    pub fn expire_locks(&self, stale_secs: i64) -> Vec<(i64, String)> {
        let c = self.c();
        let cutoff = now() - stale_secs;
        let freed: Vec<(i64, String)> = match c.prepare("SELECT project_id, path FROM locks WHERE heartbeat_at < ?1") {
            Ok(mut st) => st.query_map([cutoff], |r| Ok((r.get(0)?, r.get(1)?))).map(|r| r.filter_map(Result::ok).collect()).unwrap_or_default(),
            Err(_) => vec![],
        };
        let _ = c.execute("DELETE FROM locks WHERE heartbeat_at < ?1", [cutoff]);
        freed
    }

    // ---- comments ----------------------------------------------------------------------

    pub fn add_comment(&self, project_id: i64, path: &str, user_id: i64, body: &str) -> rusqlite::Result<i64> {
        let c = self.c();
        c.execute("INSERT INTO comments (project_id, path, user_id, body, created_at) VALUES (?1, ?2, ?3, ?4, ?5)", params![project_id, path, user_id, body, now()])?;
        Ok(c.last_insert_rowid())
    }

    pub fn comments(&self, project_id: i64, path: &str) -> Vec<Comment> {
        let c = self.c();
        let mut st = match c.prepare(
            "SELECT c.id, c.path, c.user_id, u.name, u.color, c.body, c.created_at FROM comments c JOIN users u ON u.id = c.user_id
             WHERE c.project_id = ?1 AND c.path = ?2 ORDER BY c.created_at",
        ) {
            Ok(s) => s,
            Err(_) => return vec![],
        };
        st.query_map(params![project_id, path], |r| {
            Ok(Comment { id: r.get(0)?, path: r.get(1)?, user_id: r.get(2)?, user_name: r.get(3)?, color: r.get(4)?, body: r.get(5)?, created_at: r.get(6)? })
        })
        .map(|r| r.filter_map(Result::ok).collect())
        .unwrap_or_default()
    }
}
