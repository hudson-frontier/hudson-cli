# Repository instructions

Read CONTRIBUTING.md and docs/architecture.md before implementation work.
Use focused branches and preserve command, credential and exit-code contracts.
Run formatting, strict Clippy and all target tests with Rust 1.92.0.
Wait for branch CI success before creating a PR; squash merge only after latest
PR checks pass, the branch is current and review conversations are resolved.
Keep distinct functional fixes in distinct PRs. Never push directly to main or
bypass checks. Do not publish releases without an explicit release request.
