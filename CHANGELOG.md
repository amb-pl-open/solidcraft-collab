# Changelog

All notable changes, newest first. Format: [Keep a Changelog](https://keepachangelog.com/en/1.1.0/);
versions follow [SemVer](https://semver.org/). Every entry names the feature IDs from
[docs/features.md](docs/features.md) and, for security fixes, the finding IDs from
[docs/security.md](docs/security.md).

Sections per release: **Added**, **Changed**, **Fixed**, **Security**, **Removed**,
**Deprecated**. A branch adds its lines under **Unreleased**; the release step moves them under
a version heading.

## [Unreleased]

### Fixed
- The editor waits up to 10 minutes for SolidCraft to start, with a note after 15 s, instead of reporting an unsupported browser after 60 s while the first download is still running on a slow link (F-C08).
- `.sc` design files are no longer offered to the browser editor, which can only import JSON designs; they are now turned into a `.json` design by the configured `solidcraft-cli`, like other CAD uploads (F-C08, F-C09).
- Panels marked hidden (file history, share form) no longer show as empty boxes; the `hidden` attribute now wins over layout rules (F-C04, F-C07).

## [0.1.0] - 2026-11-08 (not yet published)

### Added
- Admin-approved accounts with any email, one-time passwords and forced change at first sign-in (F-C01).
- Teams with owner, editor and viewer roles; other teams' projects answer 404 (F-C02).
- Personal and joint projects, shareable with named teammates (F-C03).
- One git repository per project: every save is a version with its author; history and restore (F-C04).
- Per-part edit locks with heartbeat, idle expiry after 180 s, and force-free for owners (F-C05).
- Live presence over WebSocket: who is where, editing or viewing, and what they are on (F-C06).
- Comments per file (F-C07).
- Editor page embedding the stock SolidCraft browser build (F-C08).
- CAD uploads turned into designs by a configured `solidcraft-cli`, in a background queue (F-C09).

### Security
- Reviewed in three rounds before any release; every finding fixed with a test (F-C10). See
  docs/security.md for findings C1, H1–H2, M1–M7, L1–L6, N1–N4.
