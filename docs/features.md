# Feature register

Every feature has an ID. Branches, commits and changelog entries name the IDs they touch
(see [CONTRIBUTING.md](../CONTRIBUTING.md)). When a branch changes a feature, update its row here
in the same branch: status, code, tests, docs. A new feature gets the next free ID.

Status: **planned** (agreed, not started) · **in progress** · **done** (merged, tested) ·
**changed** (behaviour changed this release; see CHANGELOG) · **retired**.

| ID | Feature | Status | Code | Tests | User docs |
|---|---|---|---|---|---|
| F-C01 | Admin-approved accounts: sign up with any email; admin approves into a team with a role; one-time passwords; must change at first sign-in | done | `src/auth.rs`, `api.rs` (signup, login, admin_*) | `signup_needs_admin_approval`, `one_time_password_only_reaches_password_change`, `weak_input_is_refused` | [user-guide § Accounts](user-guide.md#accounts) |
| F-C02 | Teams and roles (owner, editor, viewer); team isolation (other teams get 404) | done | `api.rs` (teams, `can_view/can_edit/can_manage`), `db.rs` | `teams_cannot_see_each_other`, `non_admin_cannot_use_admin_routes`, `team_owner_cannot_pull_in_outsiders_or_see_emails_as_editor` | [user-guide § Teams](user-guide.md#teams-and-roles) |
| F-C03 | Personal and joint projects; sharing with named teammates | done | `api.rs` (projects, members), `db.rs` | `personal_projects_are_private_until_shared` | [user-guide § Projects](user-guide.md#projects) |
| F-C04 | Versioned files: one git repository per project; every save is a version with its author; history; restore | done | `src/files.rs`, `api.rs` (write_file, history, restore) | `every_save_is_a_version_that_can_be_restored`, `concurrent_saves_all_commit_and_names_are_literal` | [user-guide § Versions](user-guide.md#versions) |
| F-C05 | Per-part edit locks: different people edit different parts at once; heartbeat; idle locks free themselves; owners can force-free | done | `db.rs` (locks), `api.rs` (lock, unlock, force_unlock), `lib.rs` (reaper) | `locks_let_people_edit_different_parts_at_once` | [user-guide § Working together](user-guide.md#working-together) |
| F-C06 | Live presence (Canva/Docs style): who is in which file, editing or viewing, what they are working on; live save/comment events | done | `src/live.rs`, `static/js/project.js`, `static/js/edit.js` | `live_channel_requires_this_sites_origin` (+ manual two-browser test, pending) | [user-guide § Working together](user-guide.md#working-together) |
| F-C07 | Comments per file | done | `api.rs` (comments), `static/js/project.js` | — (add one) | [user-guide § Versions](user-guide.md#versions) |
| F-C08 | Embedded SolidCraft editor (stock browser build in `?embed=editor`, same origin; open/save via `document.import/export/saved`) | done (browser test pending) | `static/edit.html`, `static/js/edit.js`, `lib.rs` (`/app` + CSP) | manual | [user-guide § Editing](user-guide.md#editing-a-design) |
| F-C09 | CAD upload becomes a design (STEP/IGES, or any format the configured `solidcraft-cli` reads); background queue; sandboxed | done | `src/convert.rs`, `api.rs` (write_file) | `uploads_return_at_once_while_a_slow_conversion_runs`, `sandboxed_converter_cannot_reach_user_systemd` | [user-guide § Uploading CAD files](user-guide.md#uploading-cad-files) |
| F-C10 | Security controls: CSRF header, CSP/HSTS, `__Host-` cookie, rate limits, body limits, quotas, hidden-character checks | done | `lib.rs`, `auth.rs`, `api.rs` | the security tests listed in [security.md](security.md) | [security.md](security.md) |
| F-C11 | Desktop SolidCraft opens/saves team projects (same locks and presence) | planned (Phase 3) | — | — | — |
| F-C12 | Two people editing inside the same part at once (server-ordered commands) | planned (Phase 5) | — | — | — |
| F-C13 | Assemblies whose linked part files open with them in the editor | planned | — | — | — |
| F-C14 | Activity feed persistence and notifications | planned | — | — | — |

## Not features of this add-on

Opening commercial CAD formats is not part of solidcraft-collab: it uses whatever the configured
`solidcraft-cli` can read. Deployment specifics (domain, branding, service users, backups) belong
to each deployment, not to this repository.
