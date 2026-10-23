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

## Public reference patterns

OpenAI publishes a dedicated [OpenAI CLI repository](https://github.com/openai/openai-cli).
Its [go.mod](https://github.com/openai/openai-cli/blob/main/go.mod) pins a Go SDK
dependency. Its [contributor guide](https://github.com/openai/openai-cli/blob/main/CONTRIBUTING.md)
separates generated API commands, handwritten features and executable wiring;
its [GoReleaser configuration](https://github.com/openai/openai-cli/blob/main/.goreleaser.yml)
owns platform builds and packaging. The [official CLI documentation](https://developers.openai.com/api/docs/libraries/openai-cli)
documents standalone installation and resource-based commands.

We adopt those repository boundaries: a separately versioned executable,
explicit SDK dependencies, isolated product adapters and independent delivery.
We retain Rust to reuse the current clients and tests. These public sources show
repository structure; they do not establish OpenAI's internal team organization.
No OpenAI implementation code is included.
