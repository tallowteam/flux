# Domain Pitfalls: High-Performance CLI File Transfer Tools

**Domain:** Cross-platform CLI file transfer tool (SMB, SFTP, WebDAV)
**Project:** Flux
**Researched:** 2026-02-16
**Overall Confidence:** HIGH (multiple verified sources)

---

## Critical Pitfalls

Mistakes that cause rewrites, data loss, or major architectural issues.

---

### Pitfall 1: Destructive Sync Operations Without Safeguards

**What goes wrong:** The `--delete` flag (mirroring source to destination) deletes files on the destination that don't exist on the source. If the source is accidentally empty, corrupted, or points to the wrong directory, the destination is wiped clean.

**Why it happens:** Developers implement sync/mirror functionality without building in safeguards, assuming users understand the implications. rsync's `--delete` is notorious for production data loss incidents.

**Consequences:**
- Complete data loss on backup/destination drives
- Corrupted source files overwrite good backup copies
- No recovery possible without separate backup system

**Warning signs:**
- No `--dry-run` equivalent in your implementation
- No confirmation prompts for destructive operations
- No threshold warnings ("About to delete 10,000 files, continue?")
- Missing audit logs of what was deleted

**Prevention:**
1. **Mandatory dry-run output** before any delete operation
2. **Threshold safeguards**: Warn if deleting >X% of destination files
3. **Soft-delete mode**: Move to trash/archive instead of permanent delete
4. **`--backup` and `--backup-dir`** equivalents to preserve overwritten files
5. **Audit logging** of all destructive operations

**Phase to address:** Phase 1 (Core Architecture) - Design delete safety into the sync subsystem from day one

**Sources:**
- [Preventing accidental deletions with rsync](https://ubuntuforums.org/showthread.php?t=519605)
- [Women Who Code - Use Rsync to Protect Against Data Loss](https://womenwhocode.com/blog/use-rsync-to-protect-against-data-loss/)

---

### Pitfall 2: Race Conditions in Parallel File Operations

**What goes wrong:** Multiple parallel writers race on the same output file or directory, causing file corruption where files have correct sizes but incorrect checksums.

**Why it happens:** Parallel transfers are implemented without proper synchronization. Non-atomic seek+write operations allow interference between threads. OS-level buffering with multiple file descriptors causes write ordering issues.

**Consequences:**
- Silent file corruption (correct size, wrong content)
- Intermittent bugs that are hard to reproduce
- Data integrity failures only discovered much later

**Warning signs:**
- Files pass size checks but fail checksum verification
- Bugs appear only under high concurrency or fast I/O
- Issues manifest only with certain storage backends (fast SSDs, NVMe)

**Prevention:**
1. **Atomic write operations** - Use critical sections for shared resources
2. **File locking** during write operations
3. **Unique temp files per transfer** - Write to `file.tmp.{uuid}`, rename on completion
4. **Single-writer principle** - Each file has exactly one writer process
5. **Post-transfer checksum verification** - Always verify, especially for parallel transfers
6. **Directory creation mutex** - Prevent stat-check-to-mkdir race conditions

**Phase to address:** Phase 1 (Core Architecture) - Parallel transfer design must include synchronization primitives

**Sources:**
- [HuggingFace XET race condition bug](https://github.com/huggingface/xet-core/issues/604)
- [Percona XtraBackup race condition](https://bugs.launchpad.net/percona-xtrabackup/+bug/717784)

---

### Pitfall 3: Tokio Async File I/O Anti-Patterns (Rust-Specific)

**What goes wrong:** Tokio's `tokio::fs` uses `spawn_blocking` behind the scenes, not true async I/O. Naive usage creates excessive thread pool overhead, blocking task starvation, and performance worse than synchronous I/O.

**Why it happens:** Developers assume async file I/O is actually async. Most operating systems don't provide async file APIs, so Tokio wraps blocking calls in thread pool workers.

**Consequences:**
- Performance degradation compared to sync I/O
- Thread pool exhaustion under heavy file workloads
- Task starvation when blocking Tokio worker threads
- Flush operations required (unlike `std::fs::File`)

**Warning signs:**
- Many small file operations creating individual `spawn_blocking` calls
- High thread count during file-heavy workloads
- Latency spikes when mixing file I/O with network I/O
- Missing `flush()` calls causing incomplete writes

**Prevention:**
1. **Batch operations** into as few `spawn_blocking` calls as possible
2. **Use `BufWriter`** and flush only when complete
3. **Consider dedicated thread pool** for file I/O separate from Tokio runtime
4. **Manual `std::fs` in `spawn_blocking`** for fine-grained control
5. **Always call `flush()`** - Tokio's file operations return before write completes
6. **Don't use `tokio::fs` for special files** (named pipes, etc.)

**Phase to address:** Phase 1 (Core Architecture) - File I/O strategy must be designed before implementation

**Sources:**
- [Tokio fs documentation](https://docs.rs/tokio/latest/tokio/fs/index)
- [Tokio spawn_blocking performance](https://github.com/tokio-rs/tokio/issues/2926)

---

### Pitfall 4: Cross-Platform Path and Filename Handling

**What goes wrong:** Windows uses backslashes (`\`) and drive letters (`C:\`), Unix uses forward slashes (`/`) and unified root. Filenames are case-insensitive on Windows/macOS but case-sensitive on Linux. Unicode normalization differs between platforms.

**Why it happens:** Testing only on one platform. Hardcoding path separators. Not handling Unicode normalization (precomposed vs decomposed characters).

**Consequences:**
- Files inaccessible or duplicated across platforms
- "UTF8 encoding conflict" errors during sync
- Transfers fail silently or create wrong paths
- Characters garbled in filenames (especially non-ASCII)

**Warning signs:**
- Tests pass on Linux but fail on Windows (or vice versa)
- Accented characters cause "file not found" errors
- Same file syncs repeatedly without changes
- Paths with spaces break on some platforms

**Prevention:**
1. **Use `std::path::Path`** abstractions, never string manipulation for paths
2. **Normalize paths** to canonical form at protocol boundaries
3. **Handle Unicode normalization** (NFC vs NFD) explicitly
4. **Test with edge-case filenames**: spaces, Unicode, long paths, reserved names (CON, PRN on Windows)
5. **Validate filenames** against target filesystem constraints
6. **Use UTF-8 internally**, convert at filesystem boundaries

**Phase to address:** Phase 1 (Core Architecture) - Path abstraction must be foundational

**Sources:**
- [Google Cloud filename encoding problems](https://cloud.google.com/storage/docs/gsutil/addlhelp/Filenameencodingandinteroperabilityproblems)
- [Syncthing UTF-8 conflicts](https://github.com/syncthing/syncthing/issues/6929)
- [Backblaze cross-platform rules](https://www.backblaze.com/blog/10-rules-for-how-to-write-cross-platform-code/)

---

### Pitfall 5: Resume/Partial Transfer Implementation Failures

**What goes wrong:** Resume logic doesn't verify file version consistency. Partial files resume from wrong offset. Source file changes between interruption and resume, creating hybrid corrupted files.

**Why it happens:** Resume implementation checks only file size, not content identity. No ETag/checksum verification before resuming.

**Consequences:**
- Corrupted files containing parts of different versions
- Silent data corruption (no errors, wrong content)
- Users trust incomplete/corrupted files

**Warning signs:**
- Resumed files fail checksum but have correct size
- No mechanism to detect source file changes
- Resume works for some protocols but not others (SCP doesn't support resume)

**Prevention:**
1. **Store resume metadata**: file size, mtime, ETag/checksum, byte offset
2. **Verify source identity** before resuming (compare stored metadata)
3. **Protocol-aware resume**: SCP can't resume, SFTP/HTTP with Range can
4. **Partial file naming**: Use `.partial` extension to mark incomplete files
5. **Checksum verification** after resume completion
6. **Graceful fallback**: If resume impossible, restart from beginning with warning

**Phase to address:** Phase 2 (Protocol Implementation) - Each protocol needs specific resume handling

**Sources:**
- [WinSCP resume documentation](https://winscp.net/eng/docs/resume)
- [Apple WWDC23 - Robust resumable file transfers](https://developer.apple.com/videos/play/wwdc2023/10006/)

---

## Moderate Pitfalls

Mistakes that cause significant bugs or performance issues but are recoverable.

---

### Pitfall 6: Memory Explosion with Large Directory Listings

**What goes wrong:** Loading entire directory listings into memory before starting transfers. With millions of files, memory usage becomes unbounded and the application crashes or hangs.

**Why it happens:** Simpler implementation pattern. Works fine in testing with small directories. rclone has this documented limitation.

**Warning signs:**
- Memory usage spikes when scanning large directories
- OOM kills during initial scan phase
- Long delays before any transfer begins

**Prevention:**
1. **Streaming directory enumeration** - Process entries as they're discovered
2. **Chunked processing** - Work in batches of N files
3. **Lazy loading** - Only load metadata when needed
4. **Memory budgets** - Cap maximum metadata memory, spill to disk if exceeded

**Phase to address:** Phase 2 (Transfer Engine) - Streaming architecture for directory operations

**Sources:**
- [rclone bugs - memory usage with many files](https://rclone.org/bugs/)

---

### Pitfall 7: Checksum Verification Performance Bottleneck

**What goes wrong:** Naive checksum implementation adds 60%+ overhead to transfers. SHA256 on 1TB takes 86 minutes on good hardware. Checksums computed serially after transfer completes.

**Why it happens:** Checksum implementation is an afterthought. Single-threaded, non-optimized algorithms. Computing after transfer instead of during.

**Warning signs:**
- "Verifying..." phase takes as long as transfer
- CPU underutilization during verification
- Users skip verification due to slowness

**Prevention:**
1. **Streaming checksums** - Compute while reading/writing, not after
2. **Hardware acceleration** - Use AES-NI, SHA extensions where available
3. **Parallel checksum computation** for multiple files
4. **Chunked checksums** - Verify in parts, can restart from last verified chunk
5. **xxHash for speed** when cryptographic strength isn't needed
6. **Optional verification** - Let users choose speed vs. safety tradeoff

**Phase to address:** Phase 2 (Transfer Engine) - Checksum must be integrated into transfer pipeline

**Sources:**
- [AWS Building scalable checksums](https://aws.amazon.com/blogs/media/building-scalable-checksums/)
- [FIVER - Fast Integrity Verification](https://arxiv.org/pdf/1811.01161)

---

### Pitfall 8: SMB Protocol Version Negotiation Failures

**What goes wrong:** Windows 10/11 disabled SMBv1 by default. Linux Samba clients may default to SMBv1. Protocol version mismatch causes silent connection failures.

**Why it happens:** Testing against single SMB version. Not handling negotiation edge cases. Different defaults across OS versions.

**Consequences:**
- "Connection refused" with no clear error message
- Works on some machines, fails on others
- Security vulnerabilities if falling back to SMBv1

**Warning signs:**
- Intermittent connection failures across different Windows versions
- Works with local Samba, fails with Windows shares (or vice versa)
- Connection works but file operations fail

**Prevention:**
1. **Explicit protocol version configuration** - Don't rely on auto-negotiation defaults
2. **Minimum version enforcement** - Default to SMB2+ for security
3. **Clear error messages** when negotiation fails
4. **Version detection** - Log negotiated version for debugging
5. **Test matrix** - Windows 10, 11, Server 2019/2022, Linux Samba

**Phase to address:** Phase 2 (Protocol Implementation) - SMB-specific

**Sources:**
- [nixCraft - Configure Samba SMB versions](https://www.cyberciti.biz/faq/how-to-configure-samba-to-use-smbv2-and-disable-smbv1-on-linux-or-unix/)
- [Argon Systems - Controlling SMB Dialects](https://argonsys.com/microsoft-cloud/library/controlling-smb-dialects/)

---

### Pitfall 9: Timestamp and Timezone Mishandling

**What goes wrong:** Timestamps not preserved during copy. FAT32 stores local time, NTFS stores UTC. Transfers between timezone-unaware and timezone-aware systems cause sync loops.

**Why it happens:** Different filesystems have different timestamp semantics. Timezone conversion not handled. 2-second FAT32 resolution vs. nanosecond resolution elsewhere.

**Warning signs:**
- Files re-sync on every run despite no changes
- Timestamps differ by exactly N hours (timezone offset)
- Files modified in DST transition periods show wrong times

**Prevention:**
1. **Store and compare in UTC** internally
2. **Handle filesystem timestamp resolution** (FAT32: 2sec, NTFS: 100ns, ext4: 1ns)
3. **Tolerance for timestamp comparison** - Don't flag 1-second differences as changes
4. **Preserve original timestamps** with explicit flags (`-t`, `--times`)
5. **Document timestamp limitations** per protocol/filesystem

**Phase to address:** Phase 2 (Transfer Engine) - Metadata preservation subsystem

**Sources:**
- [How-To Geek - Linux File Timestamps](https://www.howtogeek.com/517098/linux-file-timestamps-explained-atime-mtime-and-ctime/)
- [Microsoft - File Times](https://learn.microsoft.com/en-us/windows/win32/sysinfo/file-times)

---

### Pitfall 10: Resource Exhaustion ("Too Many Open Files")

**What goes wrong:** Parallel transfers open many file handles and network connections simultaneously. System hits `ulimit` for open file descriptors, causing cascading failures.

**Why it happens:** No resource limits in transfer engine. Connections not properly closed/pooled. File handles leaked on error paths.

**Warning signs:**
- "Too many open files" errors under high parallelism
- Works with 4 parallel transfers, fails with 32
- Memory/handle leaks over long-running sessions

**Prevention:**
1. **Connection pooling** - Reuse connections rather than opening new ones
2. **File handle budgeting** - Cap concurrent open files
3. **Explicit cleanup** - Close handles in error paths (RAII in Rust)
4. **Configurable parallelism** with sensible defaults
5. **Resource monitoring** - Warn when approaching limits
6. **Graceful degradation** - Reduce parallelism if hitting limits

**Phase to address:** Phase 2 (Transfer Engine) - Resource management architecture

**Sources:**
- [IT'S FOSS - Too many open files](https://itsfoss.gitlab.io/post/fixing-the-too-many-open-files-error-in-linux/)
- [TheLinuxCode - Resolving too many open files](https://thelinuxcode.com/linux-too-many-open-files-error/)

---

### Pitfall 11: Symbolic Link and Hard Link Handling Inconsistencies

**What goes wrong:** Hard links copied as separate files, doubling disk usage. Symlinks become stale if target paths differ on destination. Cross-platform symlink semantics differ.

**Why it happens:** Links are complex edge cases often deferred. Different tools handle them differently. User expectations vary.

**Warning signs:**
- Backup size larger than source (hard links expanded)
- "File not found" errors for symlinked content
- Symlinks work on Linux, fail on Windows

**Prevention:**
1. **Explicit link handling modes**: follow, copy-as-link, or skip
2. **Document limitations** - Hard links generally can't be preserved across copy
3. **Relative symlink preservation** where possible
4. **Warn on broken symlinks** rather than failing silently
5. **Windows symlink privilege handling** - Requires special permissions

**Phase to address:** Phase 3 (Advanced Features) - Link handling as explicit feature

**Sources:**
- [FreeFileSync Forum - Hard links handling](https://freefilesync.org/forum/viewtopic.php?t=1643)
- [Resilio Sync - Soft links, hard links](https://help.resilio.com/hc/en-us/articles/205504529-Soft-links-hard-links-and-symbolic-links)

---

### Pitfall 12: Sparse File Handling

**What goes wrong:** Sparse files (with holes) are copied as fully-allocated files, expanding from 1GB logical to 100GB physical. Transfers take far longer than expected.

**Why it happens:** Default copy mechanisms fill in holes with actual zeros. Cross-filesystem sparse detection varies.

**Warning signs:**
- Destination uses more space than source
- Copying a 1TB sparse VM image takes hours instead of minutes
- Disk fills unexpectedly during backup

**Prevention:**
1. **Detect sparse files** before copy
2. **Preserve sparseness** with appropriate flags (`--sparse` in rsync/cp)
3. **Protocol support check** - Not all protocols support sparse transfer
4. **User configuration** - Allow forcing sparse or non-sparse behavior
5. **Progress reporting** that accounts for sparse optimization

**Phase to address:** Phase 3 (Advanced Features) - Sparse file support as opt-in feature

**Sources:**
- [Wikipedia - Sparse file](https://en.wikipedia.org/wiki/Sparse_file)
- [ArchWiki - Sparse file](https://wiki.archlinux.org/title/Sparse_file)

---

## Minor Pitfalls

Mistakes that cause UX issues or minor bugs.

---

### Pitfall 13: Inadequate Progress Reporting

**What goes wrong:** No feedback during long operations. Progress bar stuck at 0% or jumping erratically. ETA wildly inaccurate. Users abort transfers thinking they're stuck.

**Prevention:**
1. **Per-file and overall progress** - Both are needed
2. **Accurate ETA** based on recent transfer rate, not overall average
3. **Activity indicator** even when progress percentage can't be calculated
4. **Clear completion/failure states** - Don't leave spinners hanging
5. **Verbose mode** for debugging slow transfers

**Phase to address:** Phase 4 (TUI/UX) - Progress reporting system

**Sources:**
- [Evil Martians - CLI UX progress displays](https://evilmartians.com/chronicles/cli-ux-best-practices-3-patterns-for-improving-progress-displays)
- [Uploadcare - File uploader UX](https://uploadcare.com/blog/file-uploader-ux-best-practices/)

---

### Pitfall 14: Trailing Slash Path Semantics

**What goes wrong:** `rsync source/ dest` vs `rsync source dest` have different meanings (copy contents vs. copy directory). Users accidentally create nested directories or miss copying the parent folder.

**Prevention:**
1. **Normalize path semantics** - Decide on one behavior
2. **Clear documentation** of path interpretation
3. **Warning/confirmation** for potentially confusing paths
4. **`--dry-run` output** that shows actual destination paths

**Phase to address:** Phase 1 (CLI Design) - Decide and document path semantics early

**Sources:**
- [rclone rsync behavior difference](https://github.com/rclone/rclone/issues/3779)

---

### Pitfall 15: ACL/Permission Preservation Across Platforms

**What goes wrong:** POSIX permissions (rwx) don't map to Windows ACLs. Extended attributes lost during copy. Permission errors when restoring backups to different systems.

**Prevention:**
1. **Document what's preserved** per protocol/platform combination
2. **Best-effort preservation** with warnings for unsupported attributes
3. **Explicit flags** for permission handling (`--perms`, `--acls`)
4. **NFSv4 ACLs** as intermediate representation where possible

**Phase to address:** Phase 3 (Advanced Features) - Metadata preservation as explicit feature scope

**Sources:**
- [Imperva - ACL Windows vs Linux](https://www.imperva.com/learn/data-security/access-control-list-acl/)
- [ArchWiki - Access Control Lists](https://wiki.archlinux.org/title/Access_Control_Lists)

---

### Pitfall 16: WebDAV Stateless Protocol Challenges

**What goes wrong:** Each WebDAV command is a separate HTTP request requiring re-authentication. COPY/MOVE operations fail behind reverse proxies. MIME type detection slows directory listing.

**Prevention:**
1. **Connection keep-alive** to reduce authentication overhead
2. **Host header preservation** through proxies
3. **Cached authentication** (within session)
4. **Pre-configured MIME types** to avoid content sniffing

**Phase to address:** Phase 2 (Protocol Implementation) - WebDAV-specific

**Sources:**
- [SFTPCloud - SFTP vs WebDAV](https://sftpcloud.io/learn/sftp/sftp-vs-webdav)
- [SFTPGo WebDAV documentation](https://docs.sftpgo.com/2.6/webdav/)

---

### Pitfall 17: Network Retry Logic Without Jitter

**What goes wrong:** Many clients retry simultaneously after failure (thundering herd). Exponential backoff without jitter causes synchronized retries. Maximum retry without limits causes infinite loops.

**Prevention:**
1. **Exponential backoff with jitter** - Random component spreads retries
2. **Maximum retry count** - Don't retry forever
3. **Backoff ceiling** - Cap maximum delay (30-60 seconds typical)
4. **Distinguish retryable errors** - 404 shouldn't retry, 503 should
5. **Per-operation timeouts** - Don't hang indefinitely

**Phase to address:** Phase 2 (Network Layer) - Retry strategy in connection handling

**Sources:**
- [AWS - Timeouts, retries, backoff with jitter](https://aws.amazon.com/builders-library/timeouts-retries-and-backoff-with-jitter/)
- [Google Cloud - Retry strategy](https://cloud.google.com/storage/docs/retry-strategy)

---

## Phase-Specific Warnings

| Phase | Likely Pitfall | Mitigation |
|-------|----------------|------------|
| **Phase 1: Core Architecture** | Race conditions in parallel design | Design synchronization primitives first; single-writer-per-file principle |
| **Phase 1: Core Architecture** | Tokio file I/O anti-patterns | Batch `spawn_blocking` calls; consider dedicated I/O thread pool |
| **Phase 1: Core Architecture** | Path handling fragility | Use proper path abstractions from day one; test cross-platform early |
| **Phase 2: Protocol Implementation** | SMB version negotiation | Test against Windows 10/11/Server and Linux Samba matrix |
| **Phase 2: Protocol Implementation** | Resume logic corruption | Store and verify file identity metadata before resuming |
| **Phase 2: Transfer Engine** | Memory explosion on large dirs | Implement streaming enumeration, not load-all-then-process |
| **Phase 2: Transfer Engine** | Checksum bottleneck | Integrate checksums into transfer pipeline, not as post-step |
| **Phase 3: Sync Features** | Destructive delete without safeguard | Mandatory dry-run, thresholds, soft-delete option |
| **Phase 3: Advanced Features** | Sparse/symlink edge cases | Explicit handling modes, clear documentation of limitations |
| **Phase 4: TUI/UX** | Progress reporting confusion | Per-file + overall progress; activity indicators; clear completion states |

---

## Anti-Patterns Summary

| Anti-Pattern | What to Do Instead |
|--------------|-------------------|
| "Works on my machine" single-platform testing | CI matrix: Windows, macOS, Linux from Phase 1 |
| Load entire file list into memory | Stream and process incrementally |
| Compute checksum after transfer completes | Stream checksum during transfer |
| Retry immediately on failure | Exponential backoff with jitter |
| `--delete` without confirmation | Dry-run, thresholds, soft-delete |
| Trust file size for resume | Verify ETag/mtime/checksum before resume |
| Assume async file I/O is async | Batch `spawn_blocking`, use `BufWriter`, explicit flush |
| Hardcode path separators | Use `std::path` abstractions |
| Ignore Unicode normalization | Normalize at boundaries, test non-ASCII |
| Unbounded parallelism | Resource budgets, graceful degradation |

---

## Sources

**File Transfer Tool Design:**
- [Progress - 7 Ways File Transfer Can Go Wrong](https://www.progress.com/blogs/7-ways-file-transfer-can-go-wrong)
- [Pure Storage - Rclone vs Rsync](https://blog.purestorage.com/purely-technical/rclone-vs-rsync/)
- [Jeff Geerling - 4x faster with rclone](https://www.jeffgeerling.com/blog/2025/4x-faster-network-file-sync-rclone-vs-rsync/)

**Rust/Tokio:**
- [Tokio fs documentation](https://docs.rs/tokio/latest/tokio/fs/index)
- [Tokio spawn_blocking](https://github.com/tokio-rs/tokio/issues/2926)

**Cross-Platform:**
- [Backblaze - 10 Rules for Cross-Platform Code](https://www.backblaze.com/blog/10-rules-for-how-to-write-cross-platform-code/)
- [Google Cloud - Filename encoding problems](https://cloud.google.com/storage/docs/gsutil/addlhelp/Filenameencodingandinteroperabilityproblems)

**Protocols:**
- [nixCraft - Samba SMB versions](https://www.cyberciti.biz/faq/how-to-configure-samba-to-use-smbv2-and-disable-smbv1-on-linux-or-unix/)
- [WinSCP - Resume transfer documentation](https://winscp.net/eng/docs/resume)

**Performance:**
- [AWS - Timeouts, retries, backoff](https://aws.amazon.com/builders-library/timeouts-retries-and-backoff-with-jitter/)
- [AWS - Building scalable checksums](https://aws.amazon.com/blogs/media/building-scalable-checksums/)

**Data Integrity:**
- [FreeFileSync Forum - Corruption discussions](https://freefilesync.org/forum/viewtopic.php?t=1930)
- [HuggingFace XET race condition](https://github.com/huggingface/xet-core/issues/604)
