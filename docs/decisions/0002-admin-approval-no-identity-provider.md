# 0002. Admin-approved email/password accounts, no outside identity provider

Date: 2026-11-08 · Status: accepted · Features: F-C01

## Context

Team members use mixed emails (company and personal); the owner does not want Google sign-in and wants an admin to approve every account. No outside mail service is assumed.

## Decision

Email + password (argon2id). New accounts are pending until an admin approves them into a team with a role. Admins reset passwords with one-time passwords that must be changed at first sign-in. A proxy-level check (e.g. Cloudflare Access) can be added in front, but is not required.

## Consequences

No email verification or self-service reset (an admin does resets). Rate limits and the approval gate carry the anti-abuse load. Adding SSO later must keep admin approval.
