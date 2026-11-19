# Decision records

Short records of choices that shape the code, so a later branch doesn't undo one by accident.
One file per decision: `NNNN-short-title.md`, never renumbered. To change a decision, add a new
record that **supersedes** the old one and mark the old one "Superseded by NNNN".

Template:

```markdown
# NNNN. Title
Date: YYYY-MM-DD · Status: accepted | superseded by NNNN · Features: F-Cxx
## Context
## Decision
## Consequences
```

| No. | Decision | Status |
|---|---|---|
| [0001](0001-generic-add-on-separate-from-deployments.md) | A generic add-on, kept separate from any deployment | accepted |
| [0002](0002-admin-approval-no-identity-provider.md) | Admin-approved email/password accounts, no outside identity provider | accepted |
| [0003](0003-git-repository-per-project.md) | One git repository per project for versions | accepted |
| [0004](0004-per-part-locks-before-co-editing.md) | Per-part locks and presence before same-part co-editing | accepted |
| [0005](0005-embed-stock-solidcraft.md) | Embed the stock SolidCraft browser build through its public API | accepted |
| [0006](0006-sandboxed-background-conversion.md) | CAD conversion in a sandboxed background queue | accepted |
