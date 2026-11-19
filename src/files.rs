//! Project folders: one git repository per project, so every save is a version with an
//! author, and any version can be read back or restored.

use std::path::{Component, Path, PathBuf};

use serde::Serialize;
use tokio::process::Command;

/// Check a path inside a project: relative, plain names, no `..`, nothing under `.git`.
pub fn clean_path(p: &str) -> Option<String> {
    let p = p.trim();
    if p.starts_with('/') {
        return None;
    }
    let p = p.trim_end_matches('/');
    if p.is_empty() || p.len() > 400 || p.contains('\\') || p.chars().any(|c| c.is_control()) {
        return None;
    }
    let mut parts = Vec::new();
    for c in Path::new(p).components() {
        match c {
            Component::Normal(s) => {
                let s = s.to_str()?;
                if s.starts_with('.') || s.len() > 120 {
                    return None;
                }
                if !s.chars().all(|ch| ch.is_alphanumeric() || " _-.+()[],&@".contains(ch)) {
                    return None;
                }
                parts.push(s.to_string());
            }
            _ => return None,
        }
    }
    (!parts.is_empty()).then(|| parts.join("/"))
}

pub fn project_dir(root: &Path, project_id: i64) -> PathBuf {
    root.join(format!("p{project_id}"))
}

async fn git(dir: &Path, args: &[&str]) -> anyhow::Result<String> {
    let out = Command::new("git").arg("-C").arg(dir).args(args).env("GIT_TERMINAL_PROMPT", "0").env("GIT_LITERAL_PATHSPECS", "1").output().await?;
    if !out.status.success() {
        anyhow::bail!("git {}: {}", args.first().unwrap_or(&""), String::from_utf8_lossy(&out.stderr).trim());
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

pub async fn init_project(dir: &Path) -> anyhow::Result<()> {
    tokio::fs::create_dir_all(dir).await?;
    if !dir.join(".git").exists() {
        git(dir, &["init", "-q", "-b", "main"]).await?;
        git(dir, &["config", "commit.gpgsign", "false"]).await?;
        tokio::fs::write(dir.join(".gitattributes"), "*.json -diff\n*.sc binary\n*.sldprt binary\n*.SLDPRT binary\n").await?;
        commit(dir, &[".gitattributes"], "Project created", "collab", "collab@localhost").await?;
    }
    Ok(())
}

/// Commit the given paths as one version. Returns the new version id (short hash).
pub async fn commit(dir: &Path, paths: &[&str], message: &str, author: &str, email: &str) -> anyhow::Result<String> {
    let mut add = vec!["add", "--"];
    add.extend_from_slice(paths);
    git(dir, &add).await?;
    let ident_name = format!("user.name={author}");
    let ident_email = format!("user.email={email}");
    let mut st = vec!["status", "--porcelain", "--"];
    st.extend_from_slice(paths);
    let status = git(dir, &st).await?;
    if status.trim().is_empty() {
        return Ok(git(dir, &["rev-parse", "--short", "HEAD"]).await?.trim().to_string());
    }
    // Commit only these paths, so another save's staged files never ride along.
    let mut c = vec!["-c", ident_name.as_str(), "-c", ident_email.as_str(), "commit", "-q", "-m", message, "--"];
    c.extend_from_slice(paths);
    git(dir, &c).await?;
    Ok(git(dir, &["rev-parse", "--short", "HEAD"]).await?.trim().to_string())
}

#[derive(Debug, Serialize)]
pub struct FileEntry {
    pub path: String,
    pub size: u64,
    pub modified: i64,
    pub kind: &'static str,
}

/// What a file is, for the UI: a SolidCraft design (`.json`) opens in the editor; a CAD file has
/// a design made from it when the server has a solidcraft-cli. A `.sc` file is SolidCraft's zip
/// container, which the browser API cannot import as text, so it is treated like a CAD file.
pub fn kind_of(path: &str) -> &'static str {
    let ext = path.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase()).unwrap_or_default();
    match ext.as_str() {
        "json" => "design",
        "sc" | "step" | "stp" | "igs" | "iges" | "sldprt" | "f3d" | "f3z" | "ipt" | "iam" | "catpart" | "prt" | "sat" | "sab" | "smt" | "smb" | "pvz" | "fcstd" | "3dm" => "cad",
        "stl" | "3mf" | "obj" | "glb" | "gltf" => "mesh",
        "dxf" | "svg" | "pdf" | "png" | "jpg" | "jpeg" => "document",
        _ => "other",
    }
}

pub fn list_files(dir: &Path) -> Vec<FileEntry> {
    fn walk(base: &Path, d: &Path, out: &mut Vec<FileEntry>) {
        let Ok(rd) = std::fs::read_dir(d) else { return };
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            if name.starts_with('.') {
                continue;
            }
            let p = e.path();
            if p.is_dir() {
                walk(base, &p, out);
            } else if let (Ok(meta), Ok(rel)) = (e.metadata(), p.strip_prefix(base)) {
                let rel = rel.to_string_lossy().replace('\\', "/");
                let modified = meta.modified().ok().and_then(|m| m.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_secs() as i64).unwrap_or(0);
                out.push(FileEntry { kind: kind_of(&rel), path: rel, size: meta.len(), modified });
            }
        }
    }
    let mut out = Vec::new();
    walk(dir, dir, &mut out);
    out.sort_by(|a, b| a.path.to_lowercase().cmp(&b.path.to_lowercase()));
    out
}

#[derive(Debug, Serialize)]
pub struct Version {
    pub id: String,
    pub author: String,
    pub at: i64,
    pub message: String,
}

pub async fn history(dir: &Path, path: Option<&str>, limit: usize) -> anyhow::Result<Vec<Version>> {
    let n = format!("-n{}", limit.clamp(1, 500));
    let mut args = vec!["log", n.as_str(), "--format=%h%x1f%an%x1f%at%x1f%s%x1e"];
    if let Some(p) = path {
        args.push("--");
        args.push(p);
    }
    let out = git(dir, &args).await?;
    Ok(out
        .split('\u{1e}')
        .filter_map(|rec| {
            let f: Vec<&str> = rec.trim().split('\u{1f}').collect();
            (f.len() == 4).then(|| Version { id: f[0].into(), author: f[1].into(), at: f[2].parse().unwrap_or(0), message: f[3].into() })
        })
        .collect())
}

/// A file's bytes at an older version.
pub async fn read_at(dir: &Path, path: &str, version: &str) -> anyhow::Result<Vec<u8>> {
    if !version.chars().all(|c| c.is_ascii_hexdigit()) || version.len() < 4 || version.len() > 40 {
        anyhow::bail!("bad version id");
    }
    let spec = format!("{version}:{path}");
    let out = Command::new("git").arg("-C").arg(dir).args(["show", &spec]).env("GIT_LITERAL_PATHSPECS", "1").output().await?;
    if !out.status.success() {
        anyhow::bail!("no such version of this file");
    }
    Ok(out.stdout)
}

pub async fn current_version(dir: &Path) -> String {
    git(dir, &["rev-parse", "--short", "HEAD"]).await.map(|s| s.trim().to_string()).unwrap_or_default()
}

/// Bytes a project folder takes on disk, history included.
pub fn dir_size(dir: &Path) -> u64 {
    fn walk(d: &Path) -> u64 {
        let Ok(rd) = std::fs::read_dir(d) else { return 0 };
        rd.flatten()
            .map(|e| match e.file_type() {
                Ok(t) if t.is_dir() => walk(&e.path()),
                Ok(t) if t.is_file() => e.metadata().map(|m| m.len()).unwrap_or(0),
                _ => 0,
            })
            .sum()
    }
    walk(dir)
}
