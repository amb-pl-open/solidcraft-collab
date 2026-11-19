# 0005. Embed the stock SolidCraft browser build through its public API

Date: 2026-11-08 · Status: accepted · Features: F-C08

## Context

Forking SolidCraft's UI would make every upstream release a merge job.

## Decision

The editor page frames the unmodified browser build at `/app/?embed=editor` from the same origin and uses only the documented browser API (`document.import`, `document.export`, `document.saved`, `document.revision`, `ui.inspect`, `ui.selection`).

## Consequences

Upgrading SolidCraft is a file swap plus a service restart (the app CSP pins its inline-script hashes at start-up). Presence detail is best effort, limited to what the API exposes. Linked part files of assemblies are not loaded yet (F-C13).
