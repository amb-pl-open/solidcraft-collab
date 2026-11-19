//! Settings, all from environment variables so a deployment is just an env file.

use std::path::PathBuf;

#[derive(Clone, Debug)]
pub struct Config {
    /// Where the database and project folders live.
    pub data_dir: PathBuf,
    /// Address to listen on. Keep it on loopback and put a tunnel or proxy in front.
    pub bind: String,
    /// A SolidCraft browser build (the folder with index.html), served at /app/.
    pub app_dir: Option<PathBuf>,
    /// A solidcraft-cli binary; when set, uploaded CAD files (STEP, SolidWorks, ...) are
    /// turned into designs that open in the browser.
    pub cli: Option<PathBuf>,
    /// How the CLI runs: "systemd" (a transient `systemd-run --user` unit with memory, CPU,
    /// time and network limits; recommended) or "none" (a plain child process).
    pub convert_sandbox: String,
    /// Memory cap for one conversion, in MB.
    pub convert_memory_mb: u64,
    /// Name shown in the page header.
    pub site_name: String,
    /// The public address people use (e.g. https://team.example.com). Live connections from
    /// any other origin are refused. When unset, the request's own Host must match the Origin.
    pub public_origin: Option<String>,
    /// Mark cookies Secure and use the __Host- prefix (default; turn off only for plain-HTTP
    /// local testing).
    pub secure_cookies: bool,
    /// Trust the CF-Connecting-IP header for per-address rate limits. Only safe when the server
    /// is reachable solely through Cloudflare (bound to loopback behind a tunnel).
    pub trust_cf_ip: bool,
    /// Seconds without a heartbeat before an edit lock frees itself.
    pub lock_stale_secs: i64,
    /// Largest upload or save, in bytes.
    pub max_body: usize,
    /// Largest project folder (including history), in bytes.
    pub project_quota: u64,
    /// Content-Security-Policy on the embedded SolidCraft app: "strict" (default) or "off".
    pub app_csp: bool,
}

impl Config {
    pub fn from_env() -> Self {
        let get = |k: &str| std::env::var(k).ok().filter(|v| !v.trim().is_empty());
        let flag = |k: &str, d: bool| get(k).map(|v| v == "1" || v.eq_ignore_ascii_case("true") || v.eq_ignore_ascii_case("yes")).unwrap_or(d);
        Config {
            data_dir: get("COLLAB_DATA_DIR").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("./collab-data")),
            bind: get("COLLAB_BIND").unwrap_or_else(|| "127.0.0.1:8710".into()),
            app_dir: get("COLLAB_APP_DIR").map(PathBuf::from),
            cli: get("COLLAB_CLI").map(PathBuf::from),
            convert_sandbox: get("COLLAB_CONVERT_SANDBOX").unwrap_or_else(|| "systemd".into()),
            convert_memory_mb: get("COLLAB_CONVERT_MEMORY_MB").and_then(|v| v.parse().ok()).unwrap_or(1500),
            site_name: get("COLLAB_SITE_NAME").unwrap_or_else(|| "SolidCraft Collab".into()),
            public_origin: get("COLLAB_PUBLIC_ORIGIN").map(|o| o.trim_end_matches('/').to_string()),
            secure_cookies: flag("COLLAB_SECURE_COOKIES", true),
            trust_cf_ip: flag("COLLAB_TRUST_CF_IP", false),
            lock_stale_secs: get("COLLAB_LOCK_STALE_SECS").and_then(|v| v.parse().ok()).unwrap_or(180),
            max_body: get("COLLAB_MAX_BODY_MB").and_then(|v| v.parse::<usize>().ok()).unwrap_or(64) * 1024 * 1024,
            project_quota: get("COLLAB_PROJECT_QUOTA_MB").and_then(|v| v.parse::<u64>().ok()).unwrap_or(2048) * 1024 * 1024,
            app_csp: get("COLLAB_APP_CSP").map(|v| v != "off").unwrap_or(true),
        }
    }

    pub fn projects_dir(&self) -> PathBuf {
        self.data_dir.join("projects")
    }

    pub fn db_path(&self) -> PathBuf {
        self.data_dir.join("collab.sqlite")
    }
}
