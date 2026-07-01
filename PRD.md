# Portal — PRD

> *"Anything pushed through the portal ends up on the other side."*

A fast, lightweight Rust CLI + TUI tool for saving, switching, and diffing `.claude` configuration profiles with bulletproof safety guarantees.

---

## Table of Contents

1. [Problem Statement](#1-problem-statement)
2. [Product Vision](#2-product-vision)
3. [Target User](#3-target-user)
4. [Core Concepts](#4-core-concepts)
5. [Architecture](#5-architecture)
6. [Data Model](#6-data-model)
7. [CLI Interface](#7-cli-interface)
8. [TUI Design](#8-tui-design)
9. [Plugin Lifecycle](#9-plugin-lifecycle)
10. [Safety Model](#10-safety-model)
11. [Diff Engine](#11-diff-engine)
12. [Command Reference](#12-command-reference)
13. [File Manifest](#13-file-manifest--what-portal-manages)
14. [Error Handling & Recovery](#14-error-handling--recovery)
15. [Implementation Plan](#15-implementation-plan)
16. [Non-Goals](#16-non-goals)
17. [Resolved Decisions](#17-resolved-decisions)

---

## 1. Problem Statement

Claude Code's configuration lives in `~/.claude/` — a sprawling directory with rules, skills, memory, commands, hooks, agents, plugins, and project-specific settings. Users who customize heavily end up with:

- **No profile isolation**: Switching between "work red-team mode" and "personal web-dev mode" means manually swapping or overwriting files.
- **No diffing**: No way to see what differs between two configurations or what a profile adds beyond the base.
- **No rollback**: A bad edit to `settings.json` or `CLAUDE.md` can break the entire setup with no undo.
- **No skeleton reset**: Starting fresh requires manually knowing which files are required vs. optional.

Portal solves this by treating `.claude` configurations as **versioned, portable profiles** that can be saved, loaded, diffed, and restored — with a skeleton "bare minimum" as the neutral ground between them.

---

## 2. Product Vision

Portal is a **configuration transport layer** for Claude Code. It:

1. **Saves** the current `.claude/` state as a named profile (snapshot)
2. **Loads** a profile by overlaying it onto a fresh skeleton `.claude/` directory
3. **Diffs** any two profiles (or a profile vs. skeleton) to show exactly what each adds
4. **Protects** the user's setup with atomic swaps, checksums, and automatic backups

The skeleton is the portal's "other side" — a minimal, known-good `.claude/` with only `settings.json` (defaults), `CLAUDE.md` (empty), and required directory structure. Every profile is defined by its **delta from this skeleton**.

---

## 3. Target User

Power users of Claude Code who:
- Maintain multiple "personas" or configurations (red-team, web-dev, research, personal)
- Want instant switching without manual file management
- Need safety guarantees — never lose a working configuration
- Want visibility into what each profile changes

---

## 4. Core Concepts

### 4.1 Skeleton (The Base State)

The minimal `.claude/` that Claude Code needs to function:

```
~/.claude/
  settings.json          # Default settings (no custom hooks, no plugins, default permissions)
  CLAUDE.md              # Empty file (0 bytes, just exists)
  .claude/
    settings.local.json  # Empty JSON object: {}
    hooks/               # Empty directory
  skills/                # Empty directory (required by Claude)
  memory/                # Empty directory
  commands/              # Empty directory
  agents/                # Empty directory
  rules/                 # Empty directory
  hooks/                 # Empty directory
```

### 4.2 Profile (A Saved Configuration)

A profile is a **snapshot** of the user's `.claude/` directory, stored in `~/.portal/profiles/<name>/`. It contains:

- All files that differ from the skeleton (content + checksum)
- A manifest (`portal.json`) listing every tracked file with its SHA-256 hash
- A plugin blueprint (`plugins.json`) describing which plugins to install on load
- Metadata: created_at, last_loaded, description, tags

A profile does **not** copy:
- `session-env/`, `sessions/`, `shell-snapshots/` (ephemeral runtime data)
- `history.jsonl` (conversation history)
- `todos/` (task state)
- `file-history/` (Claude internal)
- `telemetry/`, `statsig/` (analytics)
- `paste-cache/`, `debug/` (temp)
- `plugins/cache/`, `plugins/marketplaces/`, `plugins/data/` (auto-managed, handled via blueprint)

### 4.3 Transport (The Operation)

"Pushing through the portal" = atomically replacing `~/.claude/` with a profile's content:

```
CURRENT .claude/  ──[portal save]──>  ~/.portal/profiles/<name>/
                                       (snapshot stored + plugin blueprint)

SKELETON .claude/ ──[portal load <name>]──>  ~/.claude/
                                             (profile overlaid on skeleton)
                                             (plugins reinstalled from blueprint)
```

---

## 5. Architecture

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                          PORTAL ARCHITECTURE                                │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│  ┌──────────────┐    ┌──────────────┐    ┌───────────────────────────────┐  │
│  │   CLI Layer   │    │   TUI Layer   │    │      Core Engine              │  │
│  │  (clap)      │    │  (ratatui)   │    │                               │  │
│  │              │    │              │    │  ┌─────────────────────────┐  │  │
│  │ portal save  │───>│  Split-Pane  │───>│  │ Snapshot Engine         │  │  │
│  │ portal load  │    │  Browser     │    │  │ (copy + hash + blueprint)│  │  │
│  │ portal diff  │    │              │    │  └──────────┬──────────────┘  │  │
│  │ portal list  │    │  Detail +    │    │             │                 │  │
│  │ portal rm    │    │  Diff View   │    │  ┌──────────▼──────────────┐  │  │
│  │ portal reset │    │              │    │  │ Diff Engine             │  │  │
│  │ portal show  │    │  Content     │    │  │ (compare profiles vs    │  │  │
│  │              │    │  Diff View   │    │  │  skeleton or each other) │  │  │
│  └──────────────┘    └──────────────┘    │  └──────────┬──────────────┘  │  │
│                                          │             │                 │  │
│                                          │  ┌──────────▼──────────────┐  │  │
│                                          │  │ Plugin Manager          │  │  │
│                                          │  │ (blueprint read/write,  │  │  │
│                                          │  │  install on load,        │  │  │
│                                          │  │  verify on save)         │  │  │
│                                          │  └─────────────────────────┘  │  │
│  ┌──────────────────────────────────────────────────────────────────────┐  │
│  │                         Safety Layer                                  │  │
│  │                                                                       │  │
│  │  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐                │  │
│  │  │ Atomic Swap  │  │ Checksum     │  │ Auto-Backup  │                │  │
│  │  │ (tempdir +   │  │ Verification │  │ (pre-op      │                │  │
│  │  │  rename)     │  │ (SHA-256)    │  │  snapshot)   │                │  │
│  │  └──────────────┘  └──────────────┘  └──────────────┘                │  │
│  │                                                                       │  │
│  │  ┌──────────────┐  ┌──────────────┐                                   │  │
│  │  │ Dry-Run Mode │  │ Rollback     │                                   │  │
│  │  │ (--dry-run)  │  │ (portal undo)│                                   │  │
│  │  └──────────────┘  └──────────────┘                                   │  │
│  └──────────────────────────────────────────────────────────────────────┘  │
│                                                                             │
│  ┌──────────────────────────────────────────────────────────────────────┐  │
│  │                         Storage Layer                                  │  │
│  │                                                                       │  │
│  │  ~/.portal/                                                           │  │
│  │  ├── profiles/                                                        │  │
│  │  │   ├── work-redteam/                                                │  │
│  │  │   │   ├── portal.json     (manifest)                               │  │
│  │  │   │   ├── plugins.json    (plugin blueprint)                       │  │
│  │  │   │   ├── files/          (actual file contents)                    │  │
│  │  │   │   │   ├── CLAUDE.md                                           │  │
│  │  │   │   │   ├── settings.json                                       │  │
│  │  │   │   │   ├── rules/behaviors.md                                  │  │
│  │  │   │   │   ├── skills/autoagent/SKILL.md                           │  │
│  │  │   │   │   └── ...                                                 │  │
│  │  │   │   └── meta.json       (metadata)                              │  │
│  │  │   ├── personal-webdev/                                             │  │
│  │  │   └── research/                                                    │  │
│  │  ├── skeleton/               (reference skeleton files)               │  │
│  │  │   ├── skeleton.json       (skeleton manifest)                      │  │
│  │  │   └── files/                                                       │  │
│  │  ├── backups/                (auto-backups before each op)            │  │
│  │  │   ├── pre-load-2026-04-22T21:00:00.tar.zst                        │  │
│  │  │   └── ...                                                          │  │
│  │  └── portal.state.json       (current state: active profile)          │  │
│  └──────────────────────────────────────────────────────────────────────┘  │
└─────────────────────────────────────────────────────────────────────────────┘
```

### 5.1 Atomic Swap Flow

```
portal load work-redteam
         │
         ▼
  ┌─────────────────┐
  │ 1. Pre-flight   │  Verify profile exists & checksums valid
  │    checks       │  Verify no Claude session is running
  └────────┬────────┘
           │
           ▼
  ┌─────────────────┐
  │ 2. Auto-backup  │  Snapshot current ~/.claude/ → ~/.portal/backups/
  └────────┬────────┘
           │
           ▼
  ┌─────────────────┐
  │ 3. Build target │  Write skeleton to tempdir
  │    in tempdir   │  Overlay profile files onto tempdir
  └────────┬────────┘
           │
           ▼
  ┌─────────────────┐
  │ 4. Verify build │  Re-checksum all files in tempdir
  │    (checksums)  │  Compare against manifest
  └────────┬────────┘
           │
           ▼
  ┌─────────────────┐     ┌─────────────────┐
  │ 5. Atomic swap  │────>│ rename tempdir  │
  │                  │     │ → ~/.claude/    │
  └─────────────────┘     └─────────────────┘
           │
           ▼
  ┌─────────────────┐
  │ 6. Reinstall    │  Run `claude plugin install` for each
  │    plugins      │  plugin in the blueprint
  └────────┬────────┘
           │
           ▼
  ┌─────────────────┐
  │ 7. Post-flight  │  Update portal.state.json
  │    verification  │  Verify plugins installed correctly
  └─────────────────┘
```

If **any step fails**, the original `~/.claude/` is untouched. The swap only happens via a single `rename(2)` call which is atomic on the same filesystem. Plugin installation happens **after** the atomic swap and is independently recoverable (see Section 9).

---

## 6. Data Model

### 6.1 `portal.json` (Profile Manifest)

```jsonc
{
  "version": 1,
  "name": "work-redteam",
  "created_at": "2026-04-22T21:00:00Z",
  "last_loaded": "2026-04-22T22:30:00Z",
  "load_count": 14,
  "description": "Offensive security + red team workflows",
  "tags": ["security", "redteam", "work"],
  "files": {
    "CLAUDE.md": {
      "checksum": "sha256:a1b2c3d4...",
      "size": 16094,
      "source": "user"
    },
    "settings.json": {
      "checksum": "sha256:e5f6g7h8...",
      "size": 3297,
      "source": "user"
    },
    "rules/behaviors.md": {
      "checksum": "sha256:i9j0k1l2...",
      "size": 5955,
      "source": "user"
    },
    "skills/autoagent/SKILL.md": {
      "checksum": "sha256:m3n4o5p6...",
      "size": 2048,
      "source": "user"
    },
    ".claude/settings.local.json": {
      "checksum": "sha256:q7r8s9t0...",
      "size": 1456,
      "source": "user"
    }
  },
  "excluded_patterns": [
    "session-env/**",
    "sessions/**",
    "shell-snapshots/**",
    "history.jsonl",
    "todos/**",
    "file-history/**",
    "telemetry/**",
    "statsig/**",
    "paste-cache/**",
    "debug/**",
    "stats-cache.json",
    "mcp-needs-auth-cache.json",
    "plugins/cache/**",
    "plugins/marketplaces/**",
    "plugins/data/**",
    ".DS_Store"
  ]
}
```

### 6.2 `plugins.json` (Plugin Blueprint)

The plugin blueprint captures which plugins are installed, their source, and enough information to reinstall them on load. It does **not** store the plugin code itself — plugins are reinstalled from their marketplace or local source.

```jsonc
{
  "version": 1,
  "plugins": [
    {
      "id": "claude-hud@claude-hud",
      "enabled": true,
      "source": {
        "type": "marketplace",         // "marketplace" | "local" | "github"
        "marketplace": "claude-hud",
        "repo": "jarrodwatts/claude-hud"
      }
    },
    {
      "id": "superpowers@claude-plugins-official",
      "enabled": true,
      "source": {
        "type": "marketplace",
        "marketplace": "superpowers-marketplace",
        "repo": "obra/superpowers-marketplace"
      }
    },
    {
      "id": "shield@shield-security",
      "enabled": true,
      "source": {
        "type": "local",
        "path": "/Users/rohit/Documents/shield-claude-skill"
      }
    }
  ],
  "extra_known_marketplaces": {
    "claude-hud": {
      "source": {
        "source": "github",
        "repo": "jarrodwatts/claude-hud"
      }
    },
    "superpowers-marketplace": {
      "source": {
        "source": "github",
        "repo": "obra/superpowers-marketplace"
      }
    },
    "shield-security": {
      "source": {
        "source": "directory",
        "path": "/Users/rohit/Documents/shield-claude-skill"
      }
    }
  }
}
```

**Source types:**

| Type          | Reinstall Method                                    | Requires               |
|---------------|-----------------------------------------------------|------------------------|
| `marketplace` | `claude plugin install <id>` from known marketplace | Marketplace registered |
| `github`      | Clone repo + install                                | Git access             |
| `local`       | Verify path exists + install from path              | Path still exists      |

### 6.3 `portal.state.json` (Global State)

```jsonc
{
  "version": 1,
  "active_profile": "work-redteam",
  "last_operation": {
    "type": "load",
    "profile": "work-redteam",
    "timestamp": "2026-04-22T22:30:00Z",
    "backup_path": "~/.portal/backups/pre-load-2026-04-22T22:30:00.tar.zst",
    "plugins_installed": true
  },
  "skeleton_checksum": "sha256:deadbeef..."
}
```

### 6.4 `meta.json` (Profile Metadata — Human-Editable)

```jsonc
{
  "description": "Offensive security + red team workflows",
  "tags": ["security", "redteam", "work"],
  "notes": "Includes atomic-red-team MCP, red-teaming skill tree, shield plugin",
  "created_by": "portal v0.1.0"
}
```

---

## 7. CLI Interface

```
portal                          Launch TUI (interactive mode)
portal save [NAME]              Save current .claude/ as profile (prompts if no name)
portal load <NAME>              Load profile (atomic swap, auto-backup, plugin reinstall)
portal toggle                   Swap back to the previously active profile (instant)
portal use [NAME]               Launch claude bound to a profile's isolated CLAUDE_CONFIG_DIR (no swap)
portal list                     List all profiles (table format)
portal show <NAME>              Show profile details + file manifest + plugins
portal diff <A> [B]             Diff two profiles (B defaults to skeleton)
portal clone <SRC> <DST>        Fork a profile selectively (--only/--without/--fresh-claude-md)
portal rm <NAME>                Delete a profile (requires confirmation)
portal reset                    Reset .claude/ to skeleton
portal undo                     Undo last load/reset (restore from backup)
portal recover                  Recover from a crashed swap (.claude.portal-old)
portal status                   Show current active profile + state + plugin health
portal doctor                   Diagnose portal health + offer guided repairs (--fix)
portal history [NAME]           Show a profile's git history (commits on its history branch)
portal rename <OLD> <NEW>       Rename a profile
portal export <NAME> [PATH]     Export profile as .tar.zst archive
portal import <PATH>            Import profile from .tar.zst archive
portal verify [NAME]            Verify profile integrity (checksums + plugins)

Flags (global):
  --dry-run                     Show what would happen, don't execute
  --no-backup                   Skip auto-backup (DANGEROUS, requires --force)
  --no-plugins                  Skip plugin reinstallation on load
  --force                       Override safety checks (requires confirmation)
  --verbose / -v                Verbose output
  --quiet / -q                  Quiet mode (errors only)
```

---

## 8. TUI Design — Split-Pane Browser + Detail

The TUI launches with `portal` (no subcommand). It uses `ratatui` with `crossterm` backend.
Design: **Split-pane file-manager style** — profile list on the left, detail/diff on the right.

### 8.1 Default View: Profile Browser

```
┌─────────────────────────────────────────────────────────────────────────────┐
│  ◉ PORTAL — Configuration Transport                          [q]uit [?]help │
├──────────────────────────┬──────────────────────────────────────────────────┤
│  Profiles                │  work-redteam  ◉ active                          │
│ ┌──────────────────────┐ │  ┌──────────────────────────────────────────────┐│
│ │ ◉ work-redteam    *  │ │  │ Description: Offensive security + red team   ││
│ │ ○ personal-webdev    │ │  │ Tags: security, redteam, work               ││
│ │ ○ research           │ │  │ Created: 2026-04-22                         ││
│ │ ○ skeleton (base)    │ │  │ Last loaded: 2026-04-22 22:30              ││
│ │                       │ │  │ Load count: 14                              ││
│ │                       │ │  │                                              ││
│ │                       │ │  │ Tracked Files (37)                          ││
│ │                       │ │  │ ┌──────────────────────────────────────────┐││
│ │                       │ │  │ │ ◉ CLAUDE.md             16KB  user     │││
│ │                       │ │  │ │ ◉ settings.json        3.2KB user     │││
│ │                       │ │  │ │ ◉ .claude/settings.local 1.4KB user │││
│ │                       │ │  │ │ ◉ rules/behaviors.md   5.9KB user     │││
│ │                       │ │  │ │ ◉ skills/autoagent/    2.0KB user     │││
│ │                       │ │  │ │ ◉ skills/red-teaming/  18KB  user     │││
│ │                       │ │  │ │ ◉ agents/pr-reviewer   1.7KB user     │││
│ │                       │ │  │ │ ◉ commands/mission.md  8.3KB user     │││
│ │                       │ │  │ │ ◉ memory/...           35KB  user     │││
│ │                       │ │  │ │ ...                                        │││
│ │                       │ │  │ └──────────────────────────────────────────┘││
│ └──────────────────────┘ │  │                                              ││
│                          │  │ Plugins (3)                                  ││
│                          │  │   ◉ claude-hud         marketplace          ││
│                          │  │   ◉ superpowers        marketplace          ││
│                          │  │   ◉ shield-security    local               ││
│                          │  │                                              ││
│  * = active               │  │ [Enter] Load  [d] Diff  [x] Delete          ││
│                           │  │ [e] Export   [s] Save current               ││
└──────────────────────────┴──┴──────────────────────────────────────────────┘
│  Active: work-redteam │ Profiles: 3 │ Backups: 2 │ Last op: load (22:30)  │
└─────────────────────────────────────────────────────────────────────────────┘
```

### 8.2 Diff Mode: Side-by-Side Profile Comparison

Activated with `d` on a profile in the left pane. Right pane switches to a diff view comparing the selected profile against the active profile (or skeleton if `Tab` pressed).

```
┌─────────────────────────────────────────────────────────────────────────────┐
│  ◉ PORTAL — Diff Mode                         [Esc] Back [Tab] Switch     │
├──────────────────────────┬──────────────────────────────────────────────────┤
│  Profiles                │  DIFF: work-redteam ◉ ← → ○ personal-webdev     │
│ ┌──────────────────────┐ │  ┌──────────────────────────────────────────────┐│
│ │ ◉ work-redteam    *  │ │  │                                              ││
│ │ ○ personal-webdev  d │ │  │ Shared (same content):     1 file         ││
│ │ ○ research           │ │  │ Shared (different):        2 files        ││
│ │ ○ skeleton (base)    │ │  │ Only in work-redteam:     30 files        ││
│ │                       │ │  │ Only in personal-webdev:  10 files        ││
│ │                       │ │  │                                              ││
│ │                       │ │  │ ┌────────────────────────────────────────┐││
│ │                       │ │  │ │ Shared (different content)              │││
│ │                       │ │  │ │ ● CLAUDE.md        16KB → 4KB         │││
│ │                       │ │  │ │ ● settings.json   3.2KB → 2.1KB      │││
│ │                       │ │  │ ├────────────────────────────────────────┤││
│ │                       │ │  │ │ Only in work-redteam                   │││
│ │                       │ │  │ │ + rules/behaviors.md       5.9KB       │││
│ │                       │ │  │ │ + skills/autoagent/        2.0KB       │││
│ │                       │ │  │ │ + skills/red-teaming/      18KB        │││
│ │                       │ │  │ │ + agents/                 8 items     │││
│ │                       │ │  │ │ + commands/               5 items     │││
│ │                       │ │  │ │ + memory/                 15 items    │││
│ │                       │ │  │ ├────────────────────────────────────────┤││
│ │                       │ │  │ │ Only in personal-webdev                │││
│ │                       │ │  │ │ + skills/swiftui-pro/      6KB        │││
│ │                       │ │  │ │ + skills/helm/             3 items     │││
│ │                       │ │  │ │ + plugins/swift-lsp        1 item      │││
│ │                       │ │  │ └────────────────────────────────────────┘││
│ └──────────────────────┘ │  │                                              ││
│                          │  │ Plugins                                     ││
│  d = diff target          │  │   Only in work-redteam:                     ││
│                           │  │     + superpowers (marketplace)              ││
│                           │  │     + shield-security (local)                ││
│                           │  │   Only in personal-webdev:                   ││
│                           │  │     + swift-lsp (marketplace)                ││
│                           │  │                                              ││
│                           │  │ [Enter] View file diff  [Tab] vs skeleton  ││
└──────────────────────────┴──┴──────────────────────────────────────────────┘
│  Diff: work-redteam vs personal-webdev │ [Esc] Back to detail view         │
└─────────────────────────────────────────────────────────────────────────────┘
```

### 8.3 Content Diff View: Inline File Comparison

When `Enter` is pressed on a file in diff mode, the right pane shows a unified diff of the file contents.

```
┌─────────────────────────────────────────────────────────────────────────────┐
│  ◉ PORTAL — File Diff                         [Esc] Back [n/N] Next hunk  │
├──────────────────────────┬──────────────────────────────────────────────────┤
│  Profiles                │  CLAUDE.md: work-redteam vs personal-webdev      │
│ ┌──────────────────────┐ │  ┌──────────────────────────────────────────────┐│
│ │ ◉ work-redteam    *  │ │  │  1  │ # CLAUDE.md          │ # CLAUDE.md    ││
│ │ ○ personal-webdev    │ │  │  2  │                      │                 ││
│ │ ○ research           │ │  │  3  │ Rohit is your       │ Clean web dev   ││
│ │ ○ skeleton (base)    │ │  │     │ creator.            │ setup.          ││
│ │                       │ │  │  4  │ Work style:         │ Work style:     ││
│ │                       │ │  │     │ telegraph; noun-   │ verbose,        ││
│ │                       │ │  │     │ phrases ok; drop   │ descriptive,    ││
│ │                       │ │  │     │ grammar; min tokens │ friendly.       ││
│ │                       │ │  │  5  │ Tone: Technical,   │ Tone: Helpful,  ││
│ │                       │ │  │     │ concise, authori-  │ patient,        ││
│ │                       │ │  │     │ tative             │ educational.    ││
│ │                       │ │  │     │                     │                 ││
│ │                       │ │  │ ... │ ...                 │ ...             ││
│ │                       │ │  │     │                     │                 ││
│ │                       │ │  │ 47  │ ## Red Team Rules   │ ## Web Dev Rules││
│ │                       │ │  │ 48  │ + Always validate   │ + Use conven-   ││
│ │                       │ │  │     │   targets           │   tional        ││
│ │                       │ │  │ 49  │ + Scope before      │ + commits for   ││
│ │                       │ │  │     │   exploitation      │   all changes   ││
│ │                       │ │  │ 50  │ + Document all      │ + Test before   ││
│ │                       │ │  │     │   findings          │   deploy        ││
│ │                       │ │  │     │                     │                 ││
│ └──────────────────────┘ │  │ [j/k] Scroll  [n/N] Next/prev hunk          ││
│                          │  │ [Esc] Back to diff list                      ││
└──────────────────────────┴──┴──────────────────────────────────────────────┘
│  File: CLAUDE.md │ 3 hunks │ [Esc] Back                                    │
└─────────────────────────────────────────────────────────────────────────────┘
```

### 8.4 Save Dialog: Creating a New Profile

Activated with `s` from any view. An inline input appears in the right pane.

```
┌─────────────────────────────────────────────────────────────────────────────┐
│  ◉ PORTAL — Save Profile                                     [Esc] Cancel  │
├──────────────────────────┬──────────────────────────────────────────────────┤
│  Profiles                │  Save Current Configuration                     │
│ ┌──────────────────────┐ │  ┌──────────────────────────────────────────────┐│
│ │ ◉ work-redteam    *  │ │  │                                              ││
│ │ ○ personal-webdev    │ │  │  Profile name:                               ││
│ │ ○ research           │ │  │  ┌──────────────────────────────────────────┐││
│ │ ○ skeleton (base)    │ │  │  │ new-profile-name_                        │││
│ │                       │ │  │  └──────────────────────────────────────────┘││
│ │                       │ │  │                                              ││
│ │                       │ │  │  Description (optional):                    ││
│ │                       │ │  │  ┌──────────────────────────────────────────┐││
│ │                       │ │  │  │                                          │││
│ │                       │ │  │  └──────────────────────────────────────────┘││
│ │                       │ │  │                                              ││
│ │                       │ │  │  Tags (comma-separated, optional):           ││
│ │                       │ │  │  ┌──────────────────────────────────────────┐││
│ │                       │ │  │  │                                          │││
│ │                       │ │  │  └──────────────────────────────────────────┘││
│ │                       │ │  │                                              ││
│ │                       │ │  │  Files to save: 37 (89KB)                  ││
│ │                       │ │  │  Plugins to blueprint: 3                    ││
│ │                       │ │  │                                              ││
│ └──────────────────────┘ │  │  [Enter] Save   [Esc] Cancel                 ││
│                          │  └──────────────────────────────────────────────┘│
└──────────────────────────┴──────────────────────────────────────────────────┘
│  Save new profile │ [Enter] Confirm │ [Esc] Cancel                          │
└─────────────────────────────────────────────────────────────────────────────┘
```

### 8.5 Load Confirmation: Safety Prompt Before Swap

When `Enter` is pressed on a non-active profile, a confirmation overlay appears.

```
┌─────────────────────────────────────────────────────────────────────────────┐
│  ◉ PORTAL — Confirm Load                                     [Esc] Cancel  │
├──────────────────────────┬──────────────────────────────────────────────────┤
│  Profiles                │  ┌──────────────────────────────────────────────┐│
│ ┌──────────────────────┐ │  │  Load profile "personal-webdev"?           ││
│ │ ◉ work-redteam    *  │ │  │                                              ││
│ │ ○ personal-webdev  ? │ │  │  Current: work-redteam (37 files, 89KB)    ││
│ │ ○ research           │ │  │  Target:  personal-webdev (12 files, 31KB) ││
│ │ ○ skeleton (base)    │ │  │                                              ││
│ │                       │ │  │  Changes:                                    ││
│ │                       │ │  │    - 25 files will be removed                ││
│ │                       │ │  │    + 11 files will be added                  ││
│ │                       │ │  │    ~ 2 files will be modified                ││
│ │                       │ │  │                                              ││
│ │                       │ │  │  Plugins:                                    ││
│ │                       │ │  │    - 2 will be removed (superpowers, shield) ││
│ │                       │ │  │    + 1 will be installed (swift-lsp)         ││
│ │                       │ │  │                                              ││
│ │                       │ │  │  Backup will be created before swap.         ││
│ │                       │ │  │                                              ││
│ └──────────────────────┘ │  │  [y] Load   [d] Dry-run first   [Esc] Cancel ││
│                          │  └──────────────────────────────────────────────┘│
└──────────────────────────┴──────────────────────────────────────────────────┘
│  Load: personal-webdev │ [y] Confirm │ [d] Dry-run │ [Esc] Cancel          │
└─────────────────────────────────────────────────────────────────────────────┘
```

### 8.6 Key Bindings — Complete Reference

| Key      | Context                         | Action                                  |
|----------|----------------------------------|-----------------------------------------|
| `up/down`| Left pane                        | Navigate profiles                       |
| `Enter`  | Left pane (inactive profile)     | Show load confirmation                  |
| `Enter`  | Left pane (active profile)       | Refresh detail view                     |
| `Enter`  | Right pane (diff file list)      | Open content diff view                  |
| `d`      | Left pane                        | Toggle diff mode (selected vs active)   |
| `Tab`    | Diff mode                        | Switch comparison target to skeleton     |
| `s`      | Any view                         | Open save dialog                        |
| `e`      | Left pane                        | Export selected profile                 |
| `x`      | Left pane                        | Delete profile (confirm inline)         |
| `u`      | Any view                         | Undo last load/reset                    |
| `y`      | Confirmation                     | Confirm operation                       |
| `n/Esc`  | Confirmation / Modal             | Cancel / Go back                        |
| `j/k`    | Content diff                     | Scroll                                  |
| `n/N`    | Content diff                     | Next/previous diff hunk                 |
| `?`      | Any view                         | Help overlay                            |
| `q`      | Main view                        | Quit (confirmation if operation pending)|
| `r`      | Left pane (skeleton selected)    | Reset to skeleton                       |

---

## 9. Plugin Lifecycle

Plugins are the trickiest part of profile management because they involve external code, marketplace state, and installation processes that can fail. Portal handles plugins through a **blueprint** model rather than copying plugin code directly.

### 9.1 Save: Blueprint Creation

When `portal save` runs, it:

1. Reads `settings.json` → extracts `enabledPlugins` and `extraKnownMarketplaces`
2. Reads `plugins/installed_plugins.json` → gets installation details
3. For each installed plugin, determines its source type:

```
┌───────────────────────────────────────────────────────────────────────────┐
│                     Plugin Blueprint Extraction                          │
├───────────────────────────────────────────────────────────────────────────┤
│                                                                           │
│  settings.json                                                            │
│  ┌─────────────────────────────────────────┐                             │
│  │ "enabledPlugins": {                     │    ┌──────────────────────┐ │
│  │   "claude-hud@claude-hud": true,        │───>│  marketplace plugin  │ │
│  │   "superpowers@...": true,              │───>│  marketplace plugin  │ │
│  │   "shield@shield-security": true        │───>│  local plugin        │ │
│  │ }                                       │    └──────────────────────┘ │
│  └─────────────────────────────────────────┘                             │
│                                                                           │
│  extraKnownMarketplaces                                                   │
│  ┌─────────────────────────────────────────┐                             │
│  │ "claude-hud": { source: github, ... },  │    Maps plugin IDs to       │
│  │ "superpowers-marketplace": { ... },     │───> their install sources   │
│  │ "shield-security": { source: directory }│                             │
│  └─────────────────────────────────────────┘                             │
│                                                                           │
│  installed_plugins.json                                                   │
│  ┌─────────────────────────────────────────┐                             │
│  │ [ { "id": "claude-hud@...", ... },      │───> Version info, install   │
│  │   { "id": "superpowers@...", ... },     │     timestamps for metadata │
│  │   { "id": "shield@...", ... } ]         │                             │
│  └─────────────────────────────────────────┘                             │
│                                                                           │
│  Output: plugins.json (see Section 6.2)                                   │
│                                                                           │
│  Excluded from file copy:                                                 │
│    plugins/cache/          ← auto-managed, rebuilt on install             │
│    plugins/marketplaces/   ← auto-managed, rebuilt on install             │
│    plugins/data/           ← auto-managed, rebuilt on install             │
│    plugins/blocklist.json  ← user preference, not profile-specific        │
│    plugins/install-counts-cache.json  ← auto-managed                      │
│                                                                           │
│  Included in file copy:                                                   │
│    plugins/installed_plugins.json  ← needed for version tracking          │
│    plugins/config.json            ← global plugin config                  │
│                                                                           │
└───────────────────────────────────────────────────────────────────────────┘
```

### 9.2 Load: Plugin Reinstallation

After the atomic swap completes (step 5 in the flow), Portal reinstalls plugins:

```
┌──────────────────────────────────────────────────────────────────────────┐
│                     Plugin Reinstallation Flow                           │
├──────────────────────────────────────────────────────────────────────────┤
│                                                                          │
│  1. Read plugins.json from profile                                       │
│     │                                                                    │
│     ▼                                                                    │
│  2. For each marketplace plugin:                                         │
│     a. Verify marketplace is registered in settings.json                 │
│        - If not: register extraKnownMarketplaces from blueprint          │
│     b. Run: claude plugin install <plugin-id>                            │
│     c. Verify installation succeeded                                     │
│        - If fail: LOG + continue (non-fatal)                             │
│     │                                                                    │
│     ▼                                                                    │
│  3. For each local plugin:                                               │
│     a. Verify source path still exists                                   │
│        - If not: WARN "Local plugin source missing: <path>"             │
│        - If path exists: run claude plugin install from path             │
│     b. Verify installation succeeded                                     │
│     │                                                                    │
│     ▼                                                                    │
│  4. For each github plugin:                                              │
│     a. Verify git is available                                           │
│     b. Clone repo to tempdir                                             │
│     c. Run claude plugin install from cloned dir                         │
│     d. Clean up tempdir                                                  │
│     │                                                                    │
│     ▼                                                                    │
│  5. Verify all plugins are in enabledPlugins in settings.json            │
│     - Re-enable any that got disabled during install                     │
│     │                                                                    │
│     ▼                                                                    │
│  6. Report results:                                                      │
│     ✓ claude-hud installed (v1.2.3)                                      │
│     ✓ superpowers installed (v0.5.0)                                     │
│     ✓ shield-security installed (local)                                  │
│     ✗ swift-lsp failed (marketplace unavailable)                         │
│                                                                          │
│  Non-fatal failures:                                                     │
│    - Plugin install failures do NOT roll back the profile load           │
│    - The file configuration is always applied successfully               │
│    - Failed plugins are reported and can be retried manually             │
│    - Run: portal verify --fix-plugins                                    │
│                                                                          │
└──────────────────────────────────────────────────────────────────────────┘
```

### 9.3 Plugin Safety Guarantees

| Concern                          | Guarantee                                          |
|----------------------------------|-----------------------------------------------------|
| Plugin install fails             | Profile files still loaded; plugins can be retried  |
| Local plugin path gone           | WARN at load time; profile still loaded without it  |
| Marketplace unavailable          | WARN at load time; retry later                      |
| Plugin version changed           | Installs latest available (version is not pinned)   |
| Switching back to old profile    | Old profile's plugins reinstalled from its blueprint|
| Plugin state after undo          | Backup contains full plugins/ dir; restored as-is   |

### 9.4 Plugin Diff

When diffing profiles, plugins are compared at the blueprint level:

```
$ portal diff work-redteam personal-webdev --plugins

Plugins comparison:
  Shared (3):         claude-hud (marketplace)
  Only in work-redteam (2):
    + superpowers@claude-plugins-official (marketplace)
    + shield@shield-security (local: /Users/rohit/Documents/shield-claude-skill)
  Only in personal-webdev (1):
    + swift-lsp@claude-plugins-official (marketplace)
```

---

## 10. Safety Model

Safety is the #1 priority. A broken `.claude/` means a broken Claude Code. The safety model is defense-in-depth with 5 layers.

### Layer 1: Pre-Flight Checks

Before **any** write operation:

```
CHECK 1: Is Claude Code running?
  → Scan for `claude` process (pgrep)
  → If running: REFUSE. "Claude is running. Close all sessions first."
  → Rationale: Claude reads settings at startup and caches them.
    Overwriting mid-session causes undefined behavior.

CHECK 2: Is ~/.claude/ a valid directory?
  → Verify it exists and contains at least settings.json
  → If not: WARN and offer to create skeleton first

CHECK 3: Is the target profile valid?
  → Verify portal.json exists and checksums match
  → If checksum mismatch: REFUSE. "Profile integrity check failed."

CHECK 4: Is there enough disk space?
  → Estimate size of operation (profile + backup)
  → If <2x free space: WARN

CHECK 5: Is the operation reversible?
  → Verify backup can be created
  → If --no-backup: Require --force + explicit "I understand this is irreversible"
```

### Layer 2: Automatic Backups

Every mutating operation (`load`, `reset`) creates a backup **before** any changes:

- Backup location: `~/.portal/backups/pre-<op>-<ISO-timestamp>.tar.zst`
- Compression: zstd (fast, good ratio)
- Retention: Last 10 backups (configurable)
- Backup contents: **Full** `~/.claude/` (excluding ephemeral dirs) — not just the delta
- Backup verification: Checksum of archive stored in `~/.portal/backups/manifest.json`

### Layer 3: Atomic Swap

The actual `.claude/` replacement uses the filesystem-level atomic rename pattern:

```rust
// 1. Build target in tempdir
let tempdir = tempfile::tempdir_in(home_dir)?;  // same filesystem
build_skeleton_into(&tempdir)?;
overlay_profile_into(&profile, &tempdir)?;
verify_checksums(&profile, &tempdir)?;

// 2. Rename old ~/.claude/ out of the way
rename("~/.claude", "~/.claude.portal-old")?;

// 3. Rename tempdir into place
rename(tempdir.path(), "~/.claude")?;

// 4. Remove old (only after successful swap)
remove_dir_all("~/.claude.portal-old")?;

// 5. Reinstall plugins (after swap, non-fatal)
reinstall_plugins(&profile.plugins)?;
```

If step 3 fails, step 2 can be reversed by renaming `~/.claude.portal-old` back to `~/.claude`. The window where neither directory is at `~/.claude` is a single `rename(2)` syscall — effectively instantaneous.

### Layer 4: Checksum Verification

Every file in a profile has a SHA-256 checksum stored in `portal.json`. These are verified at:

- **Save time**: After copying files into profile storage, re-read and verify
- **Load time**: After building target in tempdir, verify all checksums before swap
- **Diff time**: Verify source profile integrity before comparing
- **Startup**: Quick integrity check of active profile (optional, `--verify` flag)

If any checksum fails:
```
ERROR: Integrity check failed for profile "work-redteam"
  File: skills/autoagent/SKILL.md
  Expected: sha256:a1b2c3d4...
  Actual:   sha256:5e6f7g8h...
  This profile may be corrupted. Refusing to proceed.
  Run: portal verify work-redteam --repair
```

### Layer 5: Dry-Run Mode

Every mutating command supports `--dry-run`:

```
$ portal load work-redteam --dry-run

DRY RUN — no changes will be made

[✓] Pre-flight: Claude not running
[✓] Pre-flight: Profile "work-redteam" exists (37 files)
[✓] Pre-flight: Checksums valid
[✓] Pre-flight: Disk space sufficient (89KB needed, 47GB free)

Would:
  1. Backup current ~/.claude/ → ~/.portal/backups/pre-load-2026-04-22T23:00:00.tar.zst
  2. Build target from skeleton + profile "work-redteam" (37 files)
  3. Replace ~/.claude/ with built target (atomic swap)
  4. Reinstall 3 plugins: claude-hud, superpowers, shield-security
  5. Update active profile → work-redteam

Current profile: personal-webdev (would be preserved in backup)
```

---

## 11. Diff Engine

The diff engine compares two profiles at three levels:

### Level 1: File Manifest Diff

Compare the `portal.json` manifests of two profiles. Output:

```
Shared files (same content):     2
Shared files (different):       5
Only in work-redteam:          30
Only in personal-webdev:       10
```

### Level 2: Directory Tree Diff

Show which directories/files exist in one but not the other:

```
work-redteam only:
  + rules/behaviors.md
  + skills/autoagent/
  + skills/red-teaming/
  + agents/pr-reviewer.md
  + memory/multi-agent-workflow.md

personal-webdev only:
  + skills/swiftui-pro/
  + skills/helm/
  + plugins/vscode/
```

### Level 3: Content Diff

For shared files with different content, show a unified diff:

```
$ portal diff work-redteam personal-webdev -- CLAUDE.md

--- work-redteam/CLAUDE.md
+++ personal-webdev/CLAUDE.md
@@ -1,5 +1,5 @@
 # CLAUDE.md
 
-Rohit is your creator. Contact: @caretak3r, gudi.k.rohit@gmail.com
+Personal web development setup.
 
-Work style: telegraph; noun-phrases ok; drop grammar; min tokens.
-Tone: Technical, concise, authoritative on established patterns.
+Work style: verbose, descriptive, friendly.
+Tone: Helpful, patient, educational.
```

### Level 4: Plugin Diff

Compare the `plugins.json` blueprints:

```
$ portal diff work-redteam personal-webdev --plugins

Plugins comparison:
  Shared (1):         claude-hud (marketplace)
  Only in work-redteam (2):
    + superpowers@claude-plugins-official (marketplace)
    + shield@shield-security (local: /Users/rohit/Documents/shield-claude-skill)
  Only in personal-webdev (1):
    + swift-lsp@claude-plugins-official (marketplace)
```

### Diff Targets

The diff command supports comparing:

| Command                        | Meaning                                    |
|-------------------------------|--------------------------------------------|
| `portal diff <A>`             | Profile A vs skeleton                      |
| `portal diff <A> <B>`         | Profile A vs Profile B                     |
| `portal diff <A> --active`    | Profile A vs currently active profile       |
| `portal diff --active`        | Active profile vs skeleton                  |
| `portal diff <A> <B> -- <path>` | Content diff for specific file only      |
| `portal diff <A> <B> --plugins`| Plugin blueprint comparison               |

---

## 12. Command Reference

### `portal save [NAME]`

Save the current `~/.claude/` as a profile.

```
$ portal save work-redteam

Saving current ~/.claude/ as profile "work-redteam"...

  Scanning directory...        37 trackable files found
  Extracting plugin blueprint... 3 plugins
  Copying files...             ✓
  Computing checksums...       ✓
  Writing manifest...          ✓
  Writing plugin blueprint...  ✓
  Writing metadata...          ✓

Profile "work-redteam" saved successfully.
  37 files tracked (2 skeleton, 35 user)
  3 plugins blueprinted
  Total size: 89KB
```

If profile already exists:
```
$ portal save work-redteam

Profile "work-redteam" already exists.
  [o] Overwrite  [m] Merge (keep new files, update changed)  [c] Cancel
```

### `portal load <NAME>`

Load a profile (replace `~/.claude/`, reinstall plugins).

See Section 5.1 (Atomic Swap Flow) for the full process.

```
$ portal load work-redteam

Loading profile "work-redteam"...
  [✓] Pre-flight: Claude not running
  [✓] Pre-flight: Profile integrity verified (37 files)
  [✓] Pre-flight: Plugin blueprint valid (3 plugins)
  [✓] Backup created: pre-load-2026-04-22T23:00:00.tar.zst
  [✓] Skeleton built
  [✓] Profile overlaid (35 files)
  [✓] Checksums verified
  [✓] Atomic swap complete
  [✓] Plugin: claude-hud installed
  [✓] Plugin: superpowers installed
  [✓] Plugin: shield-security installed (local)
  [✓] Active profile → work-redteam

Profile "work-redteam" loaded successfully.
```

### `portal list`

```
$ portal list

  Profile            Files   Size    Plugins   Tags             Last Used    Active
  ──────────────────────────────────────────────────────────────────────────────────
  work-redteam        37    89KB    3         security,redteam  22:30       ◉
  personal-webdev     12    31KB    2         webdev            18:45       ○
  research            21    54KB    1         research          14:20       ○
  skeleton (base)      2    0.1KB   0         base              —           ○
```

### `portal show <NAME>`

```
$ portal show work-redteam

Profile: work-redteam
  Description: Offensive security + red team workflows
  Tags: security, redteam, work
  Created: 2026-04-22T21:00:00Z
  Last loaded: 2026-04-22T22:30:00Z
  Load count: 14

Files (37 total):
  User files (35):
    CLAUDE.md              16KB    sha256:a1b2c3...
    settings.json         3.2KB    sha256:e5f6g7...
    .claude/settings.local 1.4KB  sha256:q7r8s9...
    rules/behaviors.md    5.9KB    sha256:i9j0k1...
    skills/autoagent/     2.0KB    (1 file)
    skills/red-teaming/   18KB     (19 files)
    agents/               8 items  (8 files)
    commands/             5 items  (5 files)
    memory/               15 items (15 files)
    hooks/                1 item   (1 file)

  Skeleton files (2):
    .claude/hooks/        (empty, matches skeleton)

Delta from skeleton:
  +35 files (89KB added)

Plugins (3):
  claude-hud@claude-hud       marketplace   ✓ enabled
  superpowers@claude-plugins  marketplace   ✓ enabled
  shield@shield-security      local         ✓ enabled
```

### `portal diff <A> [B]`

See Section 11 (Diff Engine) for full details.

### `portal reset`

Reset `~/.claude/` to the skeleton (bare minimum).

```
$ portal reset

This will replace ~/.claude/ with the minimal skeleton.
  Current profile "work-redteam" will be backed up.
  3 plugins will be uninstalled.

  [y] Proceed  [n] Cancel
```

### `portal undo`

Undo the last load/reset by restoring from backup.

```
$ portal undo

Last operation: load "work-redteam" at 2026-04-22T22:30:00Z
  Backup: ~/.portal/backups/pre-load-2026-04-22T22:30:00.tar.zst (31KB)
  Plugins at backup time: 3 (claude-hud, superpowers, shield-security)

  [y] Restore from backup  [n] Cancel
```

Note: `portal undo` restores from the **full backup** (which includes the `plugins/` directory), so plugin state is restored exactly as it was. No reinstallation needed.

### `portal status`

```
$ portal status

Active profile: work-redteam
  Last operation: load (2026-04-22T22:30:00Z)
  Backup available: ~/.portal/backups/pre-load-2026-04-22T22:30:00.tar.zst

Integrity: ✓ All 37 files verified
  CLAUDE.md           ✓ sha256:a1b2c3...
  settings.json       ✓ sha256:e5f6g7...
  ... (35 more)

Plugins: 3/3 healthy
  ✓ claude-hud@claude-hud (marketplace, installed)
  ✓ superpowers@claude-plugins-official (marketplace, installed)
  ✓ shield@shield-security (local, installed)

3 profiles, 2 backups, skeleton OK
```

### `portal verify [NAME]`

Verify profile integrity.

```
$ portal verify work-redteam

Verifying profile "work-redteam"...
  Manifest checksums:  37/37 ✓
  Plugin blueprint:    3/3 ✓
    claude-hud@claude-hud       marketplace source valid
    superpowers@claude-plugins  marketplace source valid
    shield@shield-security      local source exists ✓

Profile "work-redteam" is healthy.

$ portal verify work-redteam --fix-plugins
  Manifest checksums:  37/37 ✓
  Plugin blueprint:    3/3 ✓
  Live plugins:        2/3 installed
    ✗ shield@shield-security not installed — reinstalling...
    ✓ shield@shield-security installed successfully
```

---

## 13. File Manifest — What Portal Manages

### Tracked (Saved in Profiles)

| Path                              | Category    | Notes                              |
|-----------------------------------|------------|-------------------------------------|
| `CLAUDE.md`                       | Core       | Main system prompt                  |
| `settings.json`                   | Core       | Global settings (includes hooks)    |
| `.claude/settings.local.json`     | Core       | Local overrides                     |
| `.claude/hooks/`                  | Hooks      | Hook scripts                        |
| `rules/`                          | Rules      | Behavioral rules                    |
| `skills/*/`                       | Skills     | Skill directories                   |
| `memory/`                         | Memory     | Extended memories                   |
| `MEMORY.md`                       | Memory     | Memory index                        |
| `commands/`                       | Commands   | Slash commands                      |
| `agents/`                         | Agents     | Agent definitions                   |
| `plugins/installed_plugins.json`  | Plugins    | Plugin registry (version tracking)  |
| `plugins/config.json`             | Plugins    | Plugin config                       |
| `.superpowers/`                   | Extensions | Superpowers config                  |

### Blueprint-Only (Not Copied, Reinstalled on Load)

| Path                              | Category    | Notes                                |
|-----------------------------------|------------|--------------------------------------|
| `settings.json → enabledPlugins`  | Plugins    | Extracted to plugins.json blueprint   |
| `settings.json → extraKnownMarketplaces` | Plugins | Extracted to plugins.json blueprint |
| `plugins/installed_plugins.json`  | Plugins    | Referenced in blueprint for versions  |

### Excluded (Never Saved)

| Path                              | Reason                              |
|-----------------------------------|--------------------------------------|
| `session-env/`                    | Ephemeral session data               |
| `sessions/`                       | Conversation state                   |
| `shell-snapshots/`                | Terminal snapshots                   |
| `history.jsonl`                   | Conversation history (large)         |
| `todos/`                          | Task state (ephemeral)               |
| `file-history/`                   | Claude internal tracking             |
| `telemetry/`                      | Analytics                            |
| `statsig/`                        | Feature flags (auto-managed)         |
| `paste-cache/`                    | Temporary paste data                 |
| `debug/`                          | Debug logs                           |
| `stats-cache.json`                | Statistics cache                     |
| `mcp-needs-auth-cache.json`       | MCP auth cache                       |
| `plans/`                          | Plan state (ephemeral)               |
| `projects/`                       | Project-specific state (auto-gen)    |
| `repositories/`                   | Repository tracking (auto-gen)       |
| `plugins/cache/`                  | Plugin cache (rebuilt on install)    |
| `plugins/marketplaces/`           | Marketplace data (rebuilt on install)|
| `plugins/data/`                   | Plugin data (rebuilt on install)     |
| `plugins/blocklist.json`          | User preference (not profile-specific)|
| `plugins/install-counts-cache.json` | Auto-managed                       |
| `plugins/known_marketplaces.json` | Auto-managed                         |
| `.DS_Store`                       | macOS junk                           |

### Configurable Exclusion

Users can add custom exclusions in `~/.portal/portal.exclude`:

```
# Custom exclusions for my setup
memory/today.md        # always changes, not worth tracking
memory/active-tasks.json
plugins/blocklist.json
```

---

## 14. Error Handling & Recovery

### Error Scenarios

| Scenario                        | Detection              | Response                              |
|---------------------------------|------------------------|---------------------------------------|
| Claude is running               | Process check          | REFUSE with clear message             |
| Profile checksum mismatch       | SHA-256 verify         | REFUSE, offer `--repair`              |
| Disk full during swap           | write() error          | Rollback from `.portal-old`           |
| Crash during swap (step 2-3)    | `.portal-old` exists   | Auto-recover on next `portal` run     |
| Corrupted backup                | zstd decompress fail   | WARN, suggest manual repair           |
| Profile not found               | Filesystem check       | ERROR with `portal list` suggestion   |
| Skeleton drift                  | Checksum mismatch      | WARN, offer `portal skeleton sync`    |
| Permission denied               | Filesystem error       | ERROR with chmod suggestion           |
| Plugin install fails            | Exit code != 0         | WARN, profile still loaded, retryable |
| Local plugin source missing     | Path check             | WARN, skip that plugin, continue      |
| Marketplace unreachable         | Install timeout        | WARN, skip, suggest `--fix-plugins`   |

### Crash Recovery

If Portal crashes during the atomic swap (between step 2 and 3):

```
$ portal status

WARNING: Incomplete operation detected!
  ~/.claude.portal-old exists — previous swap may have crashed.
  Current state: 
    - ~/.claude/ may be incomplete or unchanged

  [r] Recover (restore from .portal-old)  [i] Ignore (remove .portal-old)
```

### Plugin Recovery

If plugins fail to install after a successful load:

```
$ portal status

Active profile: work-redteam (loaded 22:30)
  File integrity: ✓ All 37 files verified

  Plugins: 1/3 healthy
    ✓ claude-hud@claude-hud (installed)
    ✗ superpowers@claude-plugins-official (install failed: marketplace timeout)
    ✗ shield@shield-security (local source not found at /path)

  Run: portal verify --fix-plugins
```

### Backup Pruning

Backups are pruned automatically:
- Keep last 10 backups per default
- Configurable via `~/.portal/portal.config.toml`:

```toml
[backup]
max_count = 10
max_age_days = 90
compression = "zstd"    # "zstd" | "gzip" | "none"
compression_level = 3   # zstd: 1-22, gzip: 1-9

[plugins]
reinstall_timeout_secs = 30
retry_failed_on_status = true
```

---

## 15. Implementation Plan

### Phase 0: Project Scaffold (Day 1)

```
portal/
├── Cargo.toml
├── src/
│   ├── main.rs              # CLI entry (clap)
│   ├── cli.rs               # Command definitions
│   ├── tui/
│   │   ├── mod.rs
│   │   ├── app.rs           # TUI application state
│   │   ├── ui.rs            # Rendering (split-pane layout)
│   │   └── event.rs         # Input handling
│   ├── core/
│   │   ├── mod.rs
│   │   ├── profile.rs       # Profile CRUD
│   │   ├── skeleton.rs      # Skeleton management
│   │   ├── snapshot.rs      # Snapshot engine (save)
│   │   ├── loader.rs        # Load engine (atomic swap)
│   │   ├── diff.rs          # Diff engine
│   │   ├── checksum.rs      # SHA-256 verification
│   │   ├── backup.rs        # Backup creation/restoration
│   │   ├── plugins.rs       # Plugin blueprint + reinstall
│   │   └── safety.rs        # Pre-flight checks
│   ├── storage/
│   │   ├── mod.rs
│   │   ├── manifest.rs      # portal.json read/write
│   │   ├── plugins_manifest.rs  # plugins.json read/write
│   │   ├── state.rs         # portal.state.json
│   │   └── paths.rs        # Path resolution
│   └── config.rs            # portal.config.toml
├── tests/
│   ├── integration/
│   │   ├── save_test.rs
│   │   ├── load_test.rs
│   │   ├── diff_test.rs
│   │   ├── safety_test.rs
│   │   └── plugin_test.rs
│   └── fixtures/
│       └── skeleton/        # Test skeleton
└── build.rs
```

### Dependency Stack

```toml
[dependencies]
clap = { version = "4", features = ["derive"] }
ratatui = "0.29"
crossterm = "0.28"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
sha2 = "0.10"
tempfile = "3"
walkdir = "2"
tar = "0.4"
zstd = "0.13"
chrono = { version = "0.4", features = ["serde"] }
toml = "0.8"
dialoguer = "0.11"        # CLI confirmations
console = "0.15"          # CLI styling
indicatif = "0.17"        # Progress bars
similar = "2"             # Diff engine (unified diff)
```

### Phase 1: Core Engine (Days 2-4)

1. Skeleton creation and verification
2. Profile save (snapshot engine + checksum + plugin blueprint extraction)
3. Profile load (atomic swap + backup + plugin reinstallation)
4. State management (`portal.state.json`)
5. Pre-flight safety checks

### Phase 2: Diff Engine (Days 5-6)

1. Manifest diff (Level 1)
2. Directory tree diff (Level 2)
3. Content diff (Level 3) — unified diff for text files
4. Plugin diff (Level 4)
5. CLI diff command

### Phase 3: CLI Polish (Day 7)

1. All CLI commands implemented
2. Dry-run mode
3. Undo/redo
4. Export/import
5. Status + verify commands
6. Error messages + help text

### Phase 4: TUI (Days 8-10)

1. Base TUI framework (ratatui + crossterm)
2. Split-pane layout with profile list (left) + detail (right)
3. Detail view (metadata, files, plugins)
4. Diff mode (right pane transforms to diff view)
5. Content diff view (inline unified diff)
6. Save dialog and load confirmation overlays
7. Key binding handling

### Phase 5: Safety Hardening (Day 11)

1. Crash recovery logic
2. Integrity verification on every operation
3. Plugin install error handling and retry
4. Backup pruning
5. Edge case testing
6. Concurrent access protection (file lock)

### Phase 6: Testing & Release (Days 12-14)

1. Integration tests for all commands
2. TUI test harness (inline snapshot testing)
3. Safety property tests (never lose data invariant)
4. Plugin install/reinstall tests
5. Homebrew formula
6. Cargo release

---

## 16. Non-Goals

- **Not a dotfile manager**: Portal only manages `~/.claude/`, not arbitrary dotfiles
- **Not a sync tool**: No cloud sync, no remote profiles (export/import for manual sharing)
- **Not a Claude plugin**: Portal is a standalone tool, not a Claude Code plugin
- **Not an MCP server**: No MCP integration (could be a future extension)
- **Not managing project-level `.claude/`**: Only the home directory `~/.claude/`
- **Not versioning internal state**: `session-env/`, `sessions/`, etc. are always excluded
- **Not pinning plugin versions**: Plugins are reinstalled at latest available version

---

## 17. Resolved Decisions

| Decision                    | Choice                                               | Rationale                                           |
|-----------------------------|------------------------------------------------------|-----------------------------------------------------|
| TUI design                  | **Option A: Split-Pane**                              | Information-dense, familiar, fast navigation         |
| Scope                       | **Home directory only**                               | Keep it simple; project-level `.claude/` is separate concern |
| Plugin handling             | **Blueprint model: extract + reinstall on load**      | Marketplace plugins change frequently; reinstalling from source ensures freshness. Full backup includes plugin code for undo. |
| Merge on save overwrite     | **Overwrite replaces; merge is explicit opt-in**       | Safer default; merge can lose track of removed files |
| Skeleton versioning         | **Static + manual sync command**                      | Auto-updating risks breaking existing profiles; manual `portal skeleton sync` is safer |
| File lock                   | **Yes: `.portal.lock`**                               | Prevent concurrent operations; simple and effective  |
| Color scheme                | **Respect terminal theme**                            | No forced colors; use terminal's color palette       |

---

## Appendix A: Sample Session

```
$ portal save work-redteam
Saving current ~/.claude/ as profile "work-redteam"...
  37 files tracked (2 skeleton, 35 user)  89KB
  3 plugins blueprinted (claude-hud, superpowers, shield-security)
✓ Profile "work-redteam" saved

$ portal save personal-webdev
Saving current ~/.claude/ as profile "personal-webdev"...
  12 files tracked (1 skeleton, 11 user)  31KB
  2 plugins blueprinted (claude-hud, swift-lsp)
✓ Profile "personal-webdev" saved

$ portal load personal-webdev
Loading profile "personal-webdev"...
  [✓] Claude not running
  [✓] Profile integrity verified
  [✓] Backup created: pre-load-2026-04-22T23:00:00.tar.zst
  [✓] Skeleton built
  [✓] Profile overlaid (11 files)
  [✓] Atomic swap complete
  [✓] Plugin: claude-hud installed
  [✓] Plugin: swift-lsp installed
✓ Profile "personal-webdev" loaded

$ portal diff work-redteam personal-webdev
Shared (same):     1 file
Shared (different): 2 files (CLAUDE.md, settings.json)
Only in work-redteam: 30 files
Only in personal-webdev: 10 files

Plugins:
  Shared: 1 (claude-hud)
  Only in work-redteam: 2 (superpowers, shield-security)
  Only in personal-webdev: 1 (swift-lsp)

$ portal diff work-redteam personal-webdev -- CLAUDE.md
--- work-redteam/CLAUDE.md
+++ personal-webdev/CLAUDE.md
@@ -1,5 +1,5 @@
 ...  (unified diff output)

$ portal undo
Restoring from backup (pre-load-2026-04-22T23:00:00.tar.zst)...
✓ Restored. Active profile: work-redteam
✓ Plugins restored from backup (no reinstall needed)
```

---

## Appendix B: Sequence Diagram — Save Operation

```
User           CLI (clap)         Snapshot Engine       Checksum         Storage        Filesystem
 │                 │                    │                   │               │               │
 │ portal save     │                    │                   │               │               │
 │ "work-redteam"  │                    │                   │               │               │
 │────────────────>│                    │                   │               │               │
 │                 │  save("work-redteam")                  │               │               │
 │                 │───────────────────>│                   │               │               │
 │                 │                    │  scan ~/.claude/  │               │               │
 │                 │                    │──────────────────────────────────────────────────>│
 │                 │                    │  file list        │               │               │
 │                 │                    │<──────────────────────────────────────────────────│
 │                 │                    │  copy files to    │               │               │
 │                 │                    │  profile dir      │               │               │
 │                 │                    │──────────────────────────────────────────────────>│
 │                 │                    │                   │               │               │
 │                 │                    │  checksum each    │               │               │
 │                 │                    │  copied file      │               │               │
 │                 │                    │──────────────────>│               │               │
 │                 │                    │  SHA-256 hashes   │               │               │
 │                 │                    │<──────────────────│               │               │
 │                 │                    │                   │               │               │
 │                 │                    │  extract plugin   │               │               │
 │                 │                    │  blueprint        │               │               │
 │                 │                    │──────────────────────────────────>│               │
 │                 │                    │                   │               │               │
 │                 │                    │  write manifest   │               │               │
 │                 │                    │  (portal.json)    │               │               │
 │                 │                    │──────────────────────────────────>│               │
 │                 │                    │                   │               │               │
 │                 │  ✓ Profile saved   │                   │               │               │
 │                 │<───────────────────│                   │               │               │
 │  ✓ Saved       │                    │                   │               │               │
 │<────────────────│                    │                   │               │               │
```

---

## Appendix C: Sequence Diagram — Load Operation

```
User     CLI      Safety      Backup      Loader      Plugins     Checksum    Filesystem
 │        │         │           │           │           │           │           │
 │ portal │         │           │           │           │           │           │
 │ load   │         │           │           │           │           │           │
 │ "name" │         │           │           │           │           │           │
 │───────>│         │           │           │           │           │           │
 │        │ preflight           │           │           │           │           │
 │        │────────>│           │           │           │           │           │
 │        │  ✓ safe │           │           │           │           │           │
 │        │<────────│           │           │           │           │           │
 │        │         │           │           │           │           │           │
 │        │ backup current      │           │           │           │           │
 │        │────────────────────>│           │           │           │           │
 │        │  ✓ backed up       │           │           │           │           │
 │        │<────────────────────│           │           │           │           │
 │        │         │           │           │           │           │           │
 │        │ build + swap        │           │           │           │           │
 │        │────────────────────────────────>│           │           │           │
 │        │         │           │           │ build in  │           │           │
 │        │         │           │           │ tempdir   │           │           │
 │        │         │           │           │──────────────────────────────────>│
 │        │         │           │           │ verify    │           │           │
 │        │         │           │           │ checksums │           │           │
 │        │         │           │           │──────────────────────────────────>│
 │        │         │           │           │ rename    │           │           │
 │        │         │           │           │ old→.old │           │           │
 │        │         │           │           │──────────────────────────────────>│
 │        │         │           │           │ rename    │           │           │
 │        │         │           │           │ temp→live │           │           │
 │        │         │           │           │──────────────────────────────────>│
 │        │         │           │           │ cleanup   │           │           │
 │        │         │           │           │ .old      │           │           │
 │        │         │           │           │──────────────────────────────────>│
 │        │         │           │           │           │           │           │
 │        │ reinstall plugins   │           │           │           │           │
 │        │────────────────────────────────────────────>│           │           │
 │        │         │           │           │  read      │           │           │
 │        │         │           │           │  blueprint│           │           │
 │        │         │           │           │──────────>│           │           │
 │        │         │           │           │  install  │           │           │
 │        │         │           │           │  each     │           │           │
 │        │         │           │           │──────────────────────────────────>│
 │        │         │           │           │  verify   │           │           │
 │        │         │           │           │  installed│           │           │
 │        │         │           │           │──────────────────────────────────>│
 │        │         │           │           │           │           │           │
 │        │ ✓ loaded + plugins  │           │           │           │           │
 │        │<────────────────────────────────────────────│           │           │
 │ ✓ Done │         │           │           │           │           │           │
 │<───────│         │           │           │           │           │           │
```
