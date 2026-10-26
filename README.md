# Hudson CLI

Use one executable to operate Hudson agents and Hudsonbox sandboxes. The CLI
connects to an existing service; it does not start a server or provision a VM.

```console
hudson agent --url http://127.0.0.1:4318 health
hudson agent start --task "Summarize the incident" --request-key incident-2026-001
hudson sandbox --config /private/client.json --json list
```

## Install

Homebrew packaging is prepared for macOS and Linux x86_64. Once the corresponding
release is published to the Hudson tap:

```console
brew install hudson-frontier/tap/hudson
```

Matching bottles provide a prebuilt installation with shell completion.
See [Homebrew distribution](docs/homebrew.md) for availability, upgrades and source
fallbacks. Cargo remains the source-install path below.

Choose an archive from [Releases](https://github.com/hudson-frontier/hudson-cli/releases).
Each contains `hudson`, optional `hudson-cli` and `hudsonbox` compatibility
executables, completions, guides, licenses and a source manifest. Download the
matching `.sha256` file and verify it before extracting.

| Platform | Architecture | Archive suffix | Support |
| --- | --- | --- | --- |
| Linux | x86_64 | `linux_x86_64.tar.gz` | Agent and sandbox APIs; glibc 2.35 or newer |
| Linux | aarch64 | `linux_aarch64.tar.gz` | Agent and sandbox APIs; glibc 2.35 or newer |
| macOS | Intel | `macos_x86_64.tar.gz` | Agent and sandbox APIs; macOS 12 or newer |
| macOS | Apple silicon | `macos_aarch64.tar.gz` | Agent and sandbox APIs; macOS 12 or newer |
| Windows | x86_64 | `windows_x86_64.zip` | Agent API and offline sandbox commands |

Windows sandbox API commands require Unix credential ownership and permission
checks in the current SDK. Use WSL/Linux for those commands. The native client
rejects profile loading rather than weakening those checks. The Mac archives
have ad-hoc signatures, not Developer ID notarization. See
[installation](docs/install.md) for verification, PATH setup, upgrades and removal.

Source builds require Rust 1.92.0 or newer. Install only the main executable
unless you also need the compatibility names:

```console
cargo install --git https://github.com/hudson-frontier/hudson-cli --tag v0.1.1 --locked --bin hudson
hudson --version
hudson --help
```

## Agent workflow

Connect to an agent service, submit a task, then follow its returned `run_id`:

```console
hudson agent health
hudson agent start --task "Summarize the incident" --request-key incident-2026-001
hudson agent follow RUN_UUID
```

An interactive session is available with `hudson agent session`. The configured
host owns models, tools and permissions. The CLI's current directory grants no
filesystem or tool authority.

Use `--api-token-env NAME` before the operation to read a bearer token from an
environment variable. Authenticated URLs require HTTPS or loopback HTTP. See
the [agent guide](docs/agent.md) for published definitions, replies, approvals,
event cursors, steering and cancellation.

## Sandbox workflow

Obtain a private client profile and project credential from your service operator.
Supply the profile through `--config` or `HUDSONBOX_CONFIG`:

```console
hudson sandbox --config /private/client.json --json list
hudson sandbox key --to create.key
hudson sandbox --config /private/client.json --json create --request create.json --key-file create.key
hudson sandbox --config /private/client.json --json wait OPERATION_ID --seconds 60
```

Create, execute, destroy and cancel return admission handles before completion.
Keep the request file and mutation key: retry the same logical request with the
same key and bytes when reconciling an ambiguous result. Mutations are not
automatically retried. Stopping observation does not cancel an operation.

The [sandbox guide](docs/sandbox.md) covers profile setup, request examples,
generation preconditions, outputs, streaming and verified file transfers.

## Scripts and shells

Agent API results are JSON. Sandbox commands select JSON explicitly with
`--json`; streams always emit NDJSON. Failures use stderr and a nonzero exit
status. Agent `--json` selects a structured execution-error envelope without
changing successful API output. Argument errors retain usage diagnostics.

Create and execute accept one JSON document from stdin with `--request -`:

```sh
cat create.json | hudson sandbox --config /private/client.json --json create --request - --key-file create.key
```

JSON request input is limited to 64 KiB. Ordinary agent responses are limited to
8 MiB before decoding or output. Invalid or oversized responses do not produce
successful output. Retain event cursors if observation is interrupted.

Generate completion offline for Bash, Zsh, Fish, PowerShell or Elvish:

```console
hudson completion bash > hudson.bash
hudson completion zsh > _hudson
hudson completion fish > hudson.fish
hudson completion powershell > hudson.ps1
```

Generation does not edit shell startup files or load service credentials. See
[automation and completion](docs/automation.md) for setup and exit-code contracts.

## Compatibility and help

Flat agent commands such as `hudson health` and the historical
`hudson agent NAME VERSION` lookup remain supported. Optional `hudson-cli` and
`hudsonbox` executables retain their existing command structures. Choose which
installation owns each executable on PATH.

`--help` works on every command without credentials or a network connection.
The generated [command reference](docs/reference.md) includes current options and
defaults. Start with [troubleshooting](docs/troubleshooting.md) for setup,
authentication, transport, operation and output problems.

The CLI team maintains this executable; product teams own their APIs and SDKs.
See [architecture](docs/architecture.md), [contributing](CONTRIBUTING.md),
[release maintenance](docs/releasing.md) and [security reporting](SECURITY.md).
Licensed under [Apache-2.0](LICENSE); attribution is retained in [NOTICE](NOTICE).
