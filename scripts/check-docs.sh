#!/usr/bin/env bash
# Check that a branch documents what it changes.
#   scripts/check-docs.sh [BASE]      (BASE defaults to main; compares BASE...HEAD)
# Settings (for other repositories): DOCS_MAP, DOCS_FEATURES, DOCS_CHANGELOG, DOCS_BRANCH_RE.
# Rules: docs/doc-map.tsv (path pattern -> required doc), plus commit trailers:
#   feat/fix/refactor/sec commits need "Feature: F-xxx" naming IDs that exist in docs/features.md;
#   sec commits also need "Finding: <ID>".
# Exit 1 when an error-level rule fails; warnings only print.
set -uo pipefail
cd "$(git rev-parse --show-toplevel)" || exit 2
BASE="${1:-main}"
if ! git rev-parse --verify -q "$BASE" >/dev/null; then
  echo "check-docs: base '$BASE' not found; nothing to compare (first push?)"; exit 0
fi
MAP="${DOCS_MAP:-docs/doc-map.tsv}"
FEATURES="${DOCS_FEATURES:-docs/features.md}"
CHANGELOG="${DOCS_CHANGELOG:-CHANGELOG.md}"
changed=$(git diff --name-only "$BASE"...HEAD)
[ -z "$changed" ] && { echo "check-docs: no changes against $BASE"; exit 0; }

errors=0; warnings=0
err()  { echo "  ✗ $*"; errors=$((errors+1)); }
warn() { echo "  ! $*"; warnings=$((warnings+1)); }

echo "check-docs: $(echo "$changed" | wc -l | tr -d ' ') changed file(s) against $BASE"

# 1. Path rules.
declare -A reported=()
while IFS=$'\t' read -r pattern doc level why; do
  [[ -z "${pattern:-}" || "$pattern" == \#* ]] && continue
  hits=$(echo "$changed" | grep -E "$pattern" || true)
  [ -z "$hits" ] && continue
  if ! echo "$changed" | grep -qxF "$doc"; then
    key="$doc|$why"; [ -n "${reported[$key]:-}" ] && continue; reported[$key]=1
    first=$(echo "$hits" | head -1)
    if [ "$level" = "error" ]; then err "$doc not updated — $why (e.g. $first)"; else warn "$doc — $why (e.g. $first)"; fi
  fi
done < "$MAP"

# 2. The changelog line must be under [Unreleased].
if echo "$changed" | grep -qxF "$CHANGELOG"; then
  added_under_unreleased=$(git diff "$BASE"...HEAD -- "$CHANGELOG" | awk '
    /^@@/ {next}
    /^[ +-]## \[/ { sect = ($0 ~ /\[Unreleased\]/) ? 1 : 0 }
    /^\+[^+]/ && sect == 1 && $0 !~ /^\+## / { n++ }
    END { print n+0 }')
  # Lines added right after the heading may not show the heading in the hunk; fall back to position.
  if [ "$added_under_unreleased" = "0" ]; then
    unrel=$(grep -n '^## \[Unreleased\]' "$CHANGELOG" | head -1 | cut -d: -f1)
    next=$(awk -v u="${unrel:-0}" 'NR>u && /^## \[/ {print NR; exit}' "$CHANGELOG")
    body=$(sed -n "$((unrel+1)),$(( ${next:-99999} - 1 ))p" "$CHANGELOG" | grep -E '^- ' || true)
    [ -z "$body" ] && err "$CHANGELOG changed but [Unreleased] has no entries"
  fi
fi

# 3. Commit trailers.
known=$(grep -oE '^\| F-[A-Z]+[0-9]+' "$FEATURES" 2>/dev/null | tr -d '| ' | sort -u)
while read -r sha; do
  [ -z "$sha" ] && continue
  subject=$(git log -1 --format=%s "$sha")
  body=$(git log -1 --format=%B "$sha")
  type=$(echo "$subject" | grep -oE '^[a-z]+' || true)
  case "$type" in
    feat|fix|refactor|sec)
      ids=$(echo "$body" | grep -E '^Feature:' | grep -oE 'F-[A-Z]+[0-9]+' | sort -u)
      if [ -z "$ids" ]; then err "commit ${sha:0:8} \"$subject\" has no 'Feature: F-xxx' trailer"; fi
      for id in $ids; do echo "$known" | grep -qxF "$id" || err "commit ${sha:0:8} names $id, which is not in $FEATURES"; done
      if [ "$type" = sec ] && ! echo "$body" | grep -qE '^Finding: [A-Z][0-9]+'; then err "security commit ${sha:0:8} has no 'Finding: <ID>' trailer"; fi
      ;;
    docs|test|chore|ci|build|style) ;;
    *) warn "commit ${sha:0:8} \"$subject\" doesn't follow Conventional Commits (type: …)";;
  esac
done < <(git rev-list --no-merges "$BASE"..HEAD)

# 4. Branch name.
branch=$(git rev-parse --abbrev-ref HEAD)
BRANCH_RE="${DOCS_BRANCH_RE:-^(feat|fix|sec|docs|refactor|test|chore)/((F-[A-Z]+[0-9]+|[A-Z][0-9]+)-)?[a-z0-9.-]+$}"
if ! echo "$branch" | grep -qE "$BRANCH_RE"; then
  warn "branch '$branch' doesn't match <type>/<feature-id>-<slug>"
fi

echo "check-docs: $errors error(s), $warnings warning(s)"
[ "$errors" -eq 0 ]
