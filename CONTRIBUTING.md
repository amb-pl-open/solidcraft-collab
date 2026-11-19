# Contributing

## Branches

`<type>/<feature-id>-<short-slug>`, for example `feat/F-C11-desktop-open-save`,
`fix/F-C05-lock-heartbeat`, `sec/N5-ws-replay`, `docs/F-C06-presence-guide`.
Types: `feat`, `fix`, `sec`, `docs`, `refactor`, `test`, `chore`. A branch for a new feature first
adds its row (status *in progress*) to [docs/features.md](docs/features.md).

## Commits

[Conventional Commits](https://www.conventionalcommits.org/) with the feature ID as scope and a
`Feature:` trailer (several allowed). Security fixes also carry `Finding:` with the ID from
[docs/security.md](docs/security.md).

```
feat(F-C06): show the feature an editor is working on

Reads ui.inspect / ui.selection every 2 s and sends it with presence.

Feature: F-C06
```

## Documentation that every branch updates

The pre-push hook and `scripts/check-docs.sh` enforce the first three; reviewers check the rest.

1. **CHANGELOG.md, under [Unreleased]**, one line per user-visible change, naming the feature IDs.
2. **docs/features.md**, the row of every feature the branch touches (status, code, tests, docs).
3. **docs/security.md**, a review-log row if a security-sensitive file changed.
4. **docs/user-guide.md**, the section of any feature whose behaviour people see changed.
5. **docs/decisions/**, a new record when the branch makes or reverses a design choice.
6. **docs/architecture.md** when modules or data flow change; **README.md** when settings change.

Which files require which docs is listed in [docs/doc-map.tsv](docs/doc-map.tsv).

## Before anything is shared

1. `cargo test` passes; on a host with user systemd also `COLLAB_TEST_SANDBOX=1 cargo test`.
2. Every changed behaviour has a test (name it in the feature row).
3. A **security review** of the branch is clean and logged in docs/security.md.
4. The maintainer approves the push. Nothing is pushed or released before that.

## Setup

```sh
git config core.hooksPath .githooks     # once per clone: enables the docs check on push
scripts/check-docs.sh main              # run it by hand against any base branch
```

## Licence

Contributions are MIT OR Apache-2.0, like the project.
