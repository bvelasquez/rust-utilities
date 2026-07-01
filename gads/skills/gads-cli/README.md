# gads CLI — LLM skill pack

This folder contains an [Agent Skill](https://cursor.com/docs/context/skills) for the **gads** Google Ads CLI so coding agents know how to discover, read, and safely mutate campaigns.

## Contents

| File | Purpose |
|------|---------|
| `SKILL.md` | Skill definition (frontmatter + agent instructions) |

## Install for Cursor

Copy into your personal skills directory:

```bash
mkdir -p ~/.cursor/skills/gads-cli
cp SKILL.md ~/.cursor/skills/gads-cli/SKILL.md
```

Or symlink (stays in sync with this repo):

```bash
mkdir -p ~/.cursor/skills
ln -sf "$(pwd)/SKILL.md" ~/.cursor/skills/gads-cli/SKILL.md
```

Restart or start a new Cursor agent session. The skill should appear when tasks mention Google Ads, `gads`, GAQL, or campaign management.

## Install for Claude Code

```bash
mkdir -p ~/.claude/skills/gads-cli
cp SKILL.md ~/.claude/skills/gads-cli/SKILL.md
```

(Adjust path if your Claude Code skills directory differs.)

## Install for other agents

Many agent frameworks accept a `SKILL.md` with YAML frontmatter:

- **`name`**: `gads-cli`
- **`description`**: When the agent should load this skill (keep specific; see frontmatter)

Copy `SKILL.md` into that tool’s skills folder, or reference it in project rules:

```markdown
When working with Google Ads, follow instructions in `skills/gads-cli/SKILL.md`.
```

## Prerequisites

Agents assume **`gads` is on PATH** and credentials exist:

```bash
cargo install --path /path/to/utilities/gads
gads auth status --json
```

Point users to the main [README.md](../../README.md) for auth setup.

## Keeping the skill updated

When commands change, update:

1. `SKILL.md` (this folder)
2. `../../README.md` (human docs)
3. `gads capabilities --json` output (generated from code — run after code changes)

## Verify the skill works

Ask your agent:

> List my Google Ads accounts using gads.

Expected behavior:

1. Run `gads capabilities --json` or `gads customers --json`
2. Use `--json` on commands
3. Use `--dry-run` before any mutation
