# AGM — Agent Package Manager

pnpm-style package manager for AI agent dependencies. One `agm.json`, any tool.

## Install

```bash
# Unix / macOS
curl -fsSL https://raw.githubusercontent.com/konghayao/peri/main/agm/install.sh | bash

# Windows
irm https://raw.githubusercontent.com/konghayao/peri/main/agm/install.ps1 | iex
```

## Quick Start

```bash
# 1. Init a project
agm init

# 2. Install a skill from GitHub
agm install --git https://github.com/obra/superpowers --tool claude

# 3. Install from a registry package
agm install some-package --tool claude

# 4. See what's installed
agm list

# 5. Remove a package
agm uninstall --package @git/obra/superpowers --tool claude
```

## How It Works

```
agm.json          # Your project manifest — what you depend on
agm.lock.json     # Lock file — exact versions resolved
~/.agm/store/     # Content-addressable store — one copy of each package
.claude/skills/   # Symlinks created by agm (tool-agnostic adapters)
```

**Add a dependency** → agm clones to `~/.agm/store/` → creates symlinks in your tool's directory → records the version in `agm.json` and `agm.lock.json`.

## Commands

| Command | Description |
|---------|-------------|
| `agm init` | Create `agm.json` |
| `agm install` | Install all dependencies from `agm.json` |
| `agm install --git <url> --tool <tool>` | Install directly from a git repo |
| `agm uninstall --package <name> --tool <tool>` | Remove a package |
| `agm list` | List installed packages |
| `agm update` | Update packages interactively |
| `agm gc` | Clean up unused store packages |
| `agm self-update` | Update agm itself |

## agm.json

```json
{
  "name": "my-project",
  "targets": ["claude"],
  "skills": {
    "@git/obra/superpowers": "6fd4507...",
    "some-registry-pkg": "^1.0.0"
  },
  "agents": {},
  "mcp": {},
  "overrides": {}
}
```

## Supported Tools

| Tool | Adapter |
|------|---------|
| Claude | `.claude/skills/`, `.claude/agents/` |

## Release

```bash
git tag agm-v0.1.0
git push origin agm-v0.1.0
```

CI builds 6 platforms (linux/macos/windows × x86_64/aarch64/riscv64) and publishes to GitHub Releases.
