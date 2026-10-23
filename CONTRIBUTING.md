# Contributing

The CLI team owns command design, configuration, terminal behavior, compatibility,
tests and packaging across products. Product maintainers own their services and
API contracts. Coordinate contract changes with the affected product team.

Use a focused branch. Run these checks with the pinned toolchain before pushing:

```console
cargo fmt --all -- --check
cargo clippy --all-targets --locked -- -D warnings
cargo build --bins --locked
cargo test --all-targets --locked
```

CI also validates the workflow with actionlint 1.7.12 and checks the PR title.

The process tests use local HTTP and self-signed HTTPS fixtures with synthetic
credentials. They check public requests and outputs without a product database,
model provider or virtual machine. These tests do not establish guest runtime or
hardware qualification.

Push the branch and wait for all CI jobs to pass before opening a PR. Use a
Conventional Commit PR title, for example `fix(sandbox): preserve operation exit
status`. Keep fixes for separate functional bugs in separate PRs. Squash merge
only after all checks pass on the latest PR revision, the branch is current with
`main`, and review conversations are resolved. Maintainers with write access may
merge their own PR after those gates; never bypass failed checks.

Changes to product adapters need process-level contract tests when behavior
changes. Retain compatibility aliases until a documented migration and removal
policy is agreed. Never put credentials in fixtures, logs or command arguments.

Dependency updates include an explicit SDK revision, the lockfile and matching
contract validation. Keep SDK wire types and server business rules in their
product repositories. Application Rust modules here are not a public SDK.

Release versions are independent of backend versions. CI builds and installs
native release archives on every supported target. See [release maintenance](docs/releasing.md)
for the qualification and draft publication gates. Publishing requires an
explicit release request; never publish tags or packages as an ordinary code
change. The documented archive support and signing limits must remain accurate.
