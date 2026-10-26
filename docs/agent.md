# Agent workflows

Connect to an existing Hudson service. The host owns its models, tools and access
policy; submitting a task uses that configuration. The CLI does not start a
backend or grant authority over the current folder.

## Connect

Local development defaults to `http://127.0.0.1:4318`:

```console
hudson agent health
hudson agent capabilities
```

Obtain bearer credentials from the operator and expose them through your shell
or secret manager. The argument names an environment variable, not its value:

```console
hudson agent --url https://agent.example.com --api-token-env HUDSON_API_TOKEN health
```

Use trusted HTTPS or loopback HTTP. Authenticated URLs cannot contain user
information, query strings or fragments. Connection options precede the operation.
Agent and sandbox credentials remain independent.
Authenticated loopback HTTP connects directly and ignores proxy environment variables.

## Submit and follow

Choose and retain one request key for one logical task:

```console
hudson agent start --task "Summarize the incident" --request-key incident-2026-001
```

Copy the returned `run_id` into the following commands:

```console
hudson agent get RUN_UUID
hudson agent follow RUN_UUID
hudson agent events RUN_UUID --after 42
hudson agent follow RUN_UUID --after 42
```

`follow` emits event and run records as NDJSON. Retain the last fully processed
`after` value; failure diagnostics include a reconnect cursor. Observation retries
do not resubmit a task. Ctrl-C stops the client without cancelling the run.
A completed run returns success; failed and cancelled runs return nonzero.
With `--json`, follow failures retain their HTTP, protocol or transport category
and the reconnect cursor. Transient retries omit human progress messages so stderr
contains only the final error document, if observation fails.

Read one JSON value from a file or select stdin explicitly:

```console
hudson agent start --input-file task.json --request-key structured-task-001
```

```sh
cat task.json | hudson agent start --input-file - --request-key structured-task-001
```

Request bodies are limited to 64 KiB. Values are literal JSON: a string beginning
with `@` does not read a local file. Keep the same input and key when reconciling
an ambiguous result instead of submitting a new logical task.
File inputs must be regular files; symbolic links, Windows reparse points and
special files are rejected before sending a request. Use `-` to read stdin explicitly.

## Published definitions

On a host configured for publication, select an immutable agent:

```console
hudson agent start --agent analyst --agent-version 1 --task "Review the incident" --request-key analyst-task-001
hudson agent agent analyst 1
hudson agent tool search 1
hudson agent publish-tool tool-request.json
hudson agent publish-agent agent-request.json
```

Publication files contain the service request, including its request key and
definition. Host capabilities and service validation determine what is supported.
The second `agent` in the read example is the operation inside the product
namespace. The CLI neither invents a catalog nor changes server authorization.

## Replies and controls

Use identifiers returned by the service. Approval and denial are alternatives;
run only the action you intend:

```console
hudson agent reply RUN_UUID --question-id 7 --text "Use the approved dataset" --request-key answer-001
hudson agent steer RUN_UUID --text "Focus on the last hour" --request-key direction-001
hudson agent follow-up RUN_UUID --text "Prepare a handoff" --request-key followup-001
hudson agent inbox RUN_UUID
hudson agent clear-inbox RUN_UUID
hudson agent operation OPERATION_UUID
hudson agent approve OPERATION_UUID
hudson agent approve OPERATION_UUID --deny
hudson agent resume RUN_UUID
hudson agent children RUN_UUID
hudson agent cancel RUN_UUID
```

Clearing the inbox withdraws pending messages without invalidating their retry
receipts. Cancellation is a service request; inspect the run afterwards rather
than treating a disconnected client as cancellation.

## Interactive session

```console
hudson agent session
hudson agent session --agent analyst --agent-version 1
```

Sessions require terminal input and output. Type a task or `/help`. Tasks are
independent; `/open RUN_UUID` resumes observation. Request keys are printed before
submitting tasks and replies so uncertain results can be reconciled. Use `start`
and `follow` for scripts instead of piping input to a terminal session.
Server-supplied display fields escape line breaks, tabs, terminal controls and
Unicode direction controls so they stay within their labeled output line.
If terminal output fails, the session exits with code 1. Tasks and replies are
sent only after their request key has been written and flushed successfully.

## Errors

Execution failures use stderr and exit 1. `--json` selects an error document with
`error`, `http_status` and `message`; successful API output stays unchanged.
Argument errors show usage and exit 2. Ordinary response decoding is limited to
8 MiB. Failures do not echo server error bodies or request URLs. If output cannot
be written after a mutation, inspect its outcome before retrying.

See [automation](automation.md), [troubleshooting](troubleshooting.md) and the
[command reference](reference.md).
