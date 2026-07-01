# Project agent memory

This file is the project's committed home for project-intrinsic agent knowledge: build, test, release, architecture, and sharp-edge notes that should travel with the code.

- Add durable project-specific notes here as they are discovered through real work.

## Bind-mode / Projection model (`portal use`)

`portal use <name>` is a second way to activate a profile that does **not** touch
`~/.claude`. It materializes the profile into `~/.config/portal/live/<name>/` (see
`PortalPaths::live_dir`) and launches `claude` with `CLAUDE_CONFIG_DIR` pointed there via
`exec` (process replacement). Swap-mode (`portal load`) stays the primary flow;
`live/<name>` is a rebuildable cache. Core logic lives in `src/core/bind.rs`.

Sharp edges:

- **CAS placement refuses to overwrite.** `cas::place_fresh` → `reflink_copy::reflink_or_copy`
  returns `Err(AlreadyExists)` when the destination exists (it does *not* fall back to an
  overwriting copy on EEXIST). The swap loader avoids this by always building into a fresh
  temp dir. Bind-mode reuses the persistent `live/<name>` dir across refreshes, so
  `bind::materialize` must delete every path from the *previous* materialize (recorded in
  `live/<name>/.portal-manifest.json`) before calling `loader::materialize_tracked`.
- **Runtime data is preserved by omission, not by an exclude list.** Only manifest-tracked
  paths are ever written or deleted in `live/<name>`. Session runtime (`projects/`, `todos/`,
  plugin caches) is never in the manifest, so it survives refreshes untouched.
- **Stamps must hash a canonical manifest.** `serde_json` serializes the `files` HashMap in
  nondeterministic order, so the `.portal-stamp` no-op check hashes a *sorted* rendering of
  `(path, checksum, mode)` (`bind::manifest_hash`), not the raw JSON.
