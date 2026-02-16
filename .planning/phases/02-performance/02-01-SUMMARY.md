---
phase: 02-performance
plan: 01
subsystem: transfer
tags: [blake3, zstd, rayon, serde_json, bytesize, chunked-io, positional-io]

# Dependency graph
requires:
  - phase: 01-foundation
    provides: "CLI parsing, FluxBackend trait, copy_file_with_progress, FluxError, CpArgs"
provides:
  - "ChunkPlan and TransferPlan types for chunk-based file splitting"
  - "chunk_file() function to split a file into N contiguous chunks"
  - "auto_chunk_count() heuristic for optimal chunk count by file size"
  - "Cross-platform positional I/O: read_at, write_at, read_at_exact, write_at_all"
  - "CLI flags: --chunks, --verify, --compress, --limit, --resume"
  - "FluxError variants: ChecksumMismatch, ResumeError, CompressionError"
  - "Phase 2 dependencies: blake3, zstd, rayon, serde_json, bytesize"
affects: [02-02, 02-03, 03-network]

# Tech tracking
tech-stack:
  added: [blake3 1.8, zstd 0.13, rayon 1.10, serde_json 1.0, bytesize 1.3]
  patterns: [cross-platform positional I/O via cfg(unix)/cfg(windows), chunk planning with remainder absorption, parallelism cap at available_parallelism]

key-files:
  created:
    - src/transfer/chunk.rs
    - src/transfer/parallel.rs
  modified:
    - Cargo.toml
    - Cargo.lock
    - src/cli/args.rs
    - src/error.rs
    - src/main.rs
    - src/transfer/mod.rs

key-decisions:
  - "Chunk remainder absorbed by last chunk (not distributed), matching standard chunking pattern"
  - "auto_chunk_count capped at std::thread::available_parallelism to avoid over-subscribing CPU"
  - "Positional I/O uses cfg(unix)/cfg(windows) with FileExt traits, no Mutex needed for parallel reads"
  - "read_at_exact and write_at_all retry on Interrupted errors, matching std behavior"

patterns-established:
  - "Cross-platform positional I/O: #[cfg(unix)] read_at / #[cfg(windows)] seek_read pattern"
  - "Chunk planning: chunk_file(total_size, count) -> Vec<ChunkPlan> with contiguous byte ranges"
  - "Auto-detection heuristic: tiered file-size thresholds capped at hardware parallelism"

requirements-completed: [PERF-01, PERF-02, PERF-04]

# Metrics
duration: 5min
completed: 2026-02-16
---

# Phase 2 Plan 01: Chunk Infrastructure Summary

**ChunkPlan/TransferPlan types, file chunking algorithm, auto-detection heuristic, cross-platform positional I/O primitives (read_at/write_at), and 5 new CLI flags for Phase 2 features**

## Performance

- **Duration:** 5 min
- **Started:** 2026-02-16T23:11:07Z
- **Completed:** 2026-02-16T23:16:15Z
- **Tasks:** 2
- **Files modified:** 8

## Accomplishments
- Added all Phase 2 crate dependencies (blake3, zstd, rayon, serde_json, bytesize) and verified they compile
- Created ChunkPlan and TransferPlan types with serde serialization, chunk_file() splitting algorithm, and auto_chunk_count() heuristic
- Implemented cross-platform positional I/O (read_at, write_at, read_at_exact, write_at_all) using Windows seek_read/seek_write
- Extended CLI with --chunks, --verify, --compress, --limit, --resume flags and added 3 new FluxError variants
- All 64 tests pass (36 original + 14 chunk + 14 parallel I/O)

## Task Commits

Each task was committed atomically:

1. **Task 1: Add dependencies, CLI flags, and error variants** - `39a5581` (feat)
2. **Task 2: Implement chunk planning and positional I/O primitives** - `9322f98` (feat)

## Files Created/Modified
- `Cargo.toml` - Added blake3, zstd, rayon, serde_json, bytesize dependencies
- `Cargo.lock` - Updated lockfile with 21 new packages
- `src/cli/args.rs` - Extended CpArgs with chunks, verify, compress, limit, resume fields
- `src/error.rs` - Added ChecksumMismatch, ResumeError, CompressionError variants and From<serde_json::Error>
- `src/main.rs` - Extended debug tracing to log new CLI flags
- `src/transfer/mod.rs` - Added chunk and parallel module declarations
- `src/transfer/chunk.rs` - ChunkPlan, TransferPlan, chunk_file(), auto_chunk_count() with 14 tests
- `src/transfer/parallel.rs` - read_at, write_at, read_at_exact, write_at_all with 14 tests

## Decisions Made
- Chunk remainder absorbed by last chunk (not distributed evenly) -- matches standard chunking pattern and simplifies implementation
- auto_chunk_count capped at std::thread::available_parallelism() to prevent over-subscribing CPU cores
- Positional I/O uses cfg(unix)/cfg(windows) with FileExt traits; no Mutex needed since each thread specifies its own offset
- read_at_exact and write_at_all retry on Interrupted errors, matching std Read::read_exact / Write::write_all semantics

## Deviations from Plan

None - plan executed exactly as written.

## Issues Encountered

None.

## User Setup Required

None - no external service configuration required.

## Next Phase Readiness
- ChunkPlan and TransferPlan types ready for parallel copy engine (Plan 02-02)
- Positional I/O primitives (read_at, write_at, read_at_exact, write_at_all) ready for multi-threaded chunk transfers
- CLI flags wired and appearing in --help; behavioral implementation deferred to Plans 02-02 and 02-03
- All Phase 2 crate dependencies compiled and available

## Self-Check: PASSED

- All 7 source files verified present on disk
- Commit 39a5581 (Task 1) verified in git log
- Commit 9322f98 (Task 2) verified in git log
- 64 tests passing (54 unit + 10 integration)

---
*Phase: 02-performance*
*Completed: 2026-02-16*
