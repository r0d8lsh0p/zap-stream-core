---
name: github-project-management
description: Manage the Shosho GitHub Project (Projects v2) and related issues via GitHub CLI. Use when asked to list backlog/status items, summarize project board state, move items between statuses, or create/update issues in the Shosho monorepo project.
---

# GitHub Project Management (Shosho)

## Overview

Use GitHub CLI + GraphQL to read and manage the Shosho Project board, including backlog/status views and issue triage.

## Quick Start (read-only)

1. Confirm auth scopes include `project` and `repo`:
   - `gh auth status`
2. Confirm repo owner from the current repo:
   - `git remote -v` (this repo is `r0d8lsh0p/zap-stream-core`, but issues/project live in `r0d8lsh0p/shosho-monorepo`)
3. List projects for the owner and confirm `Shosho` (project #1):
   - `gh project list --owner r0d8lsh0p --limit 200`
4. List all items and group by status:
   - `gh project item-list 1 --owner r0d8lsh0p --limit 200 --format json`
5. Filter items by status (example: Backlog):
   - `gh project item-list 1 --owner r0d8lsh0p --limit 200 --format json --jq '.items[] | select(.status=="Backlog") | {title:.title, number:.content.number, url:.content.url}'`

## Project Item Types (Issues vs Drafts)

This project currently tracks issues (not draft-only items). If the request is for lightweight planning, draft items are possible, but the default workflow here is: create issues and add them to the project.

## Status & Field Metadata

Use GraphQL to discover the Status field options and IDs:

```bash
gh api graphql -f query='query { user(login:"r0d8lsh0p") { projectV2(number: 1) { fields(first: 50) { nodes { ... on ProjectV2SingleSelectField { id name options { id name } } ... on ProjectV2Field { id name } } } } } }'
```

## Update Item Status (write)

1. Get item IDs:
   - `gh project item-list 1 --owner r0d8lsh0p --limit 200 --format json`
2. Get the Status field ID and option IDs (see query above).
3. Move an item:
   - `gh project item-edit --project-id <project-id> --id <item-id> --field-id <status-field-id> --single-select-option-id <option-id>`
4. Get the project ID when needed:
   - `gh project view 1 --owner r0d8lsh0p --format json --jq '.id'`

## Add an Issue to the Project

1. Create or identify the issue:
   - `gh issue create --repo r0d8lsh0p/shosho-monorepo --assignee <user> --body-file <path>`
2. Add the issue to the project:
   - `gh project item-add 1 --owner r0d8lsh0p --url <issue-url>`

## Add Dependencies Between Issues

Use a dedicated "Dependencies" section in the issue body and link issue numbers.

1. Fetch existing body:
   - `gh issue view <number> --repo r0d8lsh0p/shosho-monorepo --json body --jq '.body'`
2. Append a section like:
   - `## Dependencies`
   - `- [ ] #123`
3. Update the issue:
   - `gh issue edit <number> --repo r0d8lsh0p/shosho-monorepo --body-file <path>`

## Known Good Commands (from this repo)

- Auth + scopes: `gh auth status`
- Project list: `gh project list --owner r0d8lsh0p --limit 200`
- Item list (JSON): `gh project item-list 1 --owner r0d8lsh0p --limit 200 --format json`
- Add item: `gh project item-add 1 --owner r0d8lsh0p --url <issue-url> --format json --jq '.id'`
- Edit status: `gh project item-edit --project-id <project-id> --id <item-id> --field-id <status-field-id> --single-select-option-id <option-id>`
