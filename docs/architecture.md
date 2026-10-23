# Architecture and ownership

This repository is the executable product maintained by the CLI team. Its initial
adapters expose two equal product namespaces:

| Namespace | Adapter | Contract owner |
| --- | --- | --- |
| `hudson agent` | `src/agent` | Hudson agent API team |
| `hudson sandbox` | `src/sandbox.rs` | Hudsonbox API and SDK team |

`src/lib.rs` only parses and dispatches. Agent requests use the existing public
HTTP contract. Sandbox requests use `sandbox-client`, pinned to an immutable Git
revision in Cargo.toml and Cargo.lock. Generated SDK models remain upstream.
No backend databases, schedulers, policy evaluation or VM lifecycle code belongs
in this repository. Agent blocking HTTP work runs outside the sandbox async
runtime.

The initial agent adapter comes from Hudson revision
`44e81c64a43cadf61cefe42b077e7eb6997c7875`; the sandbox adapter comes from Hudsonbox
revision `df4b53cb8e3e22bd808b0e61f25ac43f3eaeb976`. Their Apache-2.0 attribution
is retained in NOTICE. The original product binaries remain intact during this
handoff. Changes made to either copy during the transition need coordination with
the CLI team; backend repositories can later replace their adapters with thin
compatibility shims after validating a released CLI dependency.

## Compatibility

`hudson` offers product namespaces and accepts valid legacy flat agent commands.
The dispatcher preserves valid legacy commands before trying the product parser,
before making any request. This retains the historical `hudson agent NAME
VERSION` lookup even for names such as `get`. `hudson-cli` always uses the legacy agent parser;
`hudsonbox` always uses the legacy sandbox parser. Help and version requests
remain offline. Installing only `--bin hudson` avoids replacing the optional
compatibility executables.

Authentication, request keys and output contracts remain product-specific.
Shared configuration must not make a credential for one service available to
another. Admission is distinct from completion, and automatic mutation retries
are forbidden. CI exercises the executable boundaries against local protocol
fixtures. Backend contract suites and runtime qualification remain in their
owning repositories.

## Command discovery and automation

Hudson's command model is the source for both help and shell completion.
`hudson completion SHELL` emits a script for the selected shell without loading
service configuration or credentials. It lists both product namespaces and their
operation flags. Operators decide where to save or load the script; generating
one never edits shell startup files.

Automation selects its input source explicitly. Sandbox create and execute accept
`--request -` for a single JSON document from stdin, with the same 64 KiB limit as
saved request files. Request content never triggers implicit local file reads.
Credentials still come from the private operator profile, and mutation keys remain
explicit. Upload paths and key-file paths retain their existing file semantics.

The CLI version, adapters, output contracts and delivery belong to this product.
Service API contracts and SDK wire models stay with their product owners. Rust
keeps the executable and existing clients in one toolchain, while immutable SDK
revisions make compatibility changes reviewable.

Agent API failures are checked before response decoding or output. Execution
errors use stderr; `--json` selects a stable error envelope for scripts without
changing ordinary successful API results or exit codes. Transport failures omit
request URLs, and server error bodies are not echoed into terminal output.
