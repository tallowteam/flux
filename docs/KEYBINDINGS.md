# Flux Keyboard Shortcuts & Keybindings

Complete reference for every keyboard shortcut in Flux — the interactive TUI, CLI flags, and command shortcuts.

---

## Table of Contents

- [TUI Mode](#tui-mode)
  - [Global Keys](#global-keys)
  - [Dashboard Tab](#dashboard-tab)
  - [File Browser Tab](#file-browser-tab)
  - [Queue Tab](#queue-tab)
  - [History Tab](#history-tab)
- [CLI Shortcuts](#cli-shortcuts)
  - [Global Flags](#global-flags)
  - [Copy Command](#copy-command-flux-cp)
  - [Send Command](#send-command-flux-send)
  - [Receive Command](#receive-command-flux-receive)
  - [Sync Command](#sync-command-flux-sync)
  - [Queue Command](#queue-command-flux-queue)
  - [History Command](#history-command-flux-history)
  - [Discovery Command](#discovery-command-flux-discover)
  - [Alias Commands](#alias-commands)
  - [Trust Commands](#trust-commands)
  - [Shell Completions](#shell-completions-flux-completions)
- [Interactive Prompts](#interactive-prompts)
- [Quick Reference Card](#quick-reference-card)

---

## TUI Mode

Launch the TUI with `flux ui` or `flux --tui`.

The TUI has four tabs, each with context-specific keybindings. Global keys work everywhere.

### Global Keys

These keys work in every tab, at all times.

| Key | Action | Description |
|-----|--------|-------------|
| `q` | **Quit** | Exit the TUI and return to the terminal |
| `1` | **Dashboard** | Jump directly to the Dashboard tab |
| `2` | **Files** | Jump directly to the File Browser tab |
| `3` | **Queue** | Jump directly to the Queue tab |
| `4` | **History** | Jump directly to the History tab |
| `Tab` | **Next tab** | Cycle forward through tabs (Dashboard → Files → Queue → History → Dashboard) |
| `Shift+Tab` | **Previous tab** | Cycle backward through tabs |
| `?` | **Help** | Toggle help overlay (reserved for future use) |

> [!TIP]
> Number keys are the fastest way to switch tabs. Use `Tab` / `Shift+Tab` when you're navigating sequentially.

---

### Dashboard Tab

The dashboard shows active transfer status with a speed history sparkline.

| Key | Action | Description |
|-----|--------|-------------|
| `j` | **Move down** | Select next transfer in the list |
| `k` | **Move up** | Select previous transfer in the list |
| `Down Arrow` | **Move down** | Same as `j` — select next transfer |
| `Up Arrow` | **Move up** | Same as `k` — select previous transfer |

**Navigation behavior:**

- The list wraps around — pressing `j` on the last item jumps to the first, and `k` on the first item jumps to the last
- The selection is highlighted with a contrasting background color
- The sparkline at the top shows transfer speed over time

**Status bar hint:** `j/k: Navigate · 1-4: Tabs · q: Quit`

---

### File Browser Tab

An interactive directory browser for navigating your filesystem.

#### Navigation

| Key | Action | Description |
|-----|--------|-------------|
| `j` | **Move down** | Move cursor to the next file/directory entry |
| `k` | **Move up** | Move cursor to the previous file/directory entry |
| `Down Arrow` | **Move down** | Same as `j` |
| `Up Arrow` | **Move up** | Same as `k` |
| `Home` | **Jump to top** | Jump to the first entry in the current directory |
| `End` | **Jump to bottom** | Jump to the last entry in the current directory |

#### Directory Operations

| Key | Action | Description |
|-----|--------|-------------|
| `Enter` | **Open / Enter** | Enter the selected directory, or open the selected file |
| `l` | **Open / Enter** | Same as `Enter` — enter directory or open file (vim-style) |
| `Backspace` | **Parent directory** | Navigate up to the parent directory |
| `h` | **Parent directory** | Same as `Backspace` — go to parent (vim-style) |

**Navigation behavior:**

- Directories are listed first, then files
- Each entry shows its name and type indicator
- Entering a directory refreshes the listing
- Going to parent directory preserves no scroll state (starts at top)

**Vim-style navigation:** The file browser supports full vim-like navigation with `h`/`j`/`k`/`l` for left (parent), down, up, right (enter).

**Status bar hint:** `j/k: Navigate · Enter: Open · Bksp: Parent · q: Quit`

---

### Queue Tab

View and manage your transfer queue. Each entry shows its ID, status, source, and destination.

#### Navigation

| Key | Action | Description |
|-----|--------|-------------|
| `j` | **Move down** | Select next queue entry |
| `k` | **Move up** | Select previous queue entry |
| `Down Arrow` | **Move down** | Same as `j` |
| `Up Arrow` | **Move up** | Same as `k` |

#### Transfer Operations

| Key | Action | Description |
|-----|--------|-------------|
| `p` | **Pause** | Pause the currently selected transfer. Only works on running transfers. Status changes to `paused` |
| `r` | **Resume** | Resume the currently selected transfer. Only works on paused transfers. Status changes to `pending` |
| `c` | **Cancel** | Cancel the currently selected transfer. Removes it from active processing. Status changes to `cancelled` |
| `x` | **Clear** | Clear all completed, failed, and cancelled entries from the queue. Running and pending entries are kept |

**Transfer status lifecycle:**

```
pending → running → completed
   │         │
   │         └──→ failed
   │
   └──→ paused (via 'p')
          │
          └──→ pending (via 'r')

Any status → cancelled (via 'c')
Completed/Failed/Cancelled → removed (via 'x')
```

**When operations take effect:**

- `p` (pause) — immediate, transfer stops at next chunk boundary
- `r` (resume) — marks as pending, runs on next `flux queue run`
- `c` (cancel) — immediate, transfer is terminated
- `x` (clear) — immediate, entries are removed from the queue file

**Status bar hint:** `j/k: Navigate · p: Pause · r: Resume · c: Cancel · x: Clear · q: Quit`

---

### History Tab

Browse your transfer history in reverse chronological order (newest first).

| Key | Action | Description |
|-----|--------|-------------|
| `j` | **Scroll down** | Scroll to older history entries |
| `k` | **Scroll up** | Scroll to newer history entries |
| `Down Arrow` | **Scroll down** | Same as `j` |
| `Up Arrow` | **Scroll up** | Same as `k` |

**Each history entry shows:**

- Source and destination paths
- Bytes transferred and file count
- Transfer duration and speed
- Timestamp
- Status (completed / failed) with error message if applicable

**Status bar hint:** `j/k: Navigate · 1-4: Tabs · q: Quit`

---

## CLI Shortcuts

Every CLI flag and its short form, organized by command.

### Global Flags

Available on all commands:

| Long Flag | Short | Type | Description |
|-----------|-------|------|-------------|
| `--verbose` | `-v` | flag | Increase verbosity. Use once for verbose, twice (`-vv`) for trace-level logging |
| `--quiet` | `-q` | flag | Quiet mode — suppress all output except errors |
| `--tui` | — | flag | Launch the interactive TUI instead of running the command |
| `--help` | `-h` | flag | Show help text for the command |
| `--version` | `-V` | flag | Show Flux version |

**Verbosity levels:**

| Flag | Level | What you see |
|------|-------|-------------|
| `-q` | Quiet | Errors only |
| *(none)* | Normal | Progress bars, results, warnings |
| `-v` | Verbose | Debug information, connection details, timing |
| `-vv` | Trace | Everything — raw I/O, protocol messages, internal state |

**Environment override:** Set `RUST_LOG=flux=debug` to override CLI verbosity flags.

---

### Copy Command (`flux cp`)

```
flux cp [FLAGS] <SOURCE> <DEST>
```

| Long Flag | Short | Type | Default | Description |
|-----------|-------|------|---------|-------------|
| `--recursive` | `-r` | flag | off | Copy directories and their contents recursively |
| `--exclude` | — | string | — | Exclude files matching a glob pattern. Can be repeated: `--exclude "*.log" --exclude "*.tmp"` |
| `--include` | — | string | — | Include only files matching a glob pattern. Can be repeated |
| `--chunks` | — | number | `0` | Number of parallel chunks. `0` = auto-detect based on file size. Manual: `--chunks 8` |
| `--verify` | — | flag | off | Verify transfer integrity with BLAKE3 checksums after completion |
| `--compress` | — | flag | off | Enable zstd compression during transfer. Best for text-heavy data |
| `--limit` | — | string | unlimited | Bandwidth limit. Formats: `10MB/s`, `500KB/s`, `1GB/s`, `100B/s` |
| `--resume` | — | flag | off | Resume a previously interrupted transfer using the sidecar manifest |
| `--on-conflict` | — | enum | `ask` | What to do when destination file exists: `overwrite`, `skip`, `rename`, `ask` |
| `--on-error` | — | enum | `retry` | What to do on transfer failure: `retry`, `skip`, `pause` |
| `--dry-run` | — | flag | off | Show what would be copied without actually doing it |

**Source/Destination formats:**

| Format | Protocol | Example |
|--------|----------|---------|
| Local path | Local | `/home/user/file.txt`, `C:\Users\file.txt` |
| Relative path | Local | `./file.txt`, `../backup/` |
| SFTP URI | SFTP | `sftp://user@host/path/to/file` |
| SFTP with port | SFTP | `sftp://user@host:2222/path/` |
| UNC path | SMB | `\\server\share\path\file.txt` |
| SMB URI | SMB | `smb://server/share/path/` |
| HTTP/HTTPS URL | WebDAV | `https://server/webdav/path/` |
| WebDAV URI | WebDAV | `webdav://server/path/` |
| Alias | Any | `nas:documents/file.txt` |

---

### Send Command (`flux send`)

```
flux send [FLAGS] <FILE> <TARGET>
```

| Long Flag | Short | Type | Default | Description |
|-----------|-------|------|---------|-------------|
| `--encrypt` | — | flag | off | Enable end-to-end encryption (X25519 + XChaCha20-Poly1305) |
| `--name` | — | string | hostname | Device name to identify as during the transfer |

**Target formats:**

| Format | Example | Description |
|--------|---------|-------------|
| `@devicename` | `@laptop` | Send to a discovered device by name |
| `host:port` | `192.168.1.5:9741` | Send to a specific host and port |
| `IP address` | `192.168.1.5` | Send to IP (uses default port 9741) |

---

### Receive Command (`flux receive`)

```
flux receive [FLAGS]
```

| Long Flag | Short | Type | Default | Description |
|-----------|-------|------|---------|-------------|
| `--output` | `-o` | string | `.` (current dir) | Directory to save received files into |
| `--port` | `-p` | number | `9741` | TCP port to listen on for incoming transfers |
| `--encrypt` | — | flag | off | Require end-to-end encryption for all incoming connections |
| `--name` | — | string | hostname | Device name to advertise on the network via mDNS |

---

### Sync Command (`flux sync`)

```
flux sync [FLAGS] <SOURCE> <DEST>
```

| Long Flag | Short | Type | Default | Description |
|-----------|-------|------|---------|-------------|
| `--dry-run` | — | flag | off | Preview what would be synced without making changes |
| `--delete` | — | flag | off | Delete files in destination that don't exist in source |
| `--watch` | — | flag | off | Watch source for filesystem changes and sync continuously |
| `--schedule` | — | string | — | Run sync on a cron schedule. Example: `"*/5 * * * *"` (every 5 minutes) |
| `--exclude` | — | string | — | Exclude glob patterns (repeatable) |
| `--include` | — | string | — | Include glob patterns (repeatable) |
| `--verify` | — | flag | off | Verify every synced file with BLAKE3 checksum |
| `--force` | — | flag | off | Force sync even when source directory is empty. Safety override for `--delete` |

**Trailing slash behavior (rsync convention):**

| Source | Behavior | Result |
|--------|----------|--------|
| `src/` | Copy **contents** of src into dest | `dest/file1.txt`, `dest/file2.txt` |
| `src` | Copy **src itself** into dest | `dest/src/file1.txt`, `dest/src/file2.txt` |

**Cron expression format:**

```
┌───────────── minute (0-59)
│ ┌───────────── hour (0-23)
│ │ ┌───────────── day of month (1-31)
│ │ │ ┌───────────── month (1-12)
│ │ │ │ ┌───────────── day of week (0-7, 0 and 7 = Sunday)
│ │ │ │ │
* * * * *
```

| Expression | Meaning |
|------------|---------|
| `*/5 * * * *` | Every 5 minutes |
| `0 * * * *` | Every hour |
| `0 */2 * * *` | Every 2 hours |
| `0 0 * * *` | Daily at midnight |
| `0 0 * * 0` | Weekly on Sunday at midnight |
| `30 2 * * 1-5` | Weekdays at 2:30 AM |

---

### Queue Command (`flux queue`)

```
flux queue [SUBCOMMAND]
```

| Subcommand | Arguments | Flags | Description |
|------------|-----------|-------|-------------|
| *(none)* | — | — | List all queued transfers (same as `list`) |
| `list` | — | — | List all queued transfers with ID, status, source, dest |
| `add` | `<SOURCE> <DEST>` | `-r`, `--verify`, `--compress` | Add a new transfer to the queue |
| `pause` | `<ID>` | — | Pause a running transfer |
| `resume` | `<ID>` | — | Resume a paused transfer |
| `cancel` | `<ID>` | — | Cancel a transfer |
| `run` | — | — | Process all pending transfers sequentially |
| `clear` | — | — | Remove all completed, failed, and cancelled entries |

**Queue Add flags:**

| Flag | Short | Description |
|------|-------|-------------|
| `--recursive` | `-r` | Copy directories recursively |
| `--verify` | — | Verify integrity with BLAKE3 |
| `--compress` | — | Enable zstd compression |

---

### History Command (`flux history`)

```
flux history [FLAGS]
```

| Long Flag | Short | Type | Default | Description |
|-----------|-------|------|---------|-------------|
| `--count` | `-n` | number | `20` | Number of history entries to display |
| `--clear` | — | flag | — | Delete all history entries |

---

### Discovery Command (`flux discover`)

```
flux discover [FLAGS]
```

| Long Flag | Short | Type | Default | Description |
|-----------|-------|------|---------|-------------|
| `--timeout` | `-t` | number | `5` | How many seconds to scan the network for devices |

---

### Alias Commands

**Save an alias:**

```
flux add <NAME> <PATH>
```

| Argument | Description | Examples |
|----------|-------------|---------|
| `<NAME>` | Short name for the alias (2+ chars, alphanumeric + hyphen + underscore) | `nas`, `backup`, `prod-server` |
| `<PATH>` | Full path or URI to associate | `\\server\share`, `sftp://user@host/path` |

**List aliases:**

```
flux alias
```

**Remove an alias:**

```
flux alias rm <NAME>
```

**Using aliases in commands:**

```
flux cp file.txt <alias>:<subpath>
```

The alias is expanded before protocol detection. Example: if `nas` = `\\server\share`, then `nas:docs/file.txt` becomes `\\server\share\docs\file.txt`.

---

### Trust Commands

**List trusted devices:**

```
flux trust
flux trust list
```

**Remove a trusted device:**

```
flux trust rm <NAME>
```

---

### Shell Completions (`flux completions`)

```
flux completions <SHELL>
```

| Shell | Value |
|-------|-------|
| Bash | `bash` |
| Zsh | `zsh` |
| Fish | `fish` |
| PowerShell | `powershell` |
| Elvish | `elvish` |

---

## Interactive Prompts

### Conflict Resolution

When `--on-conflict ask` is set (the default) and a destination file already exists, Flux prompts:

```
path/to/file.txt exists. (o)verwrite / (s)kip / (r)ename?
```

| Input | Action | Description |
|-------|--------|-------------|
| `o` or `overwrite` | **Overwrite** | Replace the existing file with the new one |
| `s` or `skip` | **Skip** | Leave the existing file untouched, move to next file |
| `r` or `rename` | **Rename** | Save as `file_1.txt`, `file_2.txt`, etc. Auto-increments the number |

**Notes:**
- The prompt only appears in interactive terminals (TTY)
- In non-interactive environments (pipes, scripts), Flux defaults to **skip**
- Type just the first letter (`o`, `s`, `r`) and press Enter — no need for the full word

### SFTP Password Prompt

When connecting to an SFTP server without SSH agent or key files:

```
Password for user@host:
```

Type your password and press Enter. The password is hidden (not echoed to the terminal).

---

## Quick Reference Card

Print this out or keep it handy.

### TUI Quick Reference

```
┌─────────────────────────────────────────────────┐
│                  FLUX TUI                       │
├─────────────────────────────────────────────────┤
│                                                 │
│  TABS           1  Dashboard                    │
│                 2  File Browser                  │
│                 3  Queue                         │
│                 4  History                       │
│                 Tab / Shift+Tab  Cycle tabs      │
│                                                 │
│  NAVIGATE       j / Down Arrow   Move down       │
│                 k / Up Arrow     Move up          │
│                 Home             Jump to top      │
│                 End              Jump to bottom   │
│                                                 │
│  FILE BROWSER   Enter / l        Open / Enter dir │
│                 Backspace / h    Parent dir       │
│                                                 │
│  QUEUE          p   Pause selected               │
│                 r   Resume selected              │
│                 c   Cancel selected              │
│                 x   Clear finished               │
│                                                 │
│  GENERAL        q   Quit                         │
│                 ?   Help                         │
│                                                 │
└─────────────────────────────────────────────────┘
```

### CLI Quick Reference

```
┌─────────────────────────────────────────────────┐
│                FLUX CLI                         │
├─────────────────────────────────────────────────┤
│                                                 │
│  TRANSFER       flux cp src dest                │
│                 flux cp -r src/ dest/           │
│                 flux cp --verify --compress f d  │
│                 flux cp --resume large.bin dest/ │
│                 flux cp --limit 10MB/s f dest/  │
│                                                 │
│  PEER-TO-PEER   flux discover                   │
│                 flux send file @device           │
│                 flux send --encrypt file @dev    │
│                 flux receive -o ~/Downloads/     │
│                                                 │
│  SYNC           flux sync src/ dest/            │
│                 flux sync --watch src/ dest/     │
│                 flux sync --schedule "*/5 * * * *" s d │
│                 flux sync --delete --verify s d  │
│                                                 │
│  ALIASES        flux add name path              │
│                 flux alias                      │
│                 flux alias rm name              │
│                                                 │
│  QUEUE          flux queue add src dest         │
│                 flux queue run                  │
│                 flux queue pause/resume/cancel N │
│                                                 │
│  OTHER          flux history                    │
│                 flux trust                      │
│                 flux ui                         │
│                 flux completions bash|zsh|fish  │
│                                                 │
│  FLAGS          -v   Verbose  (-vv = trace)     │
│                 -q   Quiet                      │
│                 -r   Recursive                  │
│                 -h   Help                       │
│                                                 │
└─────────────────────────────────────────────────┘
```

---

<p align="center">
  <a href="../README.md">Back to README</a> · <a href="SETUP.md">Setup Guide</a>
</p>
