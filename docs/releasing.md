# Release maintenance

A release promotes the exact archives produced by successful main-branch CI.
Publishing does not rebuild the client. Version metadata in Cargo.toml,
Cargo.lock, executable output and archive manifests must agree.

## Readiness

Before requesting publication:

1. Fix confirmed functional bugs in separate PRs, with regression tests.
2. Review README, installation, agent, sandbox, automation and recovery guides.
   Keep the command reference generated from current offline help.
3. Run formatting, strict Clippy, native release builds and all target tests.
4. Verify every archive, then rerun process tests against executables extracted
   into a fresh directory. Confirm the documented platform limits.
5. Check compatibility with the pinned product API/SDK revisions. Backend guest
   and hardware qualification belongs to the backend release, not the CLI tests.
6. Wait for every required check on the current main revision. Publication also
   requires an explicit release request; a merged feature PR does not authorize it.

## Build and inspect locally

Maintainer packaging scripts require Python 3.12 or newer and the pinned Rust
toolchain. Choose the target native to the machine:

```console
python scripts/ci_package.py --target x86_64-unknown-linux-gnu
```

This runs checks, builds optimized executables, generates completion, builds the
archive and verifies a fresh installation. `--allow-dirty` is available only for
local development; its manifest records that fact, and publication rejects it.
Use a new output directory or remove only your own prior build artifacts before
rebuilding an archive with the same filename.

To regenerate help after changing command definitions:

```console
cargo build --locked --bins
python scripts/generate_reference.py --binary target/debug/hudson
```

On Windows the binary filename ends in `.exe`. CI checks the committed reference
against the Linux build. Packaging includes README, the task guides, completion,
the source commit manifest, project licensing and dependency license notices.

## Supported packages

CI uses native Linux x86_64/aarch64, Mac Intel/Apple silicon and Windows x86_64
runners. Linux builds use Ubuntu 22.04. Mac builds set a macOS 12 deployment
target and receive ad-hoc signatures after stripping. Windows builds link the
Microsoft C runtime statically. No Developer ID notarization or Authenticode
signature is claimed. Native Windows sandbox profile loading remains unsupported.

Each archive has one versioned root directory with `bin`, `completions`, `docs`,
licenses and `manifest.json`. The manifest records version, source commit,
target, file modes, sizes and SHA-256 digests. Archives reject unexpected paths,
duplicates, symlinks and mismatched architecture or source metadata.

## Publish

After the authorized release is ready, locate the successful **CLI checks** push
run for the current main commit, then dispatch the publishing workflow:

```console
gh workflow run release.yml --repo hudson-frontier/hudson-cli --ref main -f ci_run=CI_RUN_ID
```

The workflow:

1. Requires a successful main push run with every native validation and policy
   job successful, and verifies that the source is still current main.
2. Downloads all five tested archives, validates their manifests and checksums,
   and creates the aggregate `SHA256SUMS` file with read-only permissions.
3. Creates or resumes only a matching draft and uploads the qualified files.
   Existing published releases and conflicting tags are never overwritten.
4. Downloads the uploaded draft files in a separate verification job and compares
   their bytes with the already installed and tested archives.
5. Rechecks source and CI, then publishes the verified draft and version tag.

Application executables run only during read-only build/install validation.
GitHub requires repository write permission to inspect an unpublished draft,
so draft administration, uploaded-asset verification and publication jobs have
that permission. Verification only reads metadata and downloads and hashes files.
These jobs do not run packaged application code or rebuild artifacts.

Failures before final publication leave any created release as a draft. Investigate the failed check and
retry failed jobs only after understanding the cause. Do not replace assets to
hide a checksum mismatch. A conflicting existing draft or tag requires maintainer
inspection. If a publication response is lost, inspect the release before retrying;
the publisher recognizes a matching published release without writing it again.
After publication, verify the public tag, assets and installation
instructions; publish a new version for a correction instead of moving the tag.
