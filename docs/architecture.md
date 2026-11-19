# Architecture

```
browser ──HTTPS──▶ proxy/tunnel ──▶ solidcraft-collab (127.0.0.1:8710)
                                    ├─ /api/*         axum handlers (api.rs) → SQLite (db.rs)
                                    ├─ /api/…/live    WebSocket presence (live.rs)
                                    ├─ /app/*         stock SolidCraft browser build (same origin)
                                    ├─ /*             pages + scripts (static/)
                                    ├─ projects/pN/   one git repository per project (files.rs)
                                    └─ conversions    systemd-run --user sandbox → solidcraft-cli (convert.rs)
```

| Module | Owns | Features |
|---|---|---|
| `config.rs` | `COLLAB_*` settings | all |
| `db.rs` | users, sessions, teams, memberships, projects, members, locks, comments, audit | F-C01–F-C05, F-C07 |
| `auth.rs` | passwords, cookies, rate limits, `CurrentUser`/`Admin`/`ClientKey` extractors | F-C01, F-C10 |
| `api.rs` | routes and permission checks | F-C01–F-C05, F-C07, F-C09 |
| `files.rs` | path rules, git commits, history, sizes | F-C04 |
| `live.rs` | presence hub, events, origin and access checks | F-C06 |
| `convert.rs` | conversion queue and sandbox | F-C09 |
| `lib.rs` | app state, router, headers, CSP, CSRF, lock reaper | F-C05, F-C08, F-C10 |
| `static/js/edit.js` | editor page: embed, lock, save, presence detail | F-C05, F-C06, F-C08 |
| `static/js/project.js` | project page: files, presence, uploads, history, comments, sharing | F-C03–F-C07, F-C09 |

Decisions behind this shape: [decisions/](decisions/README.md). Security controls:
[security.md](security.md).
