# 0003. One git repository per project for versions

Date: 2026-11-08 · Status: accepted · Features: F-C04

## Context

Every save must be a version with an author, restorable, without a separate storage engine; SolidCraft designs can be plain JSON.

## Decision

Each project folder is a git repository. A save writes the file and commits only that path (literal pathspecs) with the person's name. History and restore read git. Writes in one project are serialised.

## Consequences

Storage grows with every version (project quota applies). Backups must include the repositories. Large binary uploads are kept in history; a future pruning policy needs its own decision.
