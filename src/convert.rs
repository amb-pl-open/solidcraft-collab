//! Turn an uploaded CAD file into a SolidCraft design next to it, so it opens in the browser.
//!
//! The CAD readers parse untrusted, proprietary binaries, so each conversion:
//! - runs one at a time (a semaphore in AppState);
//! - works on a copy in its own temporary job folder, never inside the project;
//! - with sandbox "systemd", runs as a transient `systemd-run --user` unit with a memory cap and
//!   no swap, one CPU, a task limit, a time limit, no network, read-only system, an empty home
//!   apart from the job folder, no access to the user's systemd/D-Bus sockets, no Unix sockets,
//!   a system-call filter and its own user namespace;
//! - is accepted only if it produced a JSON design, which is then moved into the project under a
//!   name no other file has (never overwriting).

use std::path::{Path, PathBuf};
use std::time::Duration;

use tokio::process::Command;

use crate::config::Config;

const TIMEOUT_SECS: u64 = 300;

/// Pick a new design name next to `rel` and reserve it (created empty, exclusively) so a
/// concurrent upload can't take the same name. Returns the relative name.
fn reserve_design_name(dir: &Path, rel: &str) -> std::io::Result<String> {
    let stem = rel.rsplit_once('.').map(|(s, _)| s).unwrap_or(rel);
    for n in 1..1000 {
        let name = if n == 1 { format!("{stem}.json") } else { format!("{stem} ({n}).json") };
        match std::fs::OpenOptions::new().write(true).create_new(true).open(dir.join(&name)) {
            Ok(_) => return Ok(name),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e),
        }
    }
    Err(std::io::Error::other("too many designs with this name"))
}

fn job_dir(cfg: &Config) -> std::io::Result<PathBuf> {
    let mut b = [0u8; 8];
    rand::RngCore::fill_bytes(&mut rand::thread_rng(), &mut b);
    let d = cfg.data_dir.join("jobs").join(hex::encode(b));
    std::fs::create_dir_all(&d)?;
    Ok(d)
}

fn command(cfg: &Config, cli: &Path, job: &Path, input: &Path, output: &Path) -> Command {
    // Bind the real binary (the configured path may be a `current/...` symlink chain).
    let real = std::fs::canonicalize(cli).unwrap_or_else(|_| cli.to_path_buf());
    let cli = real.as_path();
    let mut cmd = if cfg.convert_sandbox == "systemd" {
        let mut c = Command::new("systemd-run");
        c.args(["--user", "--wait", "--pipe", "--quiet", "--collect"])
            .arg(format!("--property=MemoryMax={}M", cfg.convert_memory_mb))
            .args([
                "--property=MemorySwapMax=0",
                "--property=CPUQuota=100%",
                "--property=TasksMax=64",
                &format!("--property=RuntimeMaxSec={TIMEOUT_SECS}"),
                "--property=PrivateNetwork=yes",
                "--property=ProtectSystem=strict",
                "--property=ProtectHome=tmpfs",
                "--property=NoNewPrivileges=yes",
                "--property=PrivateTmp=yes",
                "--property=Nice=10",
                // No way back out: the user's systemd and D-Bus sockets live under /run/user,
                // and only internet sockets (useless in the private network) may be opened, so
                // a decoder exploit can't ask systemd to start anything outside the sandbox.
                "--property=InaccessiblePaths=/run/user",
                "--property=RestrictAddressFamilies=AF_INET",
                "--property=SystemCallFilter=@system-service",
                "--property=ProtectProc=invisible",
                "--property=LockPersonality=yes",
                "--property=RestrictNamespaces=yes",
                "--property=PrivateIPC=yes",
                "--property=PrivateUsers=yes",
            ])
            .arg(format!("--property=BindReadOnlyPaths={}", cli.display()))
            .arg(format!("--property=BindPaths={}", job.display()))
            .arg(format!("--working-directory={}", job.display()))
            .arg("--")
            .arg(cli);
        c
    } else {
        let mut c = Command::new("nice");
        c.args(["-n", "10"]).arg(cli).current_dir(job);
        c
    };
    cmd.arg("run").arg("--in").arg(input).arg("--out").arg(output).arg("--quiet").kill_on_drop(true);
    cmd
}

/// Convert `rel` (already saved in project folder `dir`) into a design; returns its name.
pub async fn cad_to_design(cfg: &Config, cli: &Path, dir: &Path, rel: &str) -> anyhow::Result<String> {
    let job = job_dir(cfg)?;
    let result = run(cfg, cli, dir, rel, &job).await;
    let _ = tokio::fs::remove_dir_all(&job).await;
    result
}

async fn run(cfg: &Config, cli: &Path, dir: &Path, rel: &str, job: &Path) -> anyhow::Result<String> {
    let file_name = Path::new(rel).file_name().and_then(|f| f.to_str()).ok_or_else(|| anyhow::anyhow!("bad name"))?;
    let input = job.join(file_name);
    tokio::fs::copy(dir.join(rel), &input).await?;
    let output = job.join("design.json");
    let out = tokio::time::timeout(Duration::from_secs(TIMEOUT_SECS + 10), command(cfg, cli, job, &input, &output).output())
        .await
        .map_err(|_| anyhow::anyhow!("took longer than {} minutes", TIMEOUT_SECS / 60))??;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        anyhow::bail!("{}", err.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or("conversion failed").trim());
    }
    let meta = tokio::fs::metadata(&output).await.map_err(|_| anyhow::anyhow!("no design was produced"))?;
    if meta.len() == 0 || meta.len() > cfg.max_body as u64 {
        anyhow::bail!("the design produced was empty or too large");
    }
    let bytes = tokio::fs::read(&output).await?;
    let v: serde_json::Value = serde_json::from_slice(&bytes).map_err(|_| anyhow::anyhow!("the design produced isn't valid JSON"))?;
    if !v.get("format").and_then(|f| f.as_str()).is_some_and(|f| f.starts_with("solidcraft/")) {
        anyhow::bail!("the file produced isn't a SolidCraft design");
    }
    let dir2 = dir.to_path_buf();
    let rel2 = rel.to_string();
    let name = tokio::task::spawn_blocking(move || reserve_design_name(&dir2, &rel2)).await??;
    // Write next to the reserved name, then rename over it (the reservation is ours).
    let tmp = dir.join(format!(".tmp-{}", hex::encode(rand::random::<[u8; 6]>())));
    let placed = async {
        tokio::fs::write(&tmp, &bytes).await?;
        tokio::fs::rename(&tmp, dir.join(&name)).await
    }
    .await;
    if let Err(e) = placed {
        let _ = tokio::fs::remove_file(&tmp).await;
        let _ = tokio::fs::remove_file(dir.join(&name)).await;
        return Err(e.into());
    }
    Ok(name)
}

/// Queue a background conversion of an uploaded CAD file; the design is committed as its own
/// version when ready, and everyone in the project is told. Returns false when the queue is
/// full (the upload stays saved; it can be uploaded again later to retry).
pub fn queue_job(st: &crate::AppState, project: i64, rel: String, by: crate::db::User) -> bool {
    use std::sync::atomic::Ordering;
    let Some(cli) = st.cfg.cli.clone() else { return false };
    if st.convert_pending.fetch_add(1, Ordering::SeqCst) >= crate::CONVERT_QUEUE_MAX {
        st.convert_pending.fetch_sub(1, Ordering::SeqCst);
        return false;
    }
    let st = st.clone();
    tokio::spawn(async move {
        // Give the queue slot back however this task ends (even on a panic).
        struct Slot(std::sync::Arc<std::sync::atomic::AtomicUsize>);
        impl Drop for Slot {
            fn drop(&mut self) {
                self.0.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
            }
        }
        let _slot_back = Slot(st.convert_pending.clone());
        let dir = crate::files::project_dir(&st.cfg.projects_dir(), project);
        let made = match st.convert_slot.clone().acquire_owned().await {
            Ok(_slot) => cad_to_design(&st.cfg, &cli, &dir, &rel).await,
            Err(_) => Err(anyhow::anyhow!("conversion queue closed")),
        };
        match made {
            Ok(design) => {
                let guard = st.project_lock(project);
                let _serial = guard.lock().await;
                match crate::files::commit(&dir, &[&design], &format!("Design made from {rel}"), &by.name, &by.email).await {
                    Ok(version) => st.hub.publish(
                        project,
                        serde_json::json!({"type": "saved", "path": design, "version": version, "made_from": rel, "by": {"id": by.id, "name": by.name, "color": by.color}}),
                    ),
                    Err(e) => tracing::warn!("could not record the design made from {rel}: {e}"),
                }
            }
            Err(e) => {
                tracing::warn!("could not make a design from {rel}: {e}");
                st.hub.publish(project, serde_json::json!({"type": "convert_failed", "path": rel, "by": {"id": by.id, "name": by.name, "color": by.color}}));
            }
        }
    });
    true
}
