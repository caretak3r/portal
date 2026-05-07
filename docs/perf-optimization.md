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
