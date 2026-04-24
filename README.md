# portal

Configuration profile manager for Claude Code. Save, switch, diff, and restore `~/.claude/` configurations with atomic swap safety.

```
portal save work-redteam       # snapshot current .claude/ as a profile
portal load personal-webdev    # atomic swap to a different profile
portal diff work-redteam personal-webdev   # see what differs
portal export work-redteam     # portable archive for sharing
portal import work-redteam.portal.tar.zst  # import from archive
portal undo                    # restore from automatic backup
portal                         # launch the TUI browser
```

## Why

Claude Code stores everything in `~/.claude/`: system prompts, rules, skills, memory, hooks, plugins, agents, commands. Power users who maintain multiple configurations (red-team, web-dev, research, personal) have no way to switch between them without manually moving files around. One bad edit to `settings.json` and the whole setup breaks with no undo.

Portal treats each configuration as a named profile that can be saved, loaded, and diffed. Every mutating operation creates a backup first. The swap itself is a single `rename(2)` syscall.

## Install

```bash
cargo install --path . --features tui-ratatui
```

Requires Rust 1.85+.

## Commands

```
portal                  Launch the TUI browser
portal save [NAME]      Save current .claude/ as a named profile
portal load <NAME>      Load a profile (atomic swap + auto-backup)
portal list             List all profiles
portal show <NAME>      Show profile details and file manifest
portal diff <A> [B]     Diff two profiles (B defaults to skeleton)
portal rm <NAME>        Delete a profile
portal reset            Reset .claude/ to skeleton (bare minimum)
portal undo             Restore from the last automatic backup
portal status           Show active profile, integrity, and state
portal rename OLD NEW   Rename a profile
portal verify [NAME]    Check profile integrity (SHA-256 checksums)
portal verify --fix-plugins   Verify and reinstall failed plugins
portal export <NAME>    Export a profile to a portable .tar.zst archive
portal import <PATH>    Import a profile from an archive
portal clone SRC NEW    Clone a profile, selectively choosing what to bring
portal recover          Recover from a crashed swap (.claude.portal-old)
```

**Clone examples:**
```bash
portal clone work-redteam new-webdev --only skills,rules
portal clone work-redteam minimal --without memory,hooks,plugins
portal clone work-redteam fresh --only skills --fresh-claude-md
```

**Flags:** `--dry-run`, `--no-plugins`, `--no-backup`, `--force`, `-v`, `-q`

## TUI

Running `portal` without arguments opens a split-pane terminal browser.

```
┌──────────────────────────┬──────────────────────────────────────────────────┐
│  Profiles                │  work-redteam  ● active                          │
│ ┌──────────────────────┐ │                                                  │
│ │ ● work-redteam    *  │ │  Description: Offensive security workflows       │
│ │ ○ personal-webdev    │ │  Tags: security, redteam, work                  │
│ │ ○ research           │ │  Created: 2026-04-22                            │
│ │                       │ │  Load count: 14                                 │
│ │                       │ │                                                  │
│ │                       │ │  Tracked Files (37)                             │
│ │                       │ │    ● CLAUDE.md             16KB                 │
│ │                       │ │    ● settings.json        3.2KB                 │
│ │                       │ │    ● rules/behaviors.md   5.9KB                 │
│ │                       │ │    ● skills/autoagent/    2.0KB                 │
│ │                       │ │    ...                                           │
│ │                       │ │                                                  │
│ │                       │ │  Plugins (3)                                    │
│ │                       │ │    ● claude-hud         marketplace             │
│ │                       │ │    ● superpowers        marketplace             │
│ │                       │ │    ● shield-security    local                   │
│ │                       │ │                                                  │
│ │  * = active            │ │  [Enter] Load  [d] Diff  [x] Delete  [s] Save  │
│ └──────────────────────┘ │                                                  │
└──────────────────────────┴──────────────────────────────────────────────────┘
```

**Keys:** `j/k` navigate, `Enter` load, `d` diff, `s` save current, `x` delete, `?` help, `q` quit.

## How It Works

### Profiles

A profile is a snapshot of `~/.claude/` stored in `~/.config/portal/profiles/<name>/`. It contains the actual files, a manifest (`portal.json`) with SHA-256 checksums for every tracked file, a plugin blueprint (`plugins.json`), and metadata.

Ephemeral directories (`sessions/`, `todos/`, `telemetry/`, `history.jsonl`, etc.) are excluded. Plugin code is not copied; instead, Portal records which plugins are installed and reinstalls them from source on load.

### Atomic Swap

Loading a profile replaces `~/.claude/` through a filesystem-level atomic rename:

```
1. Verify profile checksums
2. Create backup of current ~/.claude/ (tar.zst)
3. Build target in tempdir (skeleton + profile overlay)
4. Verify built checksums
5. rename(~/.claude, ~/.claude.portal-old)
6. rename(tempdir, ~/.claude)             # single syscall, atomic
7. rm ~/.claude.portal-old
8. Reinstall plugins from blueprint
```

If step 6 fails, step 5 is reversed. The window where neither path exists is one syscall. Plugin installation happens after the swap and failures are non-fatal.

### Skeleton

The skeleton is the bare minimum `~/.claude/` that Claude Code needs to function: `settings.json` with defaults, an empty `CLAUDE.md`, and the required directory structure (`skills/`, `memory/`, `commands/`, `agents/`, `rules/`, `hooks/`). Every profile is defined by its delta from this skeleton. Running `portal reset` restores it.

### Diffing

```bash
portal diff work-redteam personal-webdev           # manifest-level comparison
portal diff work-redteam personal-webdev --file CLAUDE.md  # unified text diff
portal diff work-redteam                            # compare against skeleton
```

The diff engine compares at three levels: file manifest (which files exist and checksums), directory tree (what's unique to each side), and file content (unified diff via `similar`).

### Safety

Five layers of protection:

1. **Pre-flight checks** verify Claude Code is not running, the profile exists and passes integrity checks, and disk space is sufficient.
2. **Automatic backups** create a `tar.zst` archive of `~/.claude/` before every load or reset. Last 10 kept by default.
3. **Atomic swap** uses `rename(2)` so the directory is never in a partial state.
4. **SHA-256 checksums** are verified at save time, load time, and on demand via `portal verify`.
5. **File locking** prevents concurrent Portal operations.

Run `portal undo` to restore from the most recent backup.

## Storage Layout

```
~/.config/portal/
  profiles/
    work-redteam/
      portal.json        # manifest with file checksums
      plugins.json       # plugin blueprint
      meta.json          # description, tags
      files/             # actual file contents
    personal-webdev/
    research/
  skeleton/              # reference skeleton
  backups/               # automatic pre-operation backups
  portal.state.json      # current active profile
  portal.config.toml     # optional configuration
```

## Configuration

Optional. Create `~/.config/portal/portal.config.toml`:

```toml
[backup]
max_count = 10
max_age_days = 90
compression_level = 3

[plugins]
reinstall_timeout_secs = 30
```

## Implementation Status

### Core Engine

- [x] Project scaffold, security config (`deny.toml`, `clippy.toml`, `unsafe_code = "forbid"`)
- [x] Data model types (`ProfileManifest`, `PortalState`, `PluginBlueprint`, etc.)
- [x] Path resolution (`PortalPaths` with `detect()` and `with_home()` for testing)
- [x] Storage layer (manifest, state, meta, plugins_manifest read/write)
- [x] SHA-256 checksum engine with file and manifest verification
- [x] Skeleton creation and verification
- [x] Snapshot engine (save with exclusion patterns, segment-based `.git/`/`node_modules` exclusion)
- [x] Plugin blueprint extraction from `settings.json`
- [x] Plugin reinstallation (`claude plugin install`, GitHub clone, local path)
- [x] tar.zst backup engine (create, restore, prune)
- [x] Pre-flight safety checks (Claude running, profile exists, disk state)
- [x] File locking with 300s stale timeout
- [x] Atomic swap loader (10-step pipeline with rollback)
- [x] 4-level diff engine (manifest, tree, content via `similar`, plugins)
- [x] Export/import profiles as portable `.tar.zst` archives
- [x] Crash recovery (`portal recover`)
- [x] Clone profiles with selective category inclusion (`--only`, `--without`, `--fresh-claude-md`)
- [x] Config file support (`portal.config.toml` with defaults)

### CLI

- [x] `save` with interactive prompts, overwrite/merge choice, dry-run
- [x] `load` with atomic swap, backup, plugin reinstall
- [x] `list`, `show`, `diff`, `rm`, `reset`, `undo`
- [x] `status` with integrity check and plugin health
- [x] `rename` with state update
- [x] `verify` with `--fix-plugins`
- [x] `export` / `import`
- [x] `recover`
- [x] `clone` with `--only`, `--without`, `--fresh-claude-md`
- [x] Global flags: `--dry-run`, `--no-backup`, `--no-plugins`, `--force`, `-v`, `-q`

### TUI (two competing implementations)

**Ratatui** (`tui/ratatui` branch, `--features tui-ratatui`):
- [x] Split-pane layout (profile list + detail)
- [x] Collapsible folder tree with `j/k` navigation
- [x] Save dialog, load/delete confirmation modals
- [x] Help overlay
- [ ] Diff mode (right pane shows actual diff)
- [ ] Content diff view (inline unified diff with hunk navigation)
- [ ] Rich load confirmation (file add/remove/modify counts)

**FrankenTUI** (`tui/ftui` branch, `--features tui-ftui`):
- [x] Elm-style `Model` trait architecture (`Msg` enum + `Cmd` returns)
- [x] Split-pane layout with custom color palette
- [x] Save dialog (name + description), load/delete confirmation modals
- [x] Help overlay, status bar
- [ ] Tags field in save dialog
- [ ] Diff mode
- [ ] Content diff view
- [ ] Collapsible folder tree

### Testing

- [x] 50 integration tests (save, load, diff, backup, checksum, skeleton, safety, transport, CLI)
- [ ] TUI snapshot testing
- [ ] Property tests (never-lose-data invariant)
- [ ] Plugin install/reinstall tests (require `claude` binary)

### Release

- [ ] Homebrew formula
- [ ] Cargo publish
- [ ] CI/CD (GitHub Actions)

## License

MIT
