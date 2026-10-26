# Installation

Release archives are standalone clients. They contain no backend service,
database or virtual machine. Obtain the service URL and credentials separately.

## Homebrew

After the CLI release and its tap formula are published, install the primary
executable and shell completion with:

```sh
brew install hudson-frontier/tap/hudson
```

Matching native bottles provide macOS Apple silicon and Linux x86_64
installations. Source fallback uses the pinned release and Homebrew's Rust build
dependency. See [Homebrew distribution](homebrew.md) for the publication and
upgrade contract. Use the native archives or Cargo on Intel Macs, Windows and
Linux aarch64. Native archives below remain available independently.

## Choose the archive

Download the archive and matching `.sha256` file from the
[release page](https://github.com/hudson-frontier/hudson-cli/releases):

| System | CPU | v0.1.1 archive |
| --- | --- | --- |
| Linux | x86_64 / amd64 | `hudson-cli_0.1.1_linux_x86_64.tar.gz` |
| Linux | aarch64 / arm64 | `hudson-cli_0.1.1_linux_aarch64.tar.gz` |
| macOS | Intel | `hudson-cli_0.1.1_macos_x86_64.tar.gz` |
| macOS | Apple silicon | `hudson-cli_0.1.1_macos_aarch64.tar.gz` |
| Windows | x86_64 / amd64 | `hudson-cli_0.1.1_windows_x86_64.zip` |

Linux archives are built and tested on Ubuntu 22.04 and require glibc 2.35 or
newer. Alpine/musl is not a binary target for this release. Mac binaries target
macOS 12 or newer and are tested on macOS 15. Windows binaries use the static
Microsoft C runtime and are tested on Windows Server 2025.

Windows supports agent operations, help, completion and offline sandbox key
generation. Sandbox API profiles require Unix file protections; use WSL/Linux
for those operations, including when using the `hudsonbox` compatibility name.

## Linux example

This example selects x86_64. Substitute aarch64 filenames on ARM. Download into
a new directory and check the checksum before extraction:

```sh
set -eu
mkdir hudson-download
cd hudson-download
curl --fail --location --output hudson-cli_0.1.1_linux_x86_64.tar.gz https://github.com/hudson-frontier/hudson-cli/releases/download/v0.1.1/hudson-cli_0.1.1_linux_x86_64.tar.gz
curl --fail --location --output hudson-cli_0.1.1_linux_x86_64.tar.gz.sha256 https://github.com/hudson-frontier/hudson-cli/releases/download/v0.1.1/hudson-cli_0.1.1_linux_x86_64.tar.gz.sha256
sha256sum --check hudson-cli_0.1.1_linux_x86_64.tar.gz.sha256
tar -xzf hudson-cli_0.1.1_linux_x86_64.tar.gz
mkdir -p "$HOME/.local/bin"
install -m 0755 hudson-cli_0.1.1_linux_x86_64/bin/hudson "$HOME/.local/bin/hudson"
"$HOME/.local/bin/hudson" --version
```

Add `$HOME/.local/bin` to your shell's PATH if needed. Install `hudson-cli` or
`hudsonbox` from the same archive only when you need those compatibility names.
Keep the guides and completions wherever you keep application documentation.

## macOS

Download the appropriate Mac archive and checksum into a new directory:

```sh
set -eu
shasum -a 256 --check hudson-cli_0.1.1_macos_aarch64.tar.gz.sha256
tar -xzf hudson-cli_0.1.1_macos_aarch64.tar.gz
mkdir -p "$HOME/.local/bin"
install -m 0755 hudson-cli_0.1.1_macos_aarch64/bin/hudson "$HOME/.local/bin/hudson"
"$HOME/.local/bin/hudson" --version
```

Replace `aarch64` with `x86_64` for Intel. The release has ad-hoc code signatures,
not Developer ID notarization. A browser download can require approval through
macOS Privacy & Security. Verify the checksum and follow the operating system's
approval process only for a binary you intend to trust. Source installation is
an alternative.

## Windows PowerShell

Download the ZIP and checksum from the release page into a new directory:

```powershell
$ErrorActionPreference = 'Stop'
$archive = 'hudson-cli_0.1.1_windows_x86_64.zip'
$expected = ((Get-Content "$archive.sha256" -Raw).Trim() -split '\s+')[0]
$actual = (Get-FileHash -Algorithm SHA256 -LiteralPath $archive).Hash
if ($actual -ine $expected) { throw 'Archive checksum mismatch' }
Expand-Archive -LiteralPath $archive -DestinationPath '.\unpacked'
& '.\unpacked\hudson-cli_0.1.1_windows_x86_64\bin\hudson.exe' --version
```

Copy `hudson.exe` to a directory you own and add it to your user PATH. Install
the aliases only if needed. Native Windows packages are not Authenticode
signed. Use the agent token environment option rather than credentials in
arguments; see the [agent guide](agent.md).

## Source installation

Install Rust 1.92.0 or newer, then select the exact release tag:

```console
cargo install --git https://github.com/hudson-frontier/hudson-cli --tag v0.1.1 --locked --bin hudson
```

For the aliases:

```console
cargo install --git https://github.com/hudson-frontier/hudson-cli --tag v0.1.1 --locked --bin hudson-cli --bin hudsonbox
```

Cargo normally installs to `$HOME/.cargo/bin` on Unix or
`%USERPROFILE%\.cargo\bin` on Windows. `cargo install --list` shows Cargo-managed
installations. Repository development uses the pinned Rust 1.92.0 toolchain.

## Upgrade or remove

The CLI does not update itself or change shell configuration. For a manual
installation, verify the new archive and replace only its executables. Regenerate
completion after an upgrade. For Cargo, install the new tag and use `--force`
when replacing an existing executable.

Use `command -v hudson` on Unix or `Get-Command hudson -All` in PowerShell to
check PATH ownership. Existing product repositories may provide the same names.
Remove only the executables and completion files owned by the installation you
intend to uninstall. For Cargo, use `cargo uninstall hudson-cli`.

Removing the client does not cancel service operations or delete your profiles
and request keys.
