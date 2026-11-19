# 0001. A generic add-on, kept separate from any deployment

Date: 2026-11-08 · Status: accepted · Features: F-C01–F-C10

## Context

The owner wants a multi-user layer that could later help many SolidCraft users, while running it for their own team with their own domain, branding and private CAD readers.

## Decision

solidcraft-collab contains no deployment specifics. Everything site-specific is configuration (`COLLAB_*` environment variables). Private additions (e.g. commercial CAD readers) live in the deployment's own SolidCraft build, which collab only calls through `COLLAB_CLI`.

## Consequences

Upstream-shareable code stays clean. A deployment change never needs a collab code change. Features that need deployment data must be configurable, not hard-coded.
