# Changelog

## Unreleased

- Add the local terminal agent to `hudson`, with workspace tools, approvals,
  durable conversations and explicit recovery through `hudson local`.
- Preserve saved version 1 tool definitions and completed operation receipts
  while hardening local file access, session state and process cleanup.
- Add `hudson agent schedule` for scheduler publication inspection.
- Validate all agent base URLs and keep loopback HTTP requests direct with or
  without bearer authentication.

## 0.1.1

- Prepare Homebrew source formulae and native bottles for the Hudson tap.
- Include the merged TLS verification and trust-store updates, protected JSON
  input file handling, authenticated loopback transport fixes and terminal output
  fixes since 0.1.0.
- Preserve the existing native archives and Cargo source-install workflow.

Publication is a separate maintainer action after the complete release checks.

## 0.1.0

Initial standalone Hudson CLI release.

- `hudson agent` connects to agent services for tasks, interactive sessions,
  definitions, replies, approvals, durable events and run controls.
- `hudson sandbox` uses the project HTTPS API for sandbox operations, command
  observation, retained output, streaming and verified file transfers.
- Existing flat agent commands and optional `hudson-cli` / `hudsonbox`
  compatibility executables remain available.
- Offline completion supports Bash, Zsh, Fish, PowerShell and Elvish.
- Explicit JSON file and stdin input preserve saved request keys and do not
  implicitly expand request values into local file reads.
- Agent failures use safe stderr diagnostics, optional JSON execution errors,
  bounded response decoding and handled output-pipe errors.
- Native archives include all three executables, completion, task guides,
  source manifests, project licenses, dependency notices and SHA-256 checksums.

Linux packages require glibc 2.35 or newer. Mac packages target macOS 12 or newer
and are ad-hoc signed, without Developer ID notarization. Windows x86_64 supports
agent operations and offline sandbox commands; sandbox API profiles require
Unix file protections and must be used through a Linux environment such as WSL.
Windows packages are not Authenticode signed.
