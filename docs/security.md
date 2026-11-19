# Security

## Rule

Nothing is published (pushed, released, offered upstream) until the tests pass **and** a
security review of the branch is clean. A branch that touches a **security-sensitive file**
(below) must add a line to the review log, even when the change looks harmless.

Security-sensitive files: `src/auth.rs`, `src/api.rs` (permission checks, sign-in, uploads),
`src/live.rs`, `src/files.rs`, `src/convert.rs`, `src/lib.rs` (headers, CSP, body limits, CSRF),
`static/js/index.js` (redirects), `static/common.js`, anything that sets `innerHTML`.

## Threat model (short)

- **Who:** strangers on the internet (sign-up, sign-in), signed-in members of other teams,
  members with fewer rights (viewers), a member uploading hostile CAD files, sibling sites on the
  same registrable domain sharing cookies.
- **What we protect:** project files and history, who is in which team, account emails, the host
  the server runs on (other services, SSH keys, tunnel credentials).
- **Assumptions:** served over HTTPS behind a proxy or tunnel; bound to loopback; `git` and a
  user systemd instance on the host.

## Controls (F-C10)

| Control | Where | Test |
|---|---|---|
| argon2id passwords; hashing off the async threads, max 2 at once, 10 s wait then 503 | `auth.rs` | `repeated_wrong_passwords_pause_sign_in` |
| Sign-in limits: failures per address, attempts per email; sign-up per address; IPv6 by /64 | `auth.rs`, `api.rs` | `signup_and_sign_in_are_limited_per_address` |
| Same reply and same work for taken and free emails at sign-up | `api.rs` signup | `signup_does_not_reveal_existing_accounts` |
| One-time password users reach only password change | `auth.rs` CurrentUser | `one_time_password_only_reaches_password_change` |
| `__Host-` session cookie, HttpOnly, SameSite=Lax, Secure; token stored as SHA-256 | `auth.rs` | `pages_carry_security_headers_and_secure_cookie_is_host_prefixed` |
| CSRF: every non-GET `/api` call needs `X-Collab` | `lib.rs` csrf_guard | `state_changes_need_the_csrf_header` |
| CSP `script-src 'self'` on pages; app CSP pins SolidCraft's inline scripts by hash; HSTS | `lib.rs` | `pages_carry_security_headers_…` |
| Safe redirect after sign-in (same origin only) | `static/js/index.js` safeNext | manual |
| Team isolation (404), role checks, owners can't add outsiders, emails only to owners | `api.rs` | `teams_cannot_see_each_other`, `team_owner_cannot_pull_in_outsiders_…` |
| Paths: relative, plain names, no dot-files, no `.git`, no hidden characters; literal git pathspecs; commits scoped to paths | `files.rs` | `unsafe_file_names_are_refused`, `names_and_paths_refuse_hidden_characters`, `concurrent_saves_…` |
| Body limits: 64 KiB JSON, `COLLAB_MAX_BODY_MB` for files; project quota | `lib.rs`, `api.rs` | `huge_json_bodies_are_refused_before_use`, `uploads_larger_than_two_megabytes_…` |
| New files created exclusively (no silent overwrite) | `api.rs` write_file | `two_new_files_with_one_name_never_overwrite` |
| WebSocket: origin check, 8 KiB messages, 5/s, access re-checked every 30 s, "editing" only for the lock holder | `live.rs` | `live_channel_requires_this_sites_origin` |
| CAD conversion: one at a time, bounded queue, background; `systemd-run --user` with no network, no Unix sockets, `/run/user` hidden, memory cap without swap, one CPU, time limit, syscall filter, own user namespace; output validated, never overwrites | `convert.rs` | `sandboxed_converter_cannot_reach_user_systemd` (`COLLAB_TEST_SANDBOX=1`), `uploads_return_at_once_…` |

## Review log

| Date | Scope | Reviewer | Result |
|---|---|---|---|
| 2026-11-08 | Round 1: all of v0.1 + Team fork diff | security agent | Not ready. C1 Critical (`?next=` XSS/open redirect), H1 argon2 DoS / no signup limit, H2 unsandboxed hostile-file parsing, M1 one-time password client-side only, M2 WS origin + no re-check, M3 WS message size, M4 owners could add any user, M5 no CSP/HSTS, M6 signup enumeration, M7 git races + glob pathspecs, L1–L6 (cookie prefix, logout CSRF, create race, hidden chars, insecure default, quota). |
| 2026-11-08 | Round 2: fixes for round 1 | security agent | Not yet. All round-1 findings fixed. New: N1 High sandbox escape via user D-Bus (**confirmed on the host**), N2 large JSON bodies, N3 conversion held project lock + memory, N4 shared rate-limit bucket; Lows. |
| 2026-11-08 | Round 3: fixes for round 2 | security agent | **Ready to share (security).** N1–N4 fixed and tested; last Lows (IP limit counting successes, queue slot leak on panic) fixed afterwards; README security notes added. 23 tests pass. |

Add a row for every review. Keep finding IDs stable; reference them in commits and the changelog.
