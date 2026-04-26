<!-- managed by milky-kit | DO NOT EDIT — changes will be overwritten on next sync -->

# Agent Instructions (OpenCode compatibility)

Claude Code reads `CLAUDE.md` and `.claude/rules/*.md` natively. This file exists so OpenCode (and other agent tools) can find them too.

## External file loading

When you see a file reference like `@.claude/rules/general.md`, use your Read tool to load it. Lazy — load based on what the current task actually needs, not preemptively. Loaded content is mandatory instruction.

Always-loaded rules are declared in `opencode.json` (`instructions` field). The rules below are topical — load them when relevant.

## Rules library

- @.claude/rules/claude-meta.md
- @.claude/rules/module-docs.md
- @.claude/rules/rust-style.md
- @.claude/rules/testing.md
- @.claude/rules/time.md
- @.claude/rules/worktrees.md
