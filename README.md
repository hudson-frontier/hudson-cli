<h1 align="center">Hudson CLI</h1>

<p align="center">
  Work with a local agent, Hudson services and Hudsonbox sandboxes from your terminal.
</p>

<p align="center">
  <a href="https://github.com/hudson-frontier/hudson-cli/actions/workflows/ci.yml"><img alt="CLI checks" src="https://github.com/hudson-frontier/hudson-cli/actions/workflows/ci.yml/badge.svg?branch=main"></a>
  <a href="https://github.com/hudson-frontier/hudson-cli/releases"><img alt="Latest release" src="https://img.shields.io/github/v/release/hudson-frontier/hudson-cli?style=flat-square"></a>
  <a href="LICENSE"><img alt="Apache-2.0 license" src="https://img.shields.io/badge/license-Apache%202.0-blue?style=flat-square"></a>
</p>

<p align="center">
  <a href="#installation">Installation</a> ·
  <a href="#local-terminal-agent">Local agent</a> ·
  <a href="#work-with-agents">Agents</a> ·
  <a href="#manage-sandboxes">Sandboxes</a> ·
  <a href="#reference">Documentation</a>
</p>

---

Hudson CLI is the public operator interface for Hudson products. Work in a local
project, submit tasks to configured agents, follow runs, manage sandboxes and
transfer files through one executable. Service operators provide API endpoints,
credentials and workload configuration.

The published v0.1.1 release provides the API and sandbox commands. Local terminal
execution and scheduler inspection are unreleased source features. The guides
describe current source behavior; build `main` to try these additions.

## Installation

### Homebrew

Install Hudson CLI on macOS Apple silicon or Linux x86_64:

```sh
brew install hudson-frontier/tap/hudson
```

The formula installs `hudson` and Bash, Zsh and Fish completion. Matching bottles
provide a prebuilt installation. See [Homebrew distribution](docs/homebrew.md)
for availability, upgrades and removal.

### Native archives

Download an archive and its matching `.sha256` file from
[Releases](https://github.com/hudson-frontier/hudson-cli/releases). Verify the
checksum before extracting, then place `hudson` on your PATH.

| Platform | Architectures | Requirements |
| --- | --- | --- |
| Linux | x86_64, aarch64 | glibc 2.35 or newer |
| macOS | Apple silicon, Intel | macOS 12 or newer |
| Windows | x86_64 | Agent API and offline sandbox commands; WSL/Linux for sandbox API commands |

Archives also include the optional `hudson-cli` and `hudsonbox` compatibility
executables, completions, guides and licenses. Mac archives use ad-hoc signatures;
Windows archives are unsigned. Follow the [installation guide](docs/install.md)
for verification, platform limits, PATH setup, upgrades and removal.

### Cargo

Build and install the published release with Rust 1.92.0 or newer:

```sh
cargo install --git https://github.com/hudson-frontier/hudson-cli --tag v0.1.1 --locked --bin hudson
```

To select another release tag or install compatibility executables, see
[source installation](docs/install.md#source-installation).

Check your installation:

```sh
hudson --version
hudson --help
```

## Local terminal agent

Development builds open a local agent when you run `hudson` without a command:

```sh
hudson --workspace /path/to/project
hudson local --permission read-only
```

The local host uses Anthropic Messages and reads `ANTHROPIC_API_KEY` from the
environment. The default policy asks before edits or shell commands. It records
session state for explicit recovery and keeps the workspace's files in place.
Linux and macOS are supported; use WSL on Windows. Workspace search requires
ripgrep (`rg`) installed separately, for example with `brew install ripgrep`.

Build current source without replacing a stable installation:

```sh
git clone --branch main https://github.com/hudson-frontier/hudson-cli.git
cd hudson-cli
cargo build --locked --bin hudson
./target/debug/hudson local --workspace /path/to/project
```

See the [local agent guide](docs/local-agent.md) for
permissions, model selection, conversation recovery and execution limits.

## Work with agents

Connect to an existing service. The default endpoint is
`http://127.0.0.1:4318`:

```sh
hudson agent health
hudson agent start --task "Summarize the incident" --request-key incident-2026-001
```

Copy the returned `run_id` to follow the task:

```sh
hudson agent follow RUN_UUID
```

For an interactive terminal session:

```sh
hudson agent session
```

The host controls models, tools and permissions. Use `--url` to select another
service and `--api-token-env NAME` to read its token from an environment variable.
Authenticated connections require HTTPS or loopback HTTP. See the
[agent guide](docs/agent.md) for configuration, published agents, replies,
approvals and cancellation.

## Manage sandboxes

Obtain a private client profile from your operator and prepare a `create.json`
request using the [sandbox guide](docs/sandbox.md). Pass the profile with
`--config` or select it through `HUDSONBOX_CONFIG`:

```sh
hudson sandbox --config /private/client.json --json list
hudson sandbox key --to create.key
hudson sandbox --config /private/client.json --json create --request create.json --key-file create.key
hudson sandbox --config /private/client.json --json wait OPERATION_ID --seconds 60
```

Mutations return operation handles before completion. Keep the saved request and
key so retries of the same logical request use the same bytes and key. Stopping
observation leaves the operation running; cancellation is an explicit command.

The sandbox guide also covers execution, output streaming, generation
preconditions and verified file transfers.

## Automate workflows

Agent API results are JSON; agent `--json` selects structured execution errors.
Sandbox commands select JSON with `--json`, and event streams emit NDJSON.
Failures use stderr and a nonzero exit status.

Read a sandbox create or execute request from stdin with `--request -`:

```sh
cat create.json | hudson sandbox --config /private/client.json --json create --request - --key-file create.key
```

See [automation](docs/automation.md) for input limits, event cursors, exit codes
and retry behavior.

## Shell completion

Generate completion offline for Bash, Zsh, Fish, PowerShell or Elvish:

```sh
hudson completion bash > hudson.bash
hudson completion zsh > _hudson
hudson completion fish > hudson.fish
hudson completion powershell > hudson.ps1
```

Follow the [completion guide](docs/automation.md#completion-setup) to load the
generated scripts.

## Reference

Use `--help` on any command without credentials or a network connection:

```sh
hudson agent --help
hudson sandbox create --help
```

| Guide | Covers |
| --- | --- |
| [Command reference](docs/reference.md) | Commands, flags and defaults |
| [Agent workflows](docs/agent.md) | Tasks, sessions, events and approvals |
| [Local terminal agent](docs/local-agent.md) | Workspace tools, approvals and durable conversations |
| [Sandbox workflows](docs/sandbox.md) | Profiles, operations, output and file transfers |
| [Automation](docs/automation.md) | JSON, completion and exit codes |
| [Troubleshooting](docs/troubleshooting.md) | Setup, authentication, transport and recovery |

Legacy flat agent commands, including `hudson health`, remain available.
The optional `hudson-cli` and `hudsonbox` binaries preserve their original command
structures. See [architecture and compatibility](docs/architecture.md).

## Developing

Clone the repository and use its pinned Rust 1.92.0 toolchain:

```sh
git clone --branch main https://github.com/hudson-frontier/hudson-cli.git
cd hudson-cli

cargo fmt --all -- --check
cargo clippy --all-targets --locked -- -D warnings
cargo build --bins --locked
cargo test --all-targets --locked
```

| Path | Purpose |
| --- | --- |
| [src/lib.rs](src/lib.rs) | Command parsing and dispatch |
| [src/agent](src/agent) | Agent transport and terminal workflows |
| [src/local](src/local) | Local terminal host and workspace tools |
| [src/sandbox.rs](src/sandbox.rs) | Sandbox SDK adapter |
| [scripts](scripts) | Native packaging and release validation |
| [docs](docs) | Task guides and generated command reference |

The CLI team maintains this executable. Product teams own their service APIs and
SDKs. See [release maintenance](docs/releasing.md) for delivery checks.

## Contributing

Use focused pull requests with a clear problem and relevant validation.
Read [CONTRIBUTING.md](CONTRIBUTING.md) for the branch, CI and merge process.
Report security issues through [SECURITY.md](SECURITY.md).

## License

Released under [Apache-2.0](LICENSE). Attribution is retained in [NOTICE](NOTICE).
