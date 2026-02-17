# Flux Setup Guide

Complete step-by-step installation guide for every platform. Copy and paste the commands — nothing to figure out.

---

## Table of Contents

- [Prerequisites](#prerequisites)
- [Windows](#windows)
- [macOS](#macos)
- [Linux (Ubuntu / Debian)](#linux-ubuntu--debian)
- [Linux (Fedora / RHEL)](#linux-fedora--rhel)
- [Linux (Arch)](#linux-arch)
- [Post-Install Setup](#post-install-setup)
- [Shell Completions](#shell-completions)
- [Verify Installation](#verify-installation)
- [Uninstall](#uninstall)
- [Troubleshooting](#troubleshooting)

---

## Prerequisites

Flux is built from source using the Rust toolchain. You need:

| Requirement | Version | Why |
|-------------|---------|-----|
| **Rust** (rustc + cargo) | 1.75+ | Compiler and build system |
| **Git** | Any | Clone the repository |
| **C compiler** | gcc / clang / MSVC | Native dependencies (OpenSSL, zstd) |
| **Perl** | 5.x (Windows only) | OpenSSL build script |

Don't worry — the steps below install everything you need.

---

## Windows

### Step 1: Install Rust

Open **PowerShell** (not CMD) and run:

```powershell
winget install Rustlang.Rustup
```

Close and reopen PowerShell, then verify:

```powershell
rustc --version
cargo --version
```

You should see version numbers like `rustc 1.xx.x` and `cargo 1.xx.x`.

> [!NOTE]
> If `winget` is not available, download the installer from https://rustup.rs and run it. Choose the default installation options.

### Step 2: Install Build Tools

Flux needs a C compiler and Perl for building native dependencies. Install both:

```powershell
winget install Microsoft.VisualStudio.2022.BuildTools --override "--add Microsoft.VisualStudio.Workload.VCTools --includeRecommended --passive"
```

Wait for the installation to complete (this may take a few minutes).

Install Perl (needed for vendored OpenSSL):

```powershell
winget install StrawberryPerl.StrawberryPerl
```

Close and reopen PowerShell after installing both.

### Step 3: Install Git (if not installed)

```powershell
winget install Git.Git
```

Close and reopen PowerShell.

### Step 4: Clone and Build Flux

```powershell
cd $HOME\Documents
git clone https://github.com/tallowteam/flux.git
cd flux
cargo build --release
```

The build takes a few minutes on first run. When it finishes, your binary is at:

```
target\release\flux.exe
```

### Step 5: Add to PATH

**Option A — Copy to a directory already in PATH:**

```powershell
Copy-Item target\release\flux.exe "$env:USERPROFILE\.cargo\bin\flux.exe"
```

Since Rust's `.cargo\bin` is already in your PATH, this works immediately.

**Option B — Add the build directory to PATH permanently:**

```powershell
# Add to user PATH permanently
[Environment]::SetEnvironmentVariable("Path", $env:Path + ";$HOME\Documents\flux\target\release", "User")
```

Close and reopen PowerShell.

### Step 6: Verify

```powershell
flux --version
```

Expected output:

```
flux 1.0.0
```

### Step 7: Shell Completions (PowerShell)

```powershell
# Generate completions
flux completions powershell >> $PROFILE

# Reload profile
. $PROFILE
```

Now `flux` + `Tab` will autocomplete commands and flags.

> [!TIP]
> If you get an error about `$PROFILE` not existing, create it first:
> ```powershell
> New-Item -Path $PROFILE -ItemType File -Force
> ```

---

## macOS

### Step 1: Install Xcode Command Line Tools

Open **Terminal** and run:

```bash
xcode-select --install
```

A popup will appear — click **Install** and wait for it to finish. This installs `git`, `clang`, and other build tools.

### Step 2: Install Rust

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

When prompted, press **1** and **Enter** for the default installation.

Load Rust into your current shell:

```bash
source "$HOME/.cargo/env"
```

Verify:

```bash
rustc --version
cargo --version
```

### Step 3: Install OpenSSL (for SFTP backend)

```bash
brew install openssl
```

> [!NOTE]
> If you don't have Homebrew installed, install it first:
> ```bash
> /bin/bash -c "$(curl -fsSL https://raw.githubusercontent.com/Homebrew/install/HEAD/install.sh)"
> ```

### Step 4: Clone and Build Flux

```bash
cd ~/Documents
git clone https://github.com/tallowteam/flux.git
cd flux
cargo build --release
```

### Step 5: Add to PATH

```bash
sudo cp target/release/flux /usr/local/bin/
```

Or if you prefer not to use `sudo`:

```bash
cp target/release/flux ~/.cargo/bin/
```

### Step 6: Verify

```bash
flux --version
```

Expected output:

```
flux 1.0.0
```

### Step 7: Shell Completions

**Zsh** (default macOS shell):

```bash
# Create completions directory if it doesn't exist
mkdir -p ~/.zfunc

# Generate completions
flux completions zsh > ~/.zfunc/_flux

# Add to .zshrc (only needed once)
echo 'fpath=(~/.zfunc $fpath)' >> ~/.zshrc
echo 'autoload -Uz compinit && compinit' >> ~/.zshrc

# Reload
source ~/.zshrc
```

**Bash** (if using bash):

```bash
mkdir -p ~/.local/share/bash-completion/completions
flux completions bash > ~/.local/share/bash-completion/completions/flux
source ~/.local/share/bash-completion/completions/flux
```

**Fish**:

```bash
flux completions fish > ~/.config/fish/completions/flux.fish
```

---

## Linux (Ubuntu / Debian)

### Step 1: Install System Dependencies

```bash
sudo apt update
sudo apt install -y build-essential pkg-config libssl-dev git curl
```

This installs `gcc`, `make`, `pkg-config`, OpenSSL headers, `git`, and `curl`.

### Step 2: Install Rust

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

Press **1** and **Enter** for default installation.

Load Rust into your current shell:

```bash
source "$HOME/.cargo/env"
```

Verify:

```bash
rustc --version
cargo --version
```

### Step 3: Clone and Build Flux

```bash
cd ~/Documents
git clone https://github.com/tallowteam/flux.git
cd flux
cargo build --release
```

### Step 4: Add to PATH

```bash
sudo cp target/release/flux /usr/local/bin/
```

Or without `sudo`:

```bash
cp target/release/flux ~/.cargo/bin/
```

### Step 5: Verify

```bash
flux --version
```

### Step 6: Shell Completions

**Bash**:

```bash
sudo mkdir -p /etc/bash_completion.d
flux completions bash | sudo tee /etc/bash_completion.d/flux > /dev/null
source /etc/bash_completion.d/flux
```

**Zsh**:

```bash
mkdir -p ~/.zfunc
flux completions zsh > ~/.zfunc/_flux
echo 'fpath=(~/.zfunc $fpath)' >> ~/.zshrc
echo 'autoload -Uz compinit && compinit' >> ~/.zshrc
source ~/.zshrc
```

**Fish**:

```bash
mkdir -p ~/.config/fish/completions
flux completions fish > ~/.config/fish/completions/flux.fish
```

---

## Linux (Fedora / RHEL)

### Step 1: Install System Dependencies

```bash
sudo dnf groupinstall -y "Development Tools"
sudo dnf install -y openssl-devel pkg-config git curl
```

### Step 2: Install Rust

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

Press **1** and **Enter** for default installation.

```bash
source "$HOME/.cargo/env"
```

### Step 3: Clone and Build Flux

```bash
cd ~/Documents
git clone https://github.com/tallowteam/flux.git
cd flux
cargo build --release
```

### Step 4: Add to PATH

```bash
sudo cp target/release/flux /usr/local/bin/
```

### Step 5: Verify

```bash
flux --version
```

### Step 6: Shell Completions

Follow the same [shell completions](#shell-completions) steps as Ubuntu/Debian above.

---

## Linux (Arch)

### Step 1: Install System Dependencies

```bash
sudo pacman -Syu --noconfirm base-devel openssl git
```

### Step 2: Install Rust

```bash
sudo pacman -S --noconfirm rustup
rustup default stable
```

Verify:

```bash
rustc --version
cargo --version
```

### Step 3: Clone and Build Flux

```bash
cd ~/Documents
git clone https://github.com/tallowteam/flux.git
cd flux
cargo build --release
```

### Step 4: Add to PATH

```bash
sudo cp target/release/flux /usr/local/bin/
```

### Step 5: Verify

```bash
flux --version
```

### Step 6: Shell Completions

Follow the same [shell completions](#shell-completions) steps as Ubuntu/Debian above.

---

## Post-Install Setup

These steps are optional but recommended.

### Set Up Path Aliases

Save paths you use frequently so you don't have to type them every time:

```bash
# Save your NAS
flux add nas '\\192.168.4.3\shared'

# Save an SFTP server
flux add server sftp://user@example.com/home/user

# Save a backup drive
flux add backup /mnt/external-drive/backups

# Now use them anywhere
flux cp file.txt nas:documents/
flux cp -r ./project/ server:projects/
flux sync ~/Documents/ backup:docs/
```

List your aliases:

```bash
flux alias
```

### Configure Defaults

Create the config file to set your preferences:

**Linux / macOS:**

```bash
mkdir -p ~/.config/flux
cat > ~/.config/flux/config.toml << 'EOF'
# Default verbosity: quiet, normal, verbose, trace
verbosity = "normal"

# What to do when destination file exists: overwrite, skip, rename, ask
conflict = "ask"

# What to do on failure: retry, skip, pause
failure = "retry"

# Number of retry attempts
retry_count = 3

# Retry delay in milliseconds (doubles each attempt)
retry_backoff_ms = 1000

# Maximum history entries
history_limit = 1000
EOF
```

**Windows (PowerShell):**

```powershell
$configDir = "$env:APPDATA\flux"
New-Item -Path $configDir -ItemType Directory -Force
@"
# Default verbosity: quiet, normal, verbose, trace
verbosity = "normal"

# What to do when destination file exists: overwrite, skip, rename, ask
conflict = "ask"

# What to do on failure: retry, skip, pause
failure = "retry"

# Number of retry attempts
retry_count = 3

# Retry delay in milliseconds (doubles each attempt)
retry_backoff_ms = 1000

# Maximum history entries
history_limit = 1000
"@ | Out-File -Encoding UTF8 "$configDir\config.toml"
```

### Set Up Peer-to-Peer Discovery

No setup needed — just run `flux receive` on any machine and `flux discover` on another. They'll find each other automatically via mDNS.

To receive files:

```bash
flux receive --name "my-laptop"
```

To discover peers and send:

```bash
flux discover
flux send file.zip @my-laptop
```

For encrypted transfers, add `--encrypt`:

```bash
# Receiver
flux receive --encrypt --name "my-laptop"

# Sender
flux send --encrypt secrets.zip @my-laptop
```

### First-Time Device Identity

Your device identity (encryption key pair) is generated automatically on first use of `send`, `receive`, or `--encrypt`. No manual setup needed.

The key pair is stored at:

| Platform | Location |
|----------|----------|
| Linux | `~/.config/flux/identity.json` |
| macOS | `~/Library/Application Support/flux/identity.json` |
| Windows | `%APPDATA%\flux\identity.json` |

> [!IMPORTANT]
> Your `identity.json` contains your private key. Do not share it or commit it to version control.

---

## Shell Completions

Quick reference for all supported shells:

| Shell | Command |
|-------|---------|
| Bash | `flux completions bash > ~/.local/share/bash-completion/completions/flux` |
| Zsh | `flux completions zsh > ~/.zfunc/_flux` |
| Fish | `flux completions fish > ~/.config/fish/completions/flux.fish` |
| PowerShell | `flux completions powershell >> $PROFILE` |
| Elvish | `flux completions elvish > ~/.config/elvish/lib/flux.elv` |

After generating completions, restart your shell or source the file.

---

## Verify Installation

Run these commands to confirm everything works:

```bash
# Check version
flux --version

# Copy a test file
echo "Hello Flux" > /tmp/test-flux.txt
flux cp /tmp/test-flux.txt /tmp/test-flux-copy.txt

# Verify the copy
flux cp --verify /tmp/test-flux.txt /tmp/test-flux-copy.txt

# Try recursive copy
mkdir -p /tmp/flux-test-dir/sub
echo "file1" > /tmp/flux-test-dir/file1.txt
echo "file2" > /tmp/flux-test-dir/sub/file2.txt
flux cp -r /tmp/flux-test-dir/ /tmp/flux-test-output/

# Check history
flux history

# Launch TUI
flux ui
```

If all commands complete without errors, Flux is installed correctly.

---

## Uninstall

### Remove the binary

**If installed to /usr/local/bin:**

```bash
sudo rm /usr/local/bin/flux
```

**If installed to ~/.cargo/bin:**

```bash
rm ~/.cargo/bin/flux
```

**Windows:**

```powershell
Remove-Item "$env:USERPROFILE\.cargo\bin\flux.exe"
```

### Remove config and data files

**Linux:**

```bash
rm -rf ~/.config/flux
rm -rf ~/.local/share/flux
```

**macOS:**

```bash
rm -rf ~/Library/Application\ Support/flux
rm -rf ~/.config/flux
```

**Windows (PowerShell):**

```powershell
Remove-Item -Recurse -Force "$env:APPDATA\flux"
```

### Remove shell completions

```bash
# Bash
rm ~/.local/share/bash-completion/completions/flux

# Zsh
rm ~/.zfunc/_flux

# Fish
rm ~/.config/fish/completions/flux.fish
```

### Remove source code

```bash
rm -rf ~/Documents/flux
```

---

## Troubleshooting

### `cargo build` fails with OpenSSL errors

**Linux:** Install OpenSSL development headers:

```bash
# Ubuntu/Debian
sudo apt install libssl-dev

# Fedora/RHEL
sudo dnf install openssl-devel

# Arch
sudo pacman -S openssl
```

**macOS:**

```bash
brew install openssl
export OPENSSL_DIR=$(brew --prefix openssl)
cargo build --release
```

**Windows:** Install Strawberry Perl (OpenSSL is built from source using Perl):

```powershell
winget install StrawberryPerl.StrawberryPerl
```

Close and reopen PowerShell, then rebuild.

### `cargo: command not found`

Rust isn't in your PATH. Run:

```bash
source "$HOME/.cargo/env"
```

To make it permanent, add this line to your `~/.bashrc`, `~/.zshrc`, or equivalent:

```bash
export PATH="$HOME/.cargo/bin:$PATH"
```

### `flux: command not found` after building

The binary isn't in your PATH. Either:

1. Copy it: `sudo cp target/release/flux /usr/local/bin/`
2. Or add the build directory to PATH: `export PATH="$PATH:$(pwd)/target/release"`

### TUI shows garbled characters

Your terminal may not support Unicode. Try a modern terminal:

- **Windows:** Windows Terminal (built-in on Windows 11)
- **macOS:** iTerm2 or the built-in Terminal
- **Linux:** Alacritty, Kitty, or GNOME Terminal

### SFTP connection fails

1. Check that SSH works to the target: `ssh user@host`
2. Check that your SSH key is loaded: `ssh-add -l`
3. If using password auth, Flux will prompt you automatically
4. Ensure port 22 is open on the target machine

### SMB paths don't work on Linux/macOS

SMB support currently requires Windows. On Linux/macOS, mount the share first:

```bash
# Linux
sudo mount -t cifs //server/share /mnt/share -o username=user
flux cp file.txt /mnt/share/

# macOS
open smb://server/share  # Mount via Finder
flux cp file.txt /Volumes/share/
```

### mDNS discovery finds no devices

1. Both devices must be on the same network
2. mDNS uses UDP port 5353 — check your firewall
3. Some corporate networks block multicast traffic
4. Try specifying the host directly: `flux send file.txt 192.168.1.100:9741`

### Transfer is slow

1. Try parallel chunks: `flux cp --chunks 8 large-file.bin dest/`
2. Check bandwidth limit isn't set: remove `--limit` if present
3. For network transfers, the bottleneck is usually the network — not Flux
4. Compression helps for text files: `flux cp --compress data.csv dest/`

---

<p align="center">
  <a href="../README.md">Back to README</a>
</p>
