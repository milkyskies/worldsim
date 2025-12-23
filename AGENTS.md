# 🚨 AGENT WORKFLOW (MANDATORY) 🚨

> **THIS IS NOT OPTIONAL.** Every agent MUST follow this workflow exactly.
> Failure to follow this workflow causes merge conflicts, lost work, and broken builds.

## Issue Tracking with bd (beads)

**IMPORTANT**: This project uses **bd (beads)** for ALL issue tracking. Do NOT use markdown TODOs, task lists, or other tracking methods.

### Why bd?

- Dependency-aware: Track blockers and relationships between issues
- Git-friendly: Auto-syncs to JSONL for version control
- Agent-optimized: JSON output, ready work detection, discovered-from links
- Prevents duplicate tracking systems and confusion

### Beads vs Serena Memory

**Use Beads for:**

- ALL actionable work items (features, bugs, tasks, chores)
- Tracking progress and status changes
- Dependencies and blockers between work items
- Design decisions tied to specific implementation tasks
- Polish, improvements, and "nice to have" items
- Anything in a "TODO" or "Remaining Work" list

**Use Serena Memory for:**

- Pure reference documentation (file locations, schema structure)
- Architectural patterns and decisions (how we do X in this codebase)
- Onboarding information (how authentication works, project structure)
- Library choices and rationale (why we use X instead of Y)
- Coding conventions and style guides specific to this project

**Rule of thumb:** If it's something to DO, use Beads. If it's something to KNOW, use Serena.

### Quick Start

**Check for ready work:**

```bash
bd ready --json
```

**Create new issues:**

```bash
bd create "Issue title" -t bug|feature|task -p 0-4 --json
bd create "Issue title" -p 1 --deps discovered-from:bd-123 --json
```

**Claim and update:**

```bash
bd update bd-42 --status in_progress --json
bd update bd-42 --priority 1 --json
```

**Complete work:**

```bash
bd close bd-42 --reason "Completed" --json
```

### Issue Types

- `bug` - Something broken
- `feature` - New functionality
- `task` - Work item (tests, docs, refactoring)
- `epic` - Large feature with subtasks
- `chore` - Maintenance (dependencies, tooling)

### Priorities

- `0` - Critical (security, data loss, broken builds)
- `1` - High (major features, important bugs)
- `2` - Medium (default, nice-to-have)
- `3` - Low (polish, optimization)
- `4` - Backlog (future ideas)

### Workflow for AI Agents

1. **Check ready work**: `bd ready` shows unblocked issues
2. **Claim your task**: `bd update <id> --status in_progress`
3. **Work on it**: Implement, test, document
4. **Discover new work?** Create linked issue:
   - `bd create "Found bug" -p 1 --deps discovered-from:<parent-id>`
5. **Complete**: `bd close <id> --reason "Done"`
6. **Commit together**: Always commit the `.beads/issues.jsonl` file together with the code changes so issue state stays in sync with code state

### Resuming Work on a Bead

When asked to resume work on a bead (e.g., "resume milky-123"), follow this discovery process:

**Step 1: Check the Bead Details**

```bash
bd show <bead-id>
# Look for external_ref field (e.g., "gh-25")
```

**Step 2: Check for Existing Branch**

If the bead has a GitHub issue reference (e.g., `gh-25`):

```bash
# List branches to find matching one
git branch -a | grep gh-25
```

**Step 3: Resume on the Correct Branch**

| Situation                | Action                                          |
| ------------------------ | ----------------------------------------------- |
| Branch exists            | `git checkout fix/gh-25.slug` and continue work |
| No branch but has gh-ref | Create branch: `git checkout -b fix/gh-25.slug` |
| No branch, no gh-ref     | Create a new branch for the work                |

**Complete Resume Workflow:**

```bash
# 1. Get bead details
bd show milky-123
# Example output: external_ref: "gh-25"

# 2. Check for existing branch
git branch -a | grep 25
# Found: fix/gh-25.weekly-recurring-task

# 3. Switch to the branch
git checkout fix/gh-25.weekly-recurring-task

# 4. Check current state
git status
git log --oneline -5

# 5. Continue work
```

### Auto-Sync

bd automatically syncs with git:

- Exports to `.beads/issues.jsonl` after changes (5s debounce)
- Imports from JSONL when newer (e.g., after `git pull`)
- No manual export/import needed!

### GitHub Copilot Integration

If using GitHub Copilot, also create `.github/copilot-instructions.md` for automatic instruction loading.
Run `bd onboard` to get the content, or see step 2 of the onboard instructions.

### MCP Server (Recommended)

If using Claude or MCP-compatible clients, install the beads MCP server:

```bash
pip install beads-mcp
```

Add to MCP config (e.g., `~/.config/claude/config.json`):

```json
{
  "beads": {
    "command": "beads-mcp",
    "args": []
  }
}
```

Then use `mcp__beads__*` functions instead of CLI commands.

### Managing AI-Generated Planning Documents

AI assistants often create planning and design documents during development:

- PLAN.md, IMPLEMENTATION.md, ARCHITECTURE.md
- DESIGN.md, CODEBASE_SUMMARY.md, INTEGRATION_PLAN.md
- TESTING_GUIDE.md, TECHNICAL_DESIGN.md, and similar files

**Best Practice: Use a dedicated directory for these ephemeral files**

**Recommended approach:**

- Create a `history/` directory in the project root
- Store ALL AI-generated planning/design docs in `history/`
- Keep the repository root clean and focused on permanent project files
- Only access `history/` when explicitly asked to review past planning

**Example .gitignore entry (optional):**

```
# AI planning documents (ephemeral)
history/
```

**Benefits:**

- ✅ Clean repository root
- ✅ Clear separation between ephemeral and permanent documentation
- ✅ Easy to exclude from version control if desired
- ✅ Preserves planning history for archeological research
- ✅ Reduces noise when browsing the project

### Important Rules

- ✅ Use bd for ALL task tracking
- ✅ Always use `--json` flag for programmatic use
- ✅ Link discovered work with `discovered-from` dependencies
- ✅ Check `bd ready` before asking "what should I work on?"
- ✅ Store AI planning docs in `history/` directory
- ❌ Do NOT create markdown TODO lists
- ❌ Do NOT use external issue trackers
- ❌ Do NOT duplicate tracking systems
- ❌ Do NOT clutter repo root with planning documents

### Agent Behavior Guidelines

- **DO NOT fix unrelated errors** - If typecheck or linting reveals errors outside your current task scope, ignore them. Only fix errors caused by your changes.
- **Stay focused** - Don't get sidetracked by pre-existing issues. Complete your assigned task first.
- **Report blockers** - If pre-existing errors block your work, inform the user rather than attempting to fix them yourself.
- **ALWAYS run `pnpm lint:fix` before pushing** - This is mandatory. Fix any linting errors in files you modified before creating PRs.

For more details, see README.md and QUICKSTART.md.

### Using bv as an AI sidecar

bv is a fast terminal UI for Beads projects (.beads/beads.jsonl). It renders lists/details and precomputes dependency metrics (PageRank, critical path, cycles, etc.) so you instantly see blockers and execution order. For agents, it’s a graph sidecar: instead of parsing JSONL or risking hallucinated traversal, call the robot flags to get deterministic, dependency-aware outputs.

- bv --robot-help — shows all AI-facing commands.
- bv --robot-insights — JSON graph metrics (PageRank, betweenness, HITS, critical path, cycles) with top-N summaries for quick triage.
- bv --robot-plan — JSON execution plan: parallel tracks, items per track, and unblocks lists showing what each item frees up.
- bv --robot-priority — JSON priority recommendations with reasoning and confidence.
- bv --robot-recipes — list recipes (default, actionable, blocked, etc.); apply via bv --recipe <name> to pre-filter/sort before other flags.
- bv --robot-diff --diff-since <commit|date> — JSON diff of issue changes, new/closed items, and cycles introduced/resolved.

Use these commands instead of hand-rolling graph logic; bv already computes the hard parts so agents can act safely and quickly.
