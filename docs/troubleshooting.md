# Troubleshooting

Start with `hudson --version`, `hudson --help`, and the command's own `--help`.
Help and completion work offline. Confirm PATH ownership with `command -v
hudson` on Unix or `Get-Command hudson -All` in PowerShell before changing an
installation.

## Installation does not run

Check the archive's OS and CPU against your machine and verify the matching
checksum. Linux archives require glibc 2.35 or newer; macOS archives require
macOS 12 or newer. Windows uses an `.exe` in the ZIP's `bin` directory.
See [installation](install.md) for manual and source paths, Mac approval and
platform support. A checksum mismatch requires a fresh trusted download, not a
forced extraction.

## Sandbox configuration rejected

Use an HTTPS origin without a path prefix, query, fragment or URL credentials.
The profile and referenced credential and CA files must be private regular files,
owned by the current user, with no group/other permissions. Check that relative
paths are correct from the profile directory. A credential is issued by the
operator; an arbitrary token string or a profile file is not a credential.

On WSL use a Linux filesystem directory such as `$HOME/.config/hudson/sandbox`.
Mounted Windows directories may not provide the required Unix ownership and
permissions. Native Windows profile loading is unsupported. Do not bypass file
protections to make the check pass.

## Authentication or transport failure

For agents, `--api-token-env` names a populated environment variable. It does not
take the token itself. Authenticated URLs require trusted HTTPS or loopback HTTP.
For sandboxes, confirm the selected profile, project scope and credential expiry
with the operator. HTTP 401 and 403 are not reasons to resubmit a mutation with
a new key.

Check the origin and TLS trust separately from operation status. Private sandbox
services can use the operator's CA bundle through `ca_file`. Redirects are not
followed. Error output omits credentials and raw server bodies; use the service's
own logs through an authorized operator when more detail is needed.

## A request timed out or output was lost

The service may have received a mutation even if the client did not receive its
receipt. Preserve the request and key. Inspect the run or operation, then retry
the same logical request with that key and payload when appropriate. Do not
regenerate a deadline or change input while calling it the same retry.

Stopping `follow`, `stream` or `wait` does not cancel work. Cancellation has its
own explicit command. Agent output-pipe failures return a client error instead
of a panic; inspect a mutation's outcome before repeating it.

## Events or output are incomplete

Resume agent following after the last processed `after` sequence. Resume sandbox
streaming with the last fully processed cursor. A gap or stream-error event
returns 9 and requires reconciliation with retained output or operation status.
An `end` event concerns output completion, not successful command execution.

Ordinary agent responses exceeding 8 MiB are rejected before JSON output. For
large event histories, use paged events or following. Sandbox output reads are
bounded ranges; use the returned offsets rather than assuming one range is the
whole output. Respect truncation and retention metadata.

## Uploads or downloads fail

Use a guest path allowed by the image and project policy. Upload admission is
not completed publication: wait on the returned operation. Downloads validate
size and SHA-256 before creating a new private file. Saved output and downloads
refuse to overwrite an existing destination. Choose a new path instead of
discarding an existing file unintentionally.

## Report a reproducible bug

Include the CLI version, OS/CPU, command shape, exit status and sanitized
diagnostics. Include operation identifiers only when your project's sharing
policy permits them. Omit credentials, private profile contents and sensitive
task data. Use [security reporting](../SECURITY.md) for vulnerabilities rather
than posting secrets in a public issue.
