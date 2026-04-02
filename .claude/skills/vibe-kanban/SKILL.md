---
name: vibe-kanban
description: Vibe Kanban task management workflow for agents, including epic-to-subtask linking, required fields, and status checks.
user-invocable: true
allowed-tools: Read, Grep, Glob
---

# VK Task Management

## Core Workflow
1. `list_projects` -> get `project_id`
2. `list_repos` -> get `repo_id`
3. `create_task` -> get `task_id`
4. `start_workspace_session` with `task_id`, `executor`, and `repos`
5. Poll `get_task` to monitor status

## Anti-Patterns
- Do not query the VK database directly.
- Do not guess IDs; fetch them.
- Do not omit the `repos` array in `start_workspace_session`.
- Do not use local sub-agent tools for VK-tracked work.

## Version Pinning
- Do not change VK versions or tooling without explicit repo guidance.
- If MCP tools seem stale, ask to restart the session to pick up updates.

## Epic → Subtask Pattern
1. **Discover your workspace ID**
   - Call `get_task` with your own `task_id`.
   - Read `current_workspace.id` as your `workspace_id`.
2. **Create each subtask with linkage**
   - Call `create_task` and include `parent_workspace_id: "<epic-workspace-id>"`.
3. **Spawn subtask sessions from the epic branch**
   - Use `start_workspace_session` with `base_branch` set to the epic branch.
4. **Track subtask progress**
   - Use `get_task` or `list_tasks` to monitor statuses.

## References
- `notes/ai-guide-to-vibe-kanban-mcp.md`
