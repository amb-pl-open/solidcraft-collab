# 0006. CAD conversion in a sandboxed background queue

Date: 2026-11-08 · Status: accepted · Features: F-C09, F-C10

## Context

Uploaded CAD files are hostile input parsed by large decoders; the first design ran them inline, unsandboxed, as the service user. Security review found DoS and a confirmed sandbox escape through the user D-Bus.

## Decision

Conversions run one at a time, from a bounded queue, after the upload is committed. Each runs in a transient `systemd-run --user` unit: no network, no Unix sockets, `/run/user` hidden, memory cap without swap, one CPU, time limit, syscall filter, own user namespace, only its job folder writable. Output must be a SolidCraft design and never overwrites a file.

## Consequences

Hosts need a user systemd instance. Uploads answer immediately; designs appear when ready (live event). Separate system users remain recommended for defence in depth.
