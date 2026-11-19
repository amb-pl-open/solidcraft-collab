use std::io::{BufRead, Write};

use solidcraft_collab::{app, auth, config::Config, spawn_lock_reaper, AppState};

const USAGE: &str = "usage:
  solidcraft-collab serve                      run the server (settings from COLLAB_* environment variables)
  solidcraft-collab create-admin EMAIL NAME    make an approved admin account; reads the password from stdin
  solidcraft-collab create-admin EMAIL NAME --temp-password   same, printing a one-time password to change at first sign-in
  solidcraft-collab create-team NAME           make a team
  solidcraft-collab add-member TEAM_ID EMAIL ROLE   put an approved person in a team (owner, editor, viewer)";

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt().with_env_filter(tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info,tower_http=warn".into())).init();
    let args: Vec<String> = std::env::args().skip(1).collect();
    let cfg = Config::from_env();
    match args.first().map(String::as_str) {
        Some("serve") => serve(cfg).await,
        Some("create-admin") if args.len() == 4 && args[3] == "--temp-password" => {
            let st = AppState::new(cfg)?;
            if !auth::email_ok(&args[1]) {
                anyhow::bail!("not a valid email");
            }
            let pw = auth::temp_password();
            let id = st.db.create_user(&args[1], &args[2], &auth::hash_password(&pw)?, "active", true)?;
            st.db.set_password(id, &auth::hash_password(&pw)?, true)?;
            st.db.audit(None, "create-admin", &args[1]);
            println!("admin created, id {id}\none-time password: {pw}\n(sign in with it; you'll be asked to choose your own)");
            Ok(())
        }
        Some("create-admin") if args.len() == 3 => {
            let st = AppState::new(cfg)?;
            eprint!("Password for {} (at least 10 characters): ", args[1]);
            std::io::stderr().flush()?;
            let mut pw = String::new();
            std::io::stdin().lock().read_line(&mut pw)?;
            let pw = pw.trim_end_matches(['\n', '\r']);
            auth::password_ok(pw).map_err(|m| anyhow::anyhow!(m))?;
            if !auth::email_ok(&args[1]) {
                anyhow::bail!("not a valid email");
            }
            let id = st.db.create_user(&args[1], &args[2], &auth::hash_password(pw)?, "active", true)?;
            st.db.audit(None, "create-admin", &args[1]);
            println!("admin created, id {id}");
            Ok(())
        }
        Some("create-team") if args.len() == 2 => {
            let st = AppState::new(cfg)?;
            println!("team created, id {}", st.db.create_team(&args[1])?);
            Ok(())
        }
        Some("add-member") if args.len() == 4 => {
            let st = AppState::new(cfg)?;
            let team: i64 = args[1].parse()?;
            let (u, _) = st.db.user_by_email_with_hash(&args[2]).ok_or_else(|| anyhow::anyhow!("no account with that email"))?;
            if !matches!(args[3].as_str(), "owner" | "editor" | "viewer") {
                anyhow::bail!("role must be owner, editor or viewer");
            }
            st.db.set_membership(team, u.id, &args[3])?;
            println!("{} is now {} in team {team}", u.email, args[3]);
            Ok(())
        }
        _ => {
            eprintln!("{USAGE}");
            std::process::exit(2);
        }
    }
}

async fn serve(cfg: Config) -> anyhow::Result<()> {
    let bind = cfg.bind.clone();
    let state = AppState::new(cfg)?;
    if state.db.admin_count() == 0 {
        tracing::warn!("no admin account yet: run `solidcraft-collab create-admin EMAIL NAME`");
    }
    let loopback = bind.starts_with("127.") || bind.starts_with("[::1]") || bind.starts_with("localhost");
    if loopback && !state.cfg.trust_cf_ip {
        tracing::warn!("bound to loopback without COLLAB_TRUST_CF_IP: behind a proxy every client shares one sign-in rate-limit bucket; set COLLAB_TRUST_CF_IP=true when served only through Cloudflare");
    }
    if !state.cfg.secure_cookies {
        tracing::warn!("COLLAB_SECURE_COOKIES=false: only for plain-HTTP local testing");
    }
    spawn_lock_reaper(state.clone());
    let listener = tokio::net::TcpListener::bind(&bind).await?;
    tracing::info!("listening on http://{bind}");
    axum::serve(listener, app(state).into_make_service_with_connect_info::<std::net::SocketAddr>()).with_graceful_shutdown(async {
        let mut term = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()).expect("signal handler");
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {},
            _ = term.recv() => {},
        }
        tracing::info!("shutting down");
    })
    .await?;
    Ok(())
}
