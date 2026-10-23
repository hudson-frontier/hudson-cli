# Automation and completion

Select output explicitly and check the exit status before accepting a result.
Agent API results are JSON; sandbox commands use `--json`. Streams and agent
following emit NDJSON records. Progress and execution errors use stderr.
The CLI does not change format merely because stdout is redirected.

## Capture results safely

For a finite result, write to a temporary file and publish it only after success:

```sh
temporary="$(mktemp)"
if hudson sandbox --json list > "$temporary"; then
  mv "$temporary" sandboxes.json
else
  rm -f "$temporary"
  exit 1
fi
```

This example replaces `sandboxes.json` only after the client succeeds. Choose a
new destination if preserving the existing file matters. Shell redirection opens
or truncates its destination before the CLI starts; the CLI cannot undo it.
For streams, process records incrementally and retain a durable cursor. A later
failure can follow valid earlier records.

Agent `--json` emits execution errors such as this on stderr:

```json
{"error":"http","http_status":401,"message":"Hudson returned 401 Unauthorized"}
```

Other agent execution categories include `configuration`, `transport`, `timeout`,
`protocol` and `client`. The HTTP status is null for failures without an HTTP
error response. Argument errors retain normal usage output and exit 2. Sandbox
JSON errors also expose safe problem and operation fields when available.
Do not parse human diagnostic wording as a stable protocol.

## Input and retry identity

Agent `--input-file -` and sandbox `--request -` explicitly select stdin. They
read one JSON document, bounded by the API request limit. Request fields never
implicitly read local files or become command-line flags. File-upload paths and
key-file paths retain their normal file semantics.

Keep each mutation key together with the exact saved request. Reusing a key
with changed input, a new deadline or a different project is not a retry.
After lost output or a transport failure, inspect the existing run or operation
before deciding what to repeat. Only observation reads reconnect automatically.

## Exit status

Agent commands return 0 for a successful request or completed follow, 1 for an
execution failure and 2 for invalid arguments. `get` is an inspection request;
a 0 exit does not translate a failed run into success. `follow` returns nonzero
for a failed or cancelled run. Interactive sessions require a terminal.

| Sandbox code | Meaning |
| --- | --- |
| 0 | Successful request; or a successful completed operation when using `wait` |
| 1 | Configuration, request, file or protocol failure |
| 2 | Invalid command arguments |
| 3 | Failed operation without a nonzero command exit |
| 4 | Cancelled operation |
| 5 | Unknown operation outcome |
| 6 | Observation timeout or incomplete operation |
| 7 | Transport failure |
| 8 | HTTP failure |
| 9 | Output stream gap or stream-error event |
| 10 | Completed command returned a nonzero exit code |

`operation` successfully retrieving a failed operation still returns 0. Use
`wait` when the script needs completion-sensitive status. The command's original
exit code is available in the operation result; the CLI uses 10 to distinguish
that case from its own failures. Stream completion is not command completion.

## Completion setup

Completion is generated from the installed executable and works without network
access or credentials. Loading a script is a shell configuration choice; the
CLI does not edit profiles. Regenerate saved scripts after upgrades.

Bash, in the current session:

```bash
source <(hudson completion bash)
```

Zsh, after its completion system has been initialized:

```zsh
autoload -Uz compinit
compinit
source <(hudson completion zsh)
```

Fish:

```fish
hudson completion fish | source
```

PowerShell:

```powershell
hudson completion powershell | Out-String | Invoke-Expression
```

For persistent setup, save the generated script to your shell's completion
directory and load it from the profile you maintain. Elvish generation is
available with `hudson completion elvish`. The archives include the same five
completion scripts so installation does not need service access.
