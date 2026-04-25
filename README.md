# Portal

Profile manager for Claude Code. Saves, switches, diffs, and restores `~/.claude/` configurations with atomic swap safety.

```
portal save work-redteam       # snapshot current .claude/ as a profile
portal load personal-webdev    # atomic swap to a different config
portal diff work-redteam personal-webdev   # compare two profiles
portal clone work-redteam fresh --only skills,rules --fresh-claude-md
portal export work-redteam     # portable archive for sharing
portal undo                    # restore from automatic backup
portal                         # launch the TUI
```

## Why

Claude Code stores system prompts, rules, skills, memory, hooks, plugins, agents, and commands in `~/.claude/`. If you maintain multiple configurations (red-team, web-dev, research, personal), switching between them means manually moving files around. One bad edit to `settings.json` can break the whole setup with no undo.

Portal stores each configuration as a named profile. Every mutating operation creates a backup first. The swap itself is a single `rename(2)` syscall, so the directory is never in a partial state.

## Install

### Pre-built binaries

Download from the [latest release](https://github.com/caretak3r/portal/releases/latest):

```bash
# Linux (amd64)
curl -fsSL https://github.com/caretak3r/portal/releases/latest/download/portal-linux-amd64.tar.gz | tar xz
sudo mv portal-linux-amd64 /usr/local/bin/portal

# Linux (arm64)
curl -fsSL https://github.com/caretak3r/portal/releases/latest/download/portal-linux-arm64.tar.gz | tar xz
sudo mv portal-linux-arm64 /usr/local/bin/portal

# macOS (Apple Silicon)
curl -fsSL https://github.com/caretak3r/portal/releases/latest/download/portal-darwin-arm64.tar.gz | tar xz
sudo mv portal-darwin-arm64 /usr/local/bin/portal

# macOS (Intel)
curl -fsSL https://github.com/caretak3r/portal/releases/latest/download/portal-darwin-amd64.tar.gz | tar xz
sudo mv portal-darwin-amd64 /usr/local/bin/portal
```

### From source

```bash
cargo install --path .
# or with the TUI:
cargo install --path . --features tui-ratatui
```

Requires Rust 1.85+.

## What Gets Tracked

Portal snapshots these files and directories from `~/.claude/`:

| Category | Files |
|----------|-------|
| System prompt | `CLAUDE.md` |
| Settings | `settings.json`, `.claude/settings*` |
| Skills | `skills/` (custom skill directories) |
| Rules | `rules/` (behavioral rules) |
| Memory | `memory/` (persistent memory files) |
| Commands | `commands/` (slash commands) |
| Agents | `agents/` (agent definitions) |
| Hooks | `hooks/` (lifecycle hooks) |
| Plugins | Recorded as a blueprint, reinstalled from source on load |

### Excluded

Ephemeral and generated content is excluded automatically:

- `sessions/`, `todos/`, `telemetry/`, `statsig/`
- `history.jsonl`, `cost_tracker.json`, `crash_reports/`
- `.git/`, `node_modules/`, `__pycache__/`, `.venv/` at any depth within tracked files

Plugin code is not copied into profiles. Portal records which plugins are installed and their sources (marketplace, GitHub, local path), then reinstalls them when loading.

## Commands

### `portal save [NAME]`

Snapshot the current `~/.claude/` directory as a named profile. In interactive mode, prompts for name, description, and tags if omitted. If a profile with that name already exists, offers Overwrite, Merge, or Cancel.

```bash
portal save work-redteam -d "Offensive security workflows" -t security,work
portal save                   # interactive: prompts for name
portal save existing --force  # overwrite without prompting
portal save trial --dry-run   # show what would be saved
```

### `portal load <NAME>`

Replace `~/.claude/` with the named profile using an atomic swap. Creates a backup automatically before swapping. Reinstalls plugins from the profile's blueprint after the swap completes.

```bash
portal load personal-webdev
portal load untested --no-backup --force   # skip backup (dangerous)
portal load minimal --no-plugins           # skip plugin reinstallation
```

### `portal list`

List all saved profiles with their file counts, sizes, and active status.

### `portal show <NAME>`

Print a profile's manifest: description, tags, creation date, load count, tracked files with sizes and checksums, and installed plugins.

### `portal diff <A> [B]`

Compare two profiles. If B is omitted, compares against the skeleton (bare minimum config). Shows identical, modified, added, and removed files. Use `--file` to get a unified text diff of a specific file.

```bash
portal diff work-redteam personal-webdev
portal diff work-redteam personal-webdev --file CLAUDE.md
portal diff work-redteam                 # compare against skeleton
```

### `portal clone <SOURCE> <TARGET>`

Create a new profile by selectively copying from an existing one. Categories can be individually included or excluded. Useful for forking a configuration while dropping memory or starting with a fresh system prompt.

```bash
portal clone work-redteam new-webdev --only skills,rules
portal clone work-redteam minimal --without memory,hooks,plugins
portal clone work-redteam fresh --only skills --fresh-claude-md
portal clone work-redteam fork -d "Experimental fork"
```

**Categories:** `claude-md`, `settings`, `skills`, `rules`, `memory`, `commands`, `agents`, `hooks`, `plugins`

| Flag | Effect |
|------|--------|
| `--only <cats>` | Include only these categories |
| `--without <cats>` | Include everything except these |
| `--fresh-claude-md` | Write an empty `CLAUDE.md` instead of copying the source's |
| `-d <text>` | Description for the new profile |

`--fresh-claude-md` and including `claude-md` in `--only` are mutually exclusive.

### `portal rm <NAME>`

Delete a profile permanently.

### `portal rename <OLD> <NEW>`

Rename a profile. Updates the state file if the renamed profile is active.

### `portal reset`

Replace `~/.claude/` with a clean skeleton: default `settings.json`, empty `CLAUDE.md`, and the required directory structure. Creates a backup first.

### `portal undo`

Restore `~/.claude/` from the most recent automatic backup. Every `load` and `reset` creates a timestamped `tar.zst` backup before making changes.

### `portal status`

Show the active profile, run SHA-256 integrity checks against the manifest, and report plugin health (installed vs expected).

### `portal verify [NAME]`

Check a profile's integrity by comparing stored SHA-256 checksums against actual file contents. Defaults to the active profile if no name is given.

```bash
portal verify                  # check active profile
portal verify work-redteam     # check a specific profile
portal verify --fix-plugins    # also reinstall any missing plugins
```

### `portal export <NAME>`

Pack a profile into a portable `.tar.zst` archive with a `portal-profile/<name>/` prefix. The archive includes the manifest, metadata, plugin blueprint, and all tracked files.

```bash
portal export work-redteam                    # creates work-redteam.portal.tar.zst
portal export work-redteam -o ~/Desktop/      # custom output directory
portal export work-redteam -o custom.tar.zst  # custom filename
```

### `portal import <PATH>`

Import a profile from a `.tar.zst` archive. Validates that the archive contains a `portal-profile/` prefix and a valid manifest before extracting.

```bash
portal import work-redteam.portal.tar.zst
portal import ~/Downloads/colleague-config.portal.tar.zst
```

### `portal recover`

If a previous swap crashed and left a `.claude.portal-old` directory behind, this command lets you keep the current state, roll back to the old state, or cancel.

### Global Flags

| Flag | Effect |
|------|--------|
| `--dry-run` | Show what would happen without making changes |
| `--no-backup` | Skip automatic backup (requires `--force`) |
| `--no-plugins` | Skip plugin reinstallation on load |
| `--force` | Override safety checks and skip interactive prompts |
| `-v, --verbose` | Verbose output |
| `-q, --quiet` | Suppress non-essential output |

## TUI

Running `portal` without arguments launches a split-pane terminal browser. Two implementations exist on separate branches for comparison.

### Ratatui (`tui/ratatui` branch)

Imperative rendering with `ratatui` 0.30 and `crossterm`. Split-pane layout: profile list on the left, detail/diff/dialogs on the right.

```
┌─────────────────────┬─────────────────────────────────────────────┐
│  Profiles           │  work-redteam  ● active                     │
│                     │    Offensive security workflows              │
│  ▸ ● work-redteam   │    created 2026-04-22  loaded 2026-04-24    │
│    ○ personal-webdev │                                             │
│    ○ research        │  Files 37 files, 28.3KB                     │
│                     │  ▸ memory/           12 files, 8.2KB         │
│                     │  ▸ rules/             3 files, 6.1KB         │
│                     │  ▸ skills/            8 files, 4.0KB         │
│                     │    CLAUDE.md                    3.2KB        │
│                     │    settings.json                1.8KB        │
│                     │                                             │
│                     │  j/k navigate  Enter expand  l load         │
│                     │  Tab next profile  d diff  s save  n new    │
└─────────────────────┴─────────────────────────────────────────────┘
```

**Keybindings:**

| Key | Action |
|-----|--------|
| `j/k` | Navigate file tree (detail) or modified files (diff) |
| `Enter` | Expand/collapse folder, or view content diff in diff mode |
| `Tab/S-Tab` | Next/previous profile |
| `l` | Load selected profile |
| `d` | Toggle diff view (selected vs active) |
| `s` | Save current `~/.claude/` as a new profile |
| `n` | New profile (empty or clone from selected) |
| `c` | Clone selected profile (with category picker) |
| `?` | Help overlay |
| `q` | Quit |
| `Esc` | Back / cancel |

**Diff mode** shows a structural comparison with colored file lists: `~` yellow for modified files with size deltas, `+` green for added, `-` red for removed. Press Enter on any modified file to see the unified content diff with syntax-colored hunks.

**Load confirmation** shows change counts against the active profile before swapping: how many files will be modified, added, removed, and unchanged.

**New profile dialog** (`n`) offers a mode toggle between "Empty (fresh start)" and "Clone from selected". In clone mode, nine category checkboxes control what gets copied. The CLAUDE.md category and "Start with empty CLAUDE.md" are mutually exclusive; toggling one disables the other.

### FrankenTUI (`tui/ftui` branch)

Elm-style architecture using `ftui` with a `Model` trait, `Msg` enum, and `Cmd` returns. Same profile management features, different rendering framework. Modal-based dialogs instead of right-pane overlays.

**Additional keys:** `D` delete profile, `r` refresh list.

## How It Works

### Profiles

A profile is a snapshot of `~/.claude/` stored in `~/.config/portal/profiles/<name>/`. Each profile contains:

- `portal.json` — manifest with SHA-256 checksums for every tracked file
- `plugins.json` — plugin blueprint (which plugins, their sources, enabled state)
- `meta.json` — description, tags, author
- `files/` — the actual file contents

### Atomic Swap

Loading a profile replaces `~/.claude/` through a 10-step pipeline:

```
 1. Pre-flight checks (profile exists, checksums valid, disk space)
 2. Acquire file lock (~/.config/portal/portal.lock)
 3. Create tar.zst backup of current ~/.claude/
 4. Build target directory in tempdir (skeleton + profile overlay)
 5. Verify built checksums match manifest
 6. rename(~/.claude/, ~/.claude.portal-old)
 7. rename(tempdir, ~/.claude/)              ← single syscall, atomic
 8. Remove ~/.claude.portal-old
 9. Update portal.state.json (active profile, load count, timestamp)
10. Reinstall plugins from blueprint
```

If step 7 fails, step 6 is reversed. The window where neither path exists is one syscall wide. Plugin installation happens after the swap; failures there are non-fatal and reported.

If the process crashes between steps 6 and 8, `portal recover` detects the leftover `.claude.portal-old` directory and offers rollback.

### Skeleton

The skeleton is the minimum `~/.claude/` that Claude Code needs: `settings.json` with defaults, an empty `CLAUDE.md`, and the required directory structure (`skills/`, `memory/`, `commands/`, `agents/`, `rules/`, `hooks/`). Every profile is defined by its delta from this skeleton. `portal reset` restores it.

### Diffing

The diff engine operates at four levels:

1. **Manifest** — which files exist and their SHA-256 checksums
2. **Tree** — files unique to each side, shared files with identical or different content
3. **Content** — unified text diff via `similar` for individual files
4. **Plugins** — which plugins are only in one side, which changed

### Safety

Five layers of protection:

1. **Pre-flight checks** verify the profile exists, passes integrity checks, and disk space is sufficient.
2. **Automatic backups** create a `tar.zst` archive of `~/.claude/` before every load or reset. Last 10 kept by default, configurable.
3. **Atomic swap** uses `rename(2)` so the directory is never in a partial state.
4. **SHA-256 checksums** are verified at save time, load time, and on demand via `portal verify`.
5. **File locking** prevents concurrent Portal operations. Locks older than 300 seconds are treated as stale.

`portal undo` restores from the most recent backup at any time.

## Storage Layout

```
~/.config/portal/
  profiles/
    work-redteam/
      portal.json        # manifest with file checksums
      plugins.json       # plugin blueprint
      meta.json          # description, tags, author
      files/             # actual file contents
    personal-webdev/
    research/
  skeleton/              # reference skeleton
  backups/               # timestamped tar.zst backups
  portal.state.json      # active profile tracking
  portal.lock            # file lock for concurrent access
  portal.config.toml     # optional configuration
```

## Configuration

Optional. Create `~/.config/portal/portal.config.toml`:

```toml
[backup]
max_count = 10           # keep at most 10 backups
max_age_days = 90        # prune backups older than 90 days
compression_level = 3    # zstd compression (1-22)

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
- [x] Pre-flight safety checks (profile exists, disk state)
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

### TUI (two implementations)

**Ratatui** (`tui/ratatui` branch, `--features tui-ratatui`):
- [x] Split-pane layout (profile list + detail)
- [x] Collapsible folder tree with `j/k` navigation
- [x] Save dialog, load confirmation modals
- [x] Clone dialog (`c`) with category checkboxes
- [x] New profile dialog (`n`) with Empty/CloneFrom mode toggle
- [x] Mutual exclusivity: CLAUDE.md category vs "Start with empty CLAUDE.md"
- [x] Help overlay
- [x] Structural diff mode (colored ~modified/+added/-removed file lists, navigable cursor)
- [x] Content diff view (Enter on modified file shows unified diff with syntax-colored hunks)
- [x] Rich load confirmation (modified/added/removed/unchanged counts vs active)

**FrankenTUI** (`tui/ftui` branch, `--features tui-ftui`):
- [x] Elm-style `Model` trait architecture (`Msg` enum + `Cmd` returns)
- [x] Split-pane layout with custom color palette
- [x] Save dialog (name + description), load/delete confirmation modals
- [x] Clone dialog (`c`) with category checkboxes
- [x] New profile dialog (`n`) with Empty/CloneFrom mode toggle
- [x] Mutual exclusivity: CLAUDE.md category vs "Start with empty CLAUDE.md"
- [x] Help overlay, status bar
- [ ] Tags field in save dialog
- [ ] Diff mode
- [ ] Content diff view
- [ ] Collapsible folder tree

### Testing

- [x] 57 integration tests (save, load, diff, backup, checksum, skeleton, safety, transport, clone, CLI)
- [ ] TUI snapshot testing
- [ ] Property tests (never-lose-data invariant)
- [ ] Plugin install/reinstall tests (require `claude` binary)

### Release

- [ ] Homebrew formula
- [ ] Cargo publish
- [x] CI/CD (GitHub Actions: test on push, cross-platform release on tag)

## License

MIT
