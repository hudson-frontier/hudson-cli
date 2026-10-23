# Hudson CLI

One command-line client for Hudson products, maintained in its own repository by
the CLI team. It connects to existing services; run the Hudson or Hudsonbox server
separately.

```console
hudson agent --url http://127.0.0.1:4318 health
hudson agent --url http://127.0.0.1:4318 start --task "Summarize the incident"
hudson sandbox --config /private/client.json --json list
hudson sandbox --help
```

## Install from source

Rust 1.92.0 is pinned in `rust-toolchain.toml`.

```console
cargo install --git https://github.com/hudson-frontier/hudson-cli --locked --bin hudson
hudson --help
```

There are no published binary releases yet. Linux and macOS support both product
clients. Windows supports the agent client and offline sandbox commands such as
`key`. The pinned Hudsonbox SDK requires Unix credential-file ownership and
permission checks, so sandbox API commands on Windows return a configuration
error. Use a Linux environment such as WSL for those commands.

## Shell completion

Generate completion from the installed command definitions. Supported shells are
`bash`, `zsh`, `fish`, `powershell` and `elvish`. Generation works offline and
does not load credentials or change your shell configuration.

```console
hudson completion bash > hudson.bash
hudson completion zsh > _hudson
hudson completion fish > hudson.fish
hudson completion powershell > hudson.ps1
```

Load the generated script using your shell's completion configuration. Regenerate
it after upgrading to include new commands and flags.

## Agent commands

`hudson agent` provides interactive sessions, health and capabilities, immutable
tool/agent definitions, run submission, replies, steering, durable event following,
approvals and cancellation. An omitted subcommand starts the existing interactive
session, which requires a terminal. Use `start` and `follow` for scripts.

Connection options precede the agent operation:

```console
hudson agent --url https://agent.example.com --api-token-env HUDSON_API_TOKEN health
hudson agent --url http://127.0.0.1:4318 follow RUN_UUID --after 42
hudson agent --help
```

`--api-token-env` names an environment variable containing the token. Authenticated
connections require HTTPS or loopback HTTP. Ordinary API results are JSON;
`follow` emits JSON records, and the interactive session renders terminal text.

Execution failures go to stderr and return exit code `1`. Failed API bodies are
never emitted as successful output. Select `--json` for an execution error
document containing `error`, `http_status` (when available) and `message`:

```console
hudson agent --url http://127.0.0.1:4318 --json health
```

Successful API output remains JSON with or without this flag. Argument errors
retain normal usage diagnostics and exit code `2`. Transport diagnostics omit
the request URL and response body.
Ordinary agent responses are limited to 8 MiB before decoding or output. Larger,
truncated or invalid JSON responses fail with a protocol diagnostic and no
payload on stdout. Event following retains its bounded, record-oriented output.

## Sandbox commands

`hudson sandbox` uses the public Hudsonbox SDK for sandbox creation, execution,
destruction, operation reads and waits, output, streaming and file transfer.
The service provisions the project credential; the CLI reads a private profile.
On Unix, the profile, credential and custom CA files must be owned by the current
user with no group or other permissions (for example, mode `0600`). Relative file
paths are resolved from the profile directory.

```json
{
  "version": 1,
  "endpoint": "https://sandbox.example.com",
  "credential_file": "credential.json",
  "request_timeout_seconds": 30
}
```

The endpoint must be an HTTPS origin. Add `ca_file` for a private CA bundle. Supply
`--config` or set `HUDSONBOX_CONFIG`. Use `--json` for automation; streams always
emit NDJSON. Credentials remain separate from agent credentials.

Save a mutation key before submitting a request. Reuse that key with the same
payload when reconciling an ambiguous result:

```console
hudson sandbox key --to create.key
hudson sandbox --config /private/client.json --json create --request create.json --key-file create.key
hudson sandbox --config /private/client.json --json wait OPERATION_ID --seconds 60
```

Create and execute also accept `--request -` to read one JSON document, up to
64 KiB, from stdin. For example, on a Unix shell:

```sh
cat create.json | hudson sandbox --config /private/client.json --json create --request - --key-file create.key
```

Input is consumed only when `-` is selected. Request values are data and never
implicitly read local files. Save and reuse the mutation key with the same
request when reconciling an uncertain outcome.

Create, execute, destroy and cancel return admission handles before completion.
Mutations are not automatically retried. Stopping observation does not cancel an
operation. Downloads and saved output refuse to overwrite an existing file.

Sandbox exit codes preserve the existing CLI contract:

| Code | Meaning |
| --- | --- |
| 0 | Successful request or completed successful operation |
| 1 | Configuration, request, file or protocol error |
| 2 | Invalid command arguments |
| 3 | Completed failed operation without a nonzero command exit |
| 4 | Cancelled operation |
| 5 | Unknown operation outcome |
| 6 | Wait timed out or operation remains incomplete |
| 7 | Transport failure |
| 8 | HTTP failure |
| 10 | Completed command returned a nonzero exit code |

## Existing commands

Existing `hudson-cli` and `hudsonbox` entrypoints are available as optional
compatibility binaries. They retain their original command structures:

```console
cargo install --git https://github.com/hudson-frontier/hudson-cli --locked --bin hudson-cli --bin hudsonbox
hudson-cli --url http://127.0.0.1:4318 health
hudsonbox --config /private/client.json --json list
```

Flat agent commands such as `hudson health` and the original
`hudson agent NAME VERSION` lookup remain supported. The existing product
repositories' binaries continue to work; no product deployment is required.
Choose which installation owns each executable on your `PATH`.

See [CONTRIBUTING.md](CONTRIBUTING.md) for checks and
[docs/architecture.md](docs/architecture.md) for product boundaries and ownership.
Licensed under [Apache-2.0](LICENSE); source attribution is in [NOTICE](NOTICE).
