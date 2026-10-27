# Command reference

Generated from the built CLI with `--help`. Help is available offline and
does not load service credentials. See the task guides for complete workflows.

## hudson

```text
Command-line interface for Hudson products

Usage: hudson <COMMAND>

Commands:
  agent       Connect to the Hudson agent API
  sandbox     Manage Hudsonbox sandboxes through the project HTTPS API
  completion  Print a shell completion script without connecting to a service
  help        Print this message or the help of the given subcommand(s)

Options:
  -h, --help     Print help
  -V, --version  Print version

Legacy agent commands remain available at the root. Use hudson-cli --help for their full list.
```

## hudson agent

```text
Connect to the Hudson agent API

Usage: hudson agent [OPTIONS] [COMMAND]

Commands:
  session        Interactive terminal session using an already running configured host
  health
  capabilities
  publish-tool   Publish a JSON request containing request_key and tool
  publish-agent  Publish a JSON request containing request_key and agent
  tool
  agent
  start
  steer          Queue new direction at the next complete model/tool boundary
  follow-up      Queue work after the current answer passes verification
  inbox          Inspect pending messages without consuming them
  clear-inbox    Withdraw pending messages; their idempotency receipts remain valid
  reply          Answer a waiting agent; reuse the key when retrying the same reply
  resume
  get
  schedule       Inspect scheduler publication; published does not mean the run completed
  children
  operation
  events         Read one event page (default 100); continue with --after the last sequence
  follow         Follow durable events until terminal; Ctrl-C stops observation only
  approve
  cancel
  help           Print this message or the help of the given subcommand(s)

Options:
      --url <URL>                      [default: http://127.0.0.1:4318]
      --api-token-env <API_TOKEN_ENV>  Read the bearer token from this environment variable, never from an argument
      --json                           Report execution errors as JSON on stderr. API success output is already JSON
  -h, --help                           Print help
  -V, --version                        Print version
```

## hudson sandbox

```text
Manage Hudsonbox sandboxes through the project HTTPS API

Usage: hudson sandbox [OPTIONS] <COMMAND>

Commands:
  key         Save a new mutation key before making a request. Refuses to overwrite
  create      Admit creation from a saved JSON request. Returns before completion
  list        List one page. Pass the returned cursor explicitly for the next page
  get
  execute     Admit a command from saved JSON including an absolute deadline_unix_ms
  destroy
  operations
  operation
  wait        Poll only this operation. Timeout does not cancel it
  cancel      Admit a separate cancellation operation; inspect both handles afterward
  output      Save one retained output range to a new local file, with framing metadata
  stream      Stream NDJSON with base64 bytes and resumable cursors. End is output completion
  upload      Admit a bounded file upload; returns before guest publication
  download    Capture once and verify size/SHA-256 before publishing a new private file
  help        Print this message or the help of the given subcommand(s)

Options:
      --config <CONFIG>  Private client configuration; contains the origin and credential file path [env: HUDSONBOX_CONFIG=]
      --json             Machine-readable JSON. Stream always emits newline-delimited JSON events
  -h, --help             Print help
  -V, --version          Print version
```

## hudson completion

```text
Print a shell completion script without connecting to a service

Usage: hudson completion <SHELL>

Arguments:
  <SHELL>  [possible values: bash, elvish, fish, powershell, zsh]

Options:
  -h, --help  Print help
```

## hudson agent session

```text
Interactive terminal session using an already running configured host

Usage: hudson agent session [OPTIONS]

Options:
      --agent <AGENT>                  Select an immutable published agent instead of the startup-configured agent
      --agent-version <AGENT_VERSION>
      --json                           Report execution errors as JSON on stderr. API success output is already JSON
  -h, --help                           Print help
```

## hudson agent health

```text
Usage: hudson agent health [OPTIONS]

Options:
      --json  Report execution errors as JSON on stderr. API success output is already JSON
  -h, --help  Print help
```

## hudson agent capabilities

```text
Usage: hudson agent capabilities [OPTIONS]

Options:
      --json  Report execution errors as JSON on stderr. API success output is already JSON
  -h, --help  Print help
```

## hudson agent publish-tool

```text
Publish a JSON request containing request_key and tool

Usage: hudson agent publish-tool [OPTIONS] <FILE>

Arguments:
  <FILE>

Options:
      --json  Report execution errors as JSON on stderr. API success output is already JSON
  -h, --help  Print help
```

## hudson agent publish-agent

```text
Publish a JSON request containing request_key and agent

Usage: hudson agent publish-agent [OPTIONS] <FILE>

Arguments:
  <FILE>

Options:
      --json  Report execution errors as JSON on stderr. API success output is already JSON
  -h, --help  Print help
```

## hudson agent tool

```text
Usage: hudson agent tool [OPTIONS] <NAME> <VERSION>

Arguments:
  <NAME>
  <VERSION>

Options:
      --json  Report execution errors as JSON on stderr. API success output is already JSON
  -h, --help  Print help
```

## hudson agent agent

```text
Usage: hudson agent agent [OPTIONS] <NAME> <VERSION>

Arguments:
  <NAME>
  <VERSION>

Options:
      --json  Report execution errors as JSON on stderr. API success output is already JSON
  -h, --help  Print help
```

## hudson agent start

```text
Usage: hudson agent start [OPTIONS] <--task <TASK>|--input-file <INPUT_FILE>|--order <ORDER>>

Options:
      --agent <AGENT>                  Select an immutable published agent; omit for startup-configured hosts
      --agent-version <AGENT_VERSION>
      --json                           Report execution errors as JSON on stderr. API success output is already JSON
      --task <TASK>                    Submit a plain-text task to the configured agent
      --input-file <INPUT_FILE>        Submit arbitrary JSON from a file; use - to read standard input
      --order <ORDER>                  Explicit input for the offline order fixture
      --refund
      --request-key <REQUEST_KEY>
  -h, --help                           Print help
```

## hudson agent steer

```text
Queue new direction at the next complete model/tool boundary

Usage: hudson agent steer [OPTIONS] --text <TEXT> --request-key <REQUEST_KEY> <RUN_ID>

Arguments:
  <RUN_ID>

Options:
      --text <TEXT>
      --request-key <REQUEST_KEY>
      --json                       Report execution errors as JSON on stderr. API success output is already JSON
  -h, --help                       Print help
```

## hudson agent follow-up

```text
Queue work after the current answer passes verification

Usage: hudson agent follow-up [OPTIONS] --text <TEXT> --request-key <REQUEST_KEY> <RUN_ID>

Arguments:
  <RUN_ID>

Options:
      --text <TEXT>
      --request-key <REQUEST_KEY>
      --json                       Report execution errors as JSON on stderr. API success output is already JSON
  -h, --help                       Print help
```

## hudson agent inbox

```text
Inspect pending messages without consuming them

Usage: hudson agent inbox [OPTIONS] <RUN_ID>

Arguments:
  <RUN_ID>

Options:
      --json  Report execution errors as JSON on stderr. API success output is already JSON
  -h, --help  Print help
```

## hudson agent clear-inbox

```text
Withdraw pending messages; their idempotency receipts remain valid

Usage: hudson agent clear-inbox [OPTIONS] <RUN_ID>

Arguments:
  <RUN_ID>

Options:
      --json  Report execution errors as JSON on stderr. API success output is already JSON
  -h, --help  Print help
```

## hudson agent reply

```text
Answer a waiting agent; reuse the key when retrying the same reply

Usage: hudson agent reply [OPTIONS] --question-id <QUESTION_ID> --text <TEXT> --request-key <REQUEST_KEY> <RUN_ID>

Arguments:
  <RUN_ID>

Options:
      --question-id <QUESTION_ID>
      --text <TEXT>
      --json                       Report execution errors as JSON on stderr. API success output is already JSON
      --request-key <REQUEST_KEY>
  -h, --help                       Print help
```

## hudson agent resume

```text
Usage: hudson agent resume [OPTIONS] <RUN_ID>

Arguments:
  <RUN_ID>

Options:
      --json  Report execution errors as JSON on stderr. API success output is already JSON
  -h, --help  Print help
```

## hudson agent get

```text
Usage: hudson agent get [OPTIONS] <RUN_ID>

Arguments:
  <RUN_ID>

Options:
      --json  Report execution errors as JSON on stderr. API success output is already JSON
  -h, --help  Print help
```

## hudson agent schedule

```text
Inspect scheduler publication; published does not mean the run completed

Usage: hudson agent schedule [OPTIONS] <RUN_ID>

Arguments:
  <RUN_ID>

Options:
      --json  Report execution errors as JSON on stderr. API success output is already JSON
  -h, --help  Print help
```

## hudson agent children

```text
Usage: hudson agent children [OPTIONS] <RUN_ID>

Arguments:
  <RUN_ID>

Options:
      --json  Report execution errors as JSON on stderr. API success output is already JSON
  -h, --help  Print help
```

## hudson agent operation

```text
Usage: hudson agent operation [OPTIONS] <OPERATION_ID>

Arguments:
  <OPERATION_ID>

Options:
      --json  Report execution errors as JSON on stderr. API success output is already JSON
  -h, --help  Print help
```

## hudson agent events

```text
Read one event page (default 100); continue with --after the last sequence

Usage: hudson agent events [OPTIONS] <RUN_ID>

Arguments:
  <RUN_ID>

Options:
      --after <AFTER>  [default: 0]
      --json           Report execution errors as JSON on stderr. API success output is already JSON
  -h, --help           Print help
```

## hudson agent follow

```text
Follow durable events until terminal; Ctrl-C stops observation only

Usage: hudson agent follow [OPTIONS] <RUN_ID>

Arguments:
  <RUN_ID>

Options:
      --after <AFTER>              Resume after the last fully processed event sequence [default: 0]
      --page-size <PAGE_SIZE>      [default: 100]
      --json                       Report execution errors as JSON on stderr. API success output is already JSON
      --poll-ms <POLL_MS>          [default: 1000]
      --max-retries <MAX_RETRIES>  Consecutive retries for each unavailable request; backoff caps at 30 seconds [default: 5]
  -h, --help                       Print help
```

## hudson agent approve

```text
Usage: hudson agent approve [OPTIONS] <OPERATION_ID>

Arguments:
  <OPERATION_ID>

Options:
      --deny
      --json  Report execution errors as JSON on stderr. API success output is already JSON
  -h, --help  Print help
```

## hudson agent cancel

```text
Usage: hudson agent cancel [OPTIONS] <RUN_ID>

Arguments:
  <RUN_ID>

Options:
      --json  Report execution errors as JSON on stderr. API success output is already JSON
  -h, --help  Print help
```

## hudson sandbox key

```text
Save a new mutation key before making a request. Refuses to overwrite

Usage: hudson sandbox key [OPTIONS] --to <TO>

Options:
      --config <CONFIG>  Private client configuration; contains the origin and credential file path [env: HUDSONBOX_CONFIG=]
      --to <TO>
      --json             Machine-readable JSON. Stream always emits newline-delimited JSON events
  -h, --help             Print help
```

## hudson sandbox create

```text
Admit creation from a saved JSON request. Returns before completion

Usage: hudson sandbox create [OPTIONS] --request <REQUEST> <--key <KEY>|--key-file <KEY_FILE>>

Options:
      --config <CONFIG>      Private client configuration; contains the origin and credential file path [env: HUDSONBOX_CONFIG=]
      --request <REQUEST>    JSON request file, or - to read one bounded document from stdin
      --json                 Machine-readable JSON. Stream always emits newline-delimited JSON events
      --key <KEY>            Stable key for one logical mutation. Reuse with exactly the same payload
      --key-file <KEY_FILE>  Existing file containing the stable key. Generate once with `key --to`
  -h, --help                 Print help
```

## hudson sandbox list

```text
List one page. Pass the returned cursor explicitly for the next page

Usage: hudson sandbox list [OPTIONS]

Options:
      --config <CONFIG>  Private client configuration; contains the origin and credential file path [env: HUDSONBOX_CONFIG=]
      --limit <LIMIT>
      --cursor <CURSOR>
      --json             Machine-readable JSON. Stream always emits newline-delimited JSON events
  -h, --help             Print help
```

## hudson sandbox get

```text
Usage: hudson sandbox get [OPTIONS] <SANDBOX_ID>

Arguments:
  <SANDBOX_ID>

Options:
      --config <CONFIG>  Private client configuration; contains the origin and credential file path [env: HUDSONBOX_CONFIG=]
      --json             Machine-readable JSON. Stream always emits newline-delimited JSON events
  -h, --help             Print help
```

## hudson sandbox execute

```text
Admit a command from saved JSON including an absolute deadline_unix_ms

Usage: hudson sandbox execute [OPTIONS] --request <REQUEST> <--key <KEY>|--key-file <KEY_FILE>> <SANDBOX_ID>

Arguments:
  <SANDBOX_ID>

Options:
      --config <CONFIG>      Private client configuration; contains the origin and credential file path [env: HUDSONBOX_CONFIG=]
      --request <REQUEST>    JSON request file, or - to read one bounded document from stdin
      --json                 Machine-readable JSON. Stream always emits newline-delimited JSON events
      --key <KEY>            Stable key for one logical mutation. Reuse with exactly the same payload
      --key-file <KEY_FILE>  Existing file containing the stable key. Generate once with `key --to`
  -h, --help                 Print help
```

## hudson sandbox destroy

```text
Usage: hudson sandbox destroy [OPTIONS] <--key <KEY>|--key-file <KEY_FILE>> <SANDBOX_ID>

Arguments:
  <SANDBOX_ID>

Options:
      --config <CONFIG>      Private client configuration; contains the origin and credential file path [env: HUDSONBOX_CONFIG=]
      --key <KEY>            Stable key for one logical mutation. Reuse with exactly the same payload
      --json                 Machine-readable JSON. Stream always emits newline-delimited JSON events
      --key-file <KEY_FILE>  Existing file containing the stable key. Generate once with `key --to`
  -h, --help                 Print help
```

## hudson sandbox operations

```text
Usage: hudson sandbox operations [OPTIONS]

Options:
      --config <CONFIG>          Private client configuration; contains the origin and credential file path [env: HUDSONBOX_CONFIG=]
      --sandbox-id <SANDBOX_ID>
      --json                     Machine-readable JSON. Stream always emits newline-delimited JSON events
      --limit <LIMIT>
      --cursor <CURSOR>
  -h, --help                     Print help
```

## hudson sandbox operation

```text
Usage: hudson sandbox operation [OPTIONS] <OPERATION_ID>

Arguments:
  <OPERATION_ID>

Options:
      --config <CONFIG>  Private client configuration; contains the origin and credential file path [env: HUDSONBOX_CONFIG=]
      --json             Machine-readable JSON. Stream always emits newline-delimited JSON events
  -h, --help             Print help
```

## hudson sandbox wait

```text
Poll only this operation. Timeout does not cancel it

Usage: hudson sandbox wait [OPTIONS] <OPERATION_ID>

Arguments:
  <OPERATION_ID>

Options:
      --config <CONFIG>    Private client configuration; contains the origin and credential file path [env: HUDSONBOX_CONFIG=]
      --seconds <SECONDS>  [default: 60]
      --json               Machine-readable JSON. Stream always emits newline-delimited JSON events
  -h, --help               Print help
```

## hudson sandbox cancel

```text
Admit a separate cancellation operation; inspect both handles afterward

Usage: hudson sandbox cancel [OPTIONS] <--key <KEY>|--key-file <KEY_FILE>> <OPERATION_ID>

Arguments:
  <OPERATION_ID>

Options:
      --config <CONFIG>      Private client configuration; contains the origin and credential file path [env: HUDSONBOX_CONFIG=]
      --key <KEY>            Stable key for one logical mutation. Reuse with exactly the same payload
      --json                 Machine-readable JSON. Stream always emits newline-delimited JSON events
      --key-file <KEY_FILE>  Existing file containing the stable key. Generate once with `key --to`
  -h, --help                 Print help
```

## hudson sandbox output

```text
Save one retained output range to a new local file, with framing metadata

Usage: hudson sandbox output [OPTIONS] --to <TO> <OPERATION_ID> <OUTPUT_NAME>

Arguments:
  <OPERATION_ID>
  <OUTPUT_NAME>   [possible values: stdout, stderr]

Options:
      --config <CONFIG>  Private client configuration; contains the origin and credential file path [env: HUDSONBOX_CONFIG=]
      --offset <OFFSET>  [default: 0]
      --json             Machine-readable JSON. Stream always emits newline-delimited JSON events
      --limit <LIMIT>    [default: 32768]
      --to <TO>
  -h, --help             Print help
```

## hudson sandbox stream

```text
Stream NDJSON with base64 bytes and resumable cursors. End is output completion

Usage: hudson sandbox stream [OPTIONS] <OPERATION_ID>

Arguments:
  <OPERATION_ID>

Options:
      --config <CONFIG>          Private client configuration; contains the origin and credential file path [env: HUDSONBOX_CONFIG=]
      --cursor <CURSOR>
      --json                     Machine-readable JSON. Stream always emits newline-delimited JSON events
      --seconds <SECONDS>        [default: 120]
      --reconnects <RECONNECTS>  [default: 3]
  -h, --help                     Print help
```

## hudson sandbox upload

```text
Admit a bounded file upload; returns before guest publication

Usage: hudson sandbox upload [OPTIONS] --from <FROM> <--key <KEY>|--key-file <KEY_FILE>> <SANDBOX_ID> <PATH>

Arguments:
  <SANDBOX_ID>
  <PATH>

Options:
      --config <CONFIG>      Private client configuration; contains the origin and credential file path [env: HUDSONBOX_CONFIG=]
      --from <FROM>
      --json                 Machine-readable JSON. Stream always emits newline-delimited JSON events
      --mode <MODE>          [default: 0644] [possible values: 0644, 0755]
      --key <KEY>            Stable key for one logical mutation. Reuse with exactly the same payload
      --key-file <KEY_FILE>  Existing file containing the stable key. Generate once with `key --to`
  -h, --help                 Print help
```

## hudson sandbox download

```text
Capture once and verify size/SHA-256 before publishing a new private file

Usage: hudson sandbox download [OPTIONS] --to <TO> <SANDBOX_ID> <PATH>

Arguments:
  <SANDBOX_ID>
  <PATH>

Options:
      --config <CONFIG>  Private client configuration; contains the origin and credential file path [env: HUDSONBOX_CONFIG=]
      --to <TO>
      --json             Machine-readable JSON. Stream always emits newline-delimited JSON events
  -h, --help             Print help
```
