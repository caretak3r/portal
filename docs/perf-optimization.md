# Portal Optimization Matrix — Round 1

Workload: 180-file synthetic ~/.claude (980KB), 5 profiles in CAS, --no-backup load/toggle.
Baseline (release, hyperfine warmup 3 runs 10):
  load p1     35.1 ± 1.4 ms   [user 3.0  sys 31.1]   91% kernel
  toggle      34.9 ± 1.0 ms   [user 3.0  sys 30.8]   88% kernel
  save        10.1 ± 0.4 ms   [user 4.1  sys  5.1]
  status       4.6 ± 0.3 ms
  list         3.9 ± 0.2 ms

Diagnosis: load/toggle are syscall-bound. Wins come from fewer stat/unlink calls.

| # | Lever                                                         | Impact | Conf | Effort | Score |
|---|---------------------------------------------------------------|--------|------|--------|-------|
| 1 | Drop redundant verify_manifest_objects pass                   |   4    |  5   |   1    | 20.0  |
| 2 | Skip unconditional unlink in cas::place                       |   4    |  4   |   2    |  8.0  |
| 3 | Background-thread .portal-old cleanup                         |   3    |  3   |   3    |  3.0  |
| 4 | True parallel CAS placement                                   |   3    |  2   |   2    |  3.0  |
| 5 | Skip skeleton writes overlapping manifest                     |   1    |  4   |   1    |  4.0  |

Threshold: 2.0 — implement in score order, one per commit.

## Round 1 results

| Lever | Files touched | load Δ | toggle Δ | Golden | Tests |
|-------|---------------|--------|----------|--------|-------|
| #1: drop redundant verify_manifest_objects | loader.rs | -1.1 ms | -0.4 ms | MATCH | green |
| #2: cas::place_fresh + pre-clear skeleton overlap | cas.rs, loader.rs, skeleton.rs | -0.6 ms | -1.6 ms | MATCH | green |
| **Cumulative** | | **35.1 → 33.4 ms (-4.8%)** | **34.9 → 32.9 ms (-5.7%)** | | |

System time on load: 31.1 → 29.7 ms. ~360 syscalls eliminated → ~1.7 ms saved → ~5 µs/syscall on APFS.

## Diagnosis for Round 2

Remaining ~30 ms of system time on load now lives in (in rough order):
1. **180 clonefile() syscalls** — the actual work; can't eliminate, but could parallelize
2. **`remove_dir_all(.portal-old)` after swap** — ~180 unlinks + ~10 rmdirs *after* the user-visible swap completed, but still blocking return. Background-thread this and it's free.
3. **`rename .claude → .portal-old` + `rename build → .claude`** — atomic, can't skip
4. **`tempfile::tempdir_in` cleanup at scope exit** — empty by then, cheap
5. **state.json + portal.json writes** — 2 small writes

Next-round matrix candidates:
- #3 Background-thread .portal-old cleanup: Impact 3, Conf 4, Effort 3 → 4.0 (re-scored up after measurement)
- #4 Parallelize clonefile via rayon: Impact 3, Conf 2, Effort 2 → 3.0 (clonefile is metadata-only, may not benefit from parallelism on APFS)
- #5 Skip skeleton writes for files in manifest: Impact 1, Conf 5, Effort 1 → 5.0 (eliminates 3 unneeded writes; pairs naturally with #2's pre-clear logic)

## Round 2 results — null round

All four candidate levers failed to deliver measurable improvement. Documenting here so the next attempt doesn't re-run them.

| Lever | Status | Why |
|-------|--------|-----|
| R2-A: background-thread `.portal-old` cleanup post-swap | **Reverted, regression** | Process exit kills the spawned thread mid-cleanup, leaving partial `.portal-old`. Next load's preflight then has to do a synchronous remove_dir_all of the leftover before its own swap — cost moves from end-of-run-N to start-of-run-(N+1), plus we pay ~50 µs `thread::spawn` overhead. Hyperfine: load 33.4 → 37.2 ms (+11%). |
| R2-B: skip skeleton writes for files in manifest | **Reverted, wash** | Theoretically saves 3 file writes + 3 unlinks (~30 µs). Measured delta was within noise floor (1-2 ms either direction depending on machine state). Skill principle: optimization theatre not allowed. Refactor would still be defensible for code clarity but not as a perf change. |
| R2-C: parallelize CAS placement with rayon | **Reverted, wash** | clonefile on APFS is ~10-20 µs metadata-only. 180 of them serialized ≈ 2-4 ms. Rayon's threadpool warmup + atomic counter overhead absorbs whatever speedup parallelism provides. Original author's comment ("rayon's work-stealing overhead dominates per-file work this small") confirmed by measurement. |
| R2-D: skip post-swap cleanup, defer to next run's preflight | **Killed in design** | `.claude.portal-old` is the **crash-recovery sentinel**: `status`, `recover`, and `safety::preflight_load` all check for it to detect a crashed swap. Leaving it persistent across loads breaks all three. Cannot defer cleanup beyond the current process without a wholesale redesign of crash detection (e.g., separate `.portal.swap-in-progress` marker file). |

## Architectural ceiling

The remaining ~30 ms of system time on load is split (estimated) between:

- **~10 ms post-swap `remove_dir_all(.portal-old)`** — load-bearing for crash detection. To shorten without breaking semantics: redesign crash detection to use a dedicated marker (cheap to remove) and let `.portal-old` cleanup happen in a daemon or via a GC command. Substantial change.
- **~5 ms 180-clonefile loop** — APFS metadata-only physics. Parallelism is a wash on this size; would scale better at >1k files (worth re-running R2-C on a pathological corpus).
- **~3 ms two atomic renames** — irreducible; load IS the swap.
- **~2 ms state.json + portal.json writes** — unavoidable bookkeeping.

Portal's load path is at roughly 85% of its architectural limit on a 180-file corpus. Going below ~30 ms requires changing the crash-recovery design, not micro-optimization.

## Round 3 candidates (out of scope for this skill)

If load latency genuinely matters (vs. e.g. plugin reinstall, which is the bigger user-visible cost on real systems):

1. Replace `.portal-old` crash sentinel with `~/.config/portal/.swap-in-progress` (zero-byte marker). This decouples crash detection from cleanup and unlocks lazy/deferred GC of `.portal-old`.
2. With (1) in place, retry R2-A — `.portal-old` cleanup is no longer load-bearing, so a best-effort background thread that races process exit is acceptable.
3. Or sidestep entirely: introduce a `portal-daemon` (LaunchAgent on macOS, systemd user unit on Linux) that handles GC, plugin diffing, and cache warmth. CLI becomes near-instant for the user, daemon eats the cleanup latency in the background.

These are architecture changes, not optimization passes.

## Real-corpus validation

Synthetic corpus (180 files / 980 KB) is useful for tight measurement but underrepresents the floor. Re-ran against a `cp -cR` clone of a real `~/.claude` (63,815 raw files / 1.5 GB; 3,126 trackable / 58.7 MB after `EXCLUDED_PATTERNS`). Original `~/.claude` untouched throughout.

Pre-R1 (`bb05b70`) vs R1 (`6515194` HEAD), hyperfine warmup 2 / runs 10:

| Command | Pre-R1 | R1 HEAD | Δ |
|---------|--------|---------|---|
| load real-base (3,126 files, --no-backup) | 563.3 ± 10.0 ms | 551.8 ± 8.2 ms | -2.0% (-11.5 ms) |
| toggle | 569.3 ± 4.4 ms | 560.6 ± 14.1 ms | -1.5% (-8.7 ms) |
| save (warm) | — | 231.9 ± 1.9 ms | CPU-bound, 151 ms user (sha256) |
| status | — | 8.6 ± 0.2 ms | scales with file count |
| list | — | 5.3 ± 0.2 ms | constant |
| load WITH backup | — | 0.79 s | +240 ms for tar.zst (58 MB → 11 MB) |
| load WITH plugins (delta no-op) | — | 1.18 s | 4 plugins "already current" — diff path skips reinstall |

**Scaling observation.** R1's relative win shrinks at scale: ~5% on 180-file synthetic, ~2% on 3,126-file real. R1 saves ~360 syscalls per load on synthetic, ~6,250 on real, but the architectural floor (clonefile loop + post-swap `remove_dir_all`) grows linearly faster than fixed-overhead syscall savings can keep up with. Direction is consistent — R1 helps every corpus size.

**Architectural ceiling translates upward.** The ~30 ms ceiling on synthetic becomes ~520 ms on real. Almost all of that is irreducible without redesigning crash-recovery (see Round 3 candidates above).

## Pre-existing bugs surfaced during validation

Not introduced by this work, not in scope, but worth flagging:

1. **`portal verify <profile>` is broken for CAS-mode profiles.** Verify expects content under `<profile>/files/<rel>` but CAS-mode profiles store hashes only — actual bytes live in `objects/<sha>/<rest>`. Result: every file reports `(expected sha256:..., got <missing>)` and verify exits non-zero. The same root cause produces `status`'s `Integrity: ✗ N file(s) differ` line.
2. **`portal undo` after a `--no-backup` load** errors with `Backup for last operation not found: .../backups/no-backup-skipped`. The state file records the sentinel path verbatim and `undo` doesn't recognize the sentinel. Should fail more cleanly with "previous load used --no-backup, nothing to undo".

## Quality gates passed (R1 HEAD)

- `cargo test --release`: 85 passes, 0 failures across 20 test suites
- `cargo fmt --check`: clean
- `cargo clippy --release --bins -- -D warnings`: clean
- Real-corpus golden checksum stable across multiple loads
- Original `~/.claude` byte-identical pre/post session
