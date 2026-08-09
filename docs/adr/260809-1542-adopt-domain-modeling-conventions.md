---
id: ADR-260809-1542-01
status: accepted
---

# Adopt the domain-modeling record conventions

This repo's records already used the `<KIND>-YYMMDD-HHMM-NN` ID grammar and the flat `docs/` homes, but
three files sat outside their canonical homes. We adopted the conventions wholesale — ID grammar
(`TRACEABILITY.md`), directory homes (`DOCS-LAYOUT.md`), ADR format (`ADR-FORMAT.md`), glossary format
(`CONTEXT-FORMAT.md`), issue schema (`triage`), and the record↔review-file citation grammar
(`fix-code-review`) — and relocated only those three files, because a record whose home encodes how it
may be read is worth more than one that merely has a valid name.

## Evidence

- No ID in the tree needed rewriting: every occurrence already matched `<KIND>-YYMMDD-HHMM-NN`.
  Provenance: `rg -uu -oIN -g '!.git/**' -g '!target/**' '\b(ISSUE|PRD|ADR|RDMP|DEC)-[0-9]{6}-[0-9]{4}(-[0-9]+)?' . | grep -vcE '\-[0-9]{6}-[0-9]{4}-[0-9]{2}$'` → 0; 2026-08-09.
- Both mechanical migrator tiers were dry-run and reported zero work, so neither was applied:
  `0 rename(s), 0 content file(s), 7 report entries` (datetime) and `0 rename(s), 0 content file(s), 0 report entries` (legacy).
  All 7 datetime-tier entries are `bucket=mapped-but-quoted`.
  Provenance: `migrate-id-grammar.sh --tier datetime|legacy`; 2026-08-09.
- 10 issue records and 1 PRD record were left untouched — IDs, filenames, homes, frontmatter, and body
  sections all conformed.
  Provenance: `find docs/issues -maxdepth 1 -name 'ISSUE-*.md' | wc -l`, `find docs/prd -maxdepth 1 -name 'PRD-*.md' | wc -l`; 2026-08-09.
- 3 files were relocated, all as git renames with content unchanged.
  Provenance: `git diff --cached --name-status | grep -c '^R'`; 2026-08-09.

## Consequences

Two shapes now coexist, deliberately:

- **The code-review file keeps its pre-convention content but gained a conforming name.** It moved from
  `docs/local/` to the canonical sibling home `docs/issues/code-reviews/`, renamed to
  `extract-library-code-review-20260626-002018.md`. The timestamp is the file's git first-add in UTC
  (commit `69173d1`), not the `**Date:** 2026-06-10` line in its body — the naming grammar needs
  `HHMMSS`, which the body date cannot supply without invention. The two dates disagree by 16 days; the
  body line was left as written. `docs/local/` had no counterpart in `DOCS-LAYOUT.md` and is now gone.
- **The adversary-evidence filenames are grandfathered; only their home was fixed.** Both files moved
  into `docs/prd/adversary-reports/` so that record enumeration, which never descends into that
  sibling, can no longer parse `PRD-260801-0656-01-adversary-260801-report.md` as a duplicate of the
  real PRD's ID. Their names keep the pre-convention `-adversary-<YYMMDD>-<report|prompt>` form rather
  than the current `-adversary-<YYMMDD>-round<N>-<report|prompt>` form: the files landed
  2026-08-02T15:36:33Z and the convention landed 2026-08-03T01:17:37Z, so `DOCS-LAYOUT.md`
  § Adversary evidence grandfathers them in place, and a `round<N>` ordinal would have been inferred
  rather than derived.

No citations needed rewriting — nothing in the repo, its code, its scripts, or its configs referenced
any of the three moved paths.
