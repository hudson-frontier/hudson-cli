# Homebrew distribution

Hudson's Homebrew formula installs the `hudson` executable and Bash, Zsh and Fish
completion. It includes the CLI guides and licensing. The optional `hudson-cli`
and `hudsonbox` compatibility executables remain available in native archives and
through Cargo source installation.

For a published tap release:

```sh
brew install hudson-frontier/tap/hudson
hudson --version
```

`brew update` followed by `brew upgrade hudson-frontier/tap/hudson` upgrades the
Homebrew-managed client. `brew uninstall hudson` removes that installation and
its completion files. Profiles, credentials, request keys and service operations
remain separately owned. Installation never starts a backend service or VM.

## Source and bottles

The formula pins a full CLI source commit, source archive SHA-256 and explicit
release version. Its locked Cargo build installs only the primary executable.
Source installation uses Homebrew's Rust build dependency, fetches the locked
dependencies first, then builds and tests without network access. A matching prebuilt
bottle avoids a Rust build during ordinary installation.

Native CI builds and tests bottles on macOS 15 Apple silicon and Intel, and
Linux x86_64 on Ubuntu 22.04. It tests offline help, private request-key creation
and overwrite rejection, then uninstalls the source build, pours the actual
bottle and repeats the functional test. The installed receipt must identify a
bottle installation at the selected version. No system outside that tested
Homebrew configuration is qualified by these checks. Native archives retain
their separately documented platform and signing limits.

The tap becomes available after an authorized CLI release and its Homebrew
publication. CI artifacts and a merged packaging change do not publish a release.

## Prepare a release

Run the complete [release qualification](releasing.md) for current `main`. The
existing native validation jobs also retain `homebrew-*` artifacts for the three
bottle targets. Every native and policy job must pass; no publishing job rebuilds
the application.

After the matching main CI run succeeds, prepare a reviewable bundle:

```console
gh workflow run homebrew.yml --repo hudson-frontier/hudson-cli --ref main -f ci_run=CI_RUN_ID
```

This read-only workflow verifies the CI source and complete platform gate, checks
all three formula/source/version/bottle identities and fresh-install receipts,
then retains `homebrew-publication-ready`. The bundle includes `tap/` with the
formula, bottle block, documentation and tap CI; `bottles/` with upload filenames;
and `publication.json` with source and payload digests. Source downloads are
compared with the committed Git tree without extracting downloaded files.

## Publish an approved bundle

Publication requires a separate explicit release request. Publish the matching
CLI version through its existing verified draft workflow first. Verify that its
public tag resolves to the bundle's source commit. Never substitute another
commit under the same version or replace an existing archive or bottle.

Create the `hudson-frontier/homebrew-tap` public repository with an initial README
if it does not exist. Publish the three already tested bottle files in a new
release at `vVERSION` in that repository. Download the uploaded files and compare
each SHA-256 with `publication.json` before enabling the formula. Existing
conflicting tags/releases require investigation rather than replacement.

Put the reviewed `tap/` contents on a focused branch in the tap repository.
Wait for all native branch checks before opening its PR. Those checks audit the
formula, require a bottle installation from its published checksummed URL, and
run the same offline functional test. Squash merge only after current-head PR
checks pass and discussions are resolved. The resulting install command is the
one shown above. Preserve old published bottles for existing pinned installations.

Each later release repeats this workflow with its own immutable source and
checked bytes. The package does not update itself or automatically rewrite shell
startup files.
