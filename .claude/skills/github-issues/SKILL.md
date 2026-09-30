---
name: github-issues
description: Create, read and update GitHub issues for this fork via the GitHub CLI. Use when asked to file an issue, find an existing one, comment on one, or link issues together. Issues live in r0d8lsh0p/shosho-monorepo, not in the zap-stream-core repo.
---

# GitHub Issues (Shosho)

## There is no project board

**This org has no GitHub project (Projects v2) board.** Do not look for one, do not try to add issues to one, and do
not report a missing `project` scope on the `gh` token as a problem — nothing here needs it. Issues are the whole
tracking system.

An earlier version of this skill described a "Shosho Project #1" board with status fields and `gh project` commands.
That board does not exist; the commands were never usable. Removed 2026-09-30.

## Where issues live

Issues are in **`r0d8lsh0p/shosho-monorepo`**, not in this repo (`r0d8lsh0p/zap-stream-core`). Always pass
`--repo r0d8lsh0p/shosho-monorepo` explicitly. Do **not** run `gh repo set-default`.

## Quick reference

```bash
gh auth status                                                        # confirm login (needs `repo`)
gh issue list  --repo r0d8lsh0p/shosho-monorepo --limit 20
gh issue view  <n> --repo r0d8lsh0p/shosho-monorepo
gh issue create --repo r0d8lsh0p/shosho-monorepo --title "..." --body-file <path>
gh issue comment <n> --repo r0d8lsh0p/shosho-monorepo --body-file <path>
gh issue edit    <n> --repo r0d8lsh0p/shosho-monorepo --body-file <path>
gh issue close   <n> --repo r0d8lsh0p/shosho-monorepo
gh label list  --repo r0d8lsh0p/shosho-monorepo --limit 40
```

Write issue bodies to a file and use `--body-file` — long bodies passed via `--body` get mangled by shell quoting.

## Labels worth knowing

| Label | Use for |
|---|---|
| `area:zap-stream` | Streaming backend: this fork, Cloudflare Stream, NIP-53/30311, Railway deploy |
| `upstream` | Needs a change in `v0l/zap-stream-core` (or another upstream project), not only our code |
| `bug` / `enhancement` | Standard triage |

Anything written on a feature branch off `main` is upstream-submittable by construction, so it usually wants both
`area:zap-stream` and `upstream`.

## Linking issues

There is no dependency field. Use a `## Dependencies` section in the issue body with `- [ ] #123` checkboxes, fetching
and re-submitting the body:

```bash
gh issue view <n> --repo r0d8lsh0p/shosho-monorepo --json body --jq '.body' > /tmp/body.md
# append the section, then:
gh issue edit <n> --repo r0d8lsh0p/shosho-monorepo --body-file /tmp/body.md
```
