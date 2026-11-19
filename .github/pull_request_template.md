## What and why

<!-- One or two sentences. -->

**Features:** F-C__ <!-- every feature ID this changes; new feature = new row in docs/features.md -->
**Findings:** <!-- security finding IDs fixed, if any -->

## How it was tested

- [ ] `cargo test` passes (paste the summary line)
- [ ] `COLLAB_TEST_SANDBOX=1 cargo test` on a host with user systemd (if conversion changed)
- [ ] New or changed behaviour has a test, named in the feature row
- [ ] Manual check, if UI changed (what you did, browser)

## Documentation (scripts/check-docs.sh must pass)

- [ ] CHANGELOG.md — line(s) under [Unreleased] naming the feature IDs
- [ ] docs/features.md — row(s) updated: status, code, tests, docs
- [ ] docs/user-guide.md — section updated if people see a difference
- [ ] docs/decisions/ — new record if a design choice was made or reversed
- [ ] docs/architecture.md / README.md — if modules, data flow or settings changed
- [ ] docs/security.md — review-log row if a security-sensitive file changed

## Security

- [ ] Security review done and clean (link the review-log row)
- [ ] No secrets, tokens, real customer data or site-specific settings in the diff

Nothing is merged or published before the maintainer approves.
