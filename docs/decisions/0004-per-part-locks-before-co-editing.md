# 0004. Per-part locks and presence before same-part co-editing

Date: 2026-11-08 · Status: accepted · Features: F-C05, F-C06, F-C12

## Context

The owner wants several people working at once with Canva/Docs-style presence. SolidCraft's timeline ids and topological references make merging concurrent edits of one design unsafe.

## Decision

Version 1: each file has its own edit lock; different people edit different parts simultaneously; everyone sees live who is editing or viewing what. Two people inside the same part (F-C12) comes later, through one server-side session that orders commands.

## Consequences

Viewers of a locked part watch it update on each save. Idle locks expire after 180 s; owners can force-free. Any branch adding co-editing must supersede this record.
