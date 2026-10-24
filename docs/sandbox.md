# Sandbox workflows

The sandbox client uses the project HTTPS API. Your operator supplies the service
origin, project credential, allowed image digests and resource limits. The CLI
does not own VM configuration, host access or project authorization.

## Prepare a private profile

Keep credentials outside workload and untrusted project directories. On Unix,
create a private directory and copy the operator-provided credential into it:

```sh
umask 077
mkdir -p "$HOME/.config/hudson/sandbox"
chmod 700 "$HOME/.config/hudson/sandbox"
```

Save `credential.json` there with mode `0600`. Create `client.json` alongside it,
replacing the example origin with your operator's HTTPS origin:

```json
{
  "version": 1,
  "endpoint": "https://sandbox.example.com",
  "credential_file": "credential.json",
  "request_timeout_seconds": 30
}
```

```sh
chmod 600 "$HOME/.config/hudson/sandbox/client.json" "$HOME/.config/hudson/sandbox/credential.json"
export HUDSONBOX_CONFIG="$HOME/.config/hudson/sandbox/client.json"
hudson sandbox --json list
```

Profiles, credentials and optional CA files must be regular files owned by the
current user, with no group or other permissions. Symlinks are rejected. Relative
credential and CA paths resolve from the profile directory. Add `"ca_file":
"ca.pem"` only for an operator-provided trusted CA bundle, also mode `0600`.
Origins cannot include URL credentials, a path prefix, query or fragment.

Use `--config /private/client.json` to override the environment selection.
On WSL, store profiles on the Linux filesystem where these protections apply.
Native Windows profile loading is unsupported and fails closed.

## Create and inspect

Prepare `create.json`. This is a template: replace the digest placeholder with
an operator-approved `sha256:` digest containing 64 hexadecimal characters, and
choose resources within the project's policy:

```json
{
  "image_digest": "sha256:<approved-64-hex-digest>",
  "name": "work-session",
  "resources": {"vcpu": 1, "memory_mib": 128, "disk_mib": 64}
}
```

Save a mutation key before admitting the request:

```console
hudson sandbox key --to create.key
hudson sandbox --json create --request create.json --key-file create.key
```

Keep the returned `sandbox_id` and `operation_id`. Admission is not completion:

```console
hudson sandbox --json wait OPERATION_ID --seconds 60
hudson sandbox --json get SANDBOX_ID
hudson sandbox --json list --limit 20
hudson sandbox --json list --limit 20 --cursor RETURNED_CURSOR
hudson sandbox --json operations --sandbox-id SANDBOX_ID
hudson sandbox --json operation OPERATION_ID
```

Lists return one page. Pass `next_cursor` explicitly; the CLI does not silently
fetch every page. A successful inspection request does not itself mean the
inspected operation succeeded. Use `wait` and inspect its result.

## Execute with a generation precondition

Save the current sandbox view, then build one request file. This example needs
Python 3 and an image providing `/bin/echo`:

```sh
hudson sandbox --json get SANDBOX_ID > sandbox.json
python3 - <<'PY'
import json, time
with open('sandbox.json') as stream:
    generation = json.load(stream)['generation']
body = {'argv': ['/bin/echo', 'hello'],
        'deadline_unix_ms': int(time.time() * 1000) + 120000,
        'expected_generation': generation}
with open('execute.json', 'x') as stream:
    json.dump(body, stream)
PY
hudson sandbox key --to execute.key
hudson sandbox --json execute SANDBOX_ID --request execute.json --key-file execute.key
hudson sandbox --json wait OPERATION_ID --seconds 120
```

The precondition prevents a request from being applied to a different sandbox
generation. The absolute deadline belongs to this saved logical request. Do not
regenerate it while retrying with the same key. A new logical command needs a
new request file and key. Request JSON is capped at 64 KiB; `--request -` reads
one document from stdin instead of a saved file.

## Output and streaming

Save bounded retained output to a new local file:

```console
hudson sandbox --json output OPERATION_ID stdout --offset 0 --limit 32768 --to stdout.bin
hudson sandbox --json output OPERATION_ID stderr --offset 0 --limit 32768 --to stderr.bin
hudson sandbox stream OPERATION_ID --seconds 120
hudson sandbox stream OPERATION_ID --cursor SAVED_CURSOR --seconds 120
```

Output files are bytes, not terminal text. Range results include `next_offset`,
size, EOF and truncation metadata. Streaming emits NDJSON with base64 bytes and
resumable cursors. Save a cursor only after processing the associated record.
`--reconnects` bounds additional connection attempts, including transport failures
while opening a stream. Reconnects retain the last fully emitted cursor. HTTP
and protocol errors stop observation without retrying.
An `end` event means output completion; still inspect or wait for the operation
to establish command success. Gap and stream-error events return exit 9.

## Transfer files

```console
hudson sandbox key --to upload.key
hudson sandbox --json upload SANDBOX_ID /work/input.txt --from input.txt --mode 0644 --key-file upload.key
hudson sandbox --json wait UPLOAD_OPERATION_ID --seconds 60
hudson sandbox --json download SANDBOX_ID /work/result.txt --to result.txt
```

Upload admission returns before guest publication. Download captures a version
once, verifies its size and SHA-256, and then publishes a new private local file.
Downloads and saved output refuse to overwrite an existing path. The image and
service policy determine which guest paths are usable.

## Cancel or destroy

These are deliberate mutations. Save a different key for each logical action:

```console
hudson sandbox key --to cancel.key
hudson sandbox --json cancel COMMAND_OPERATION_ID --key-file cancel.key
hudson sandbox key --to destroy.key
hudson sandbox --json destroy SANDBOX_ID --key-file destroy.key
```

Cancellation returns a separate operation handle. Inspect both the cancellation
and target handles. Wait for destruction before treating a sandbox as removed.
Timeout or transport failure never means cancellation occurred automatically.

See [automation](automation.md) for exit codes and
[troubleshooting](troubleshooting.md) for reconciliation.
