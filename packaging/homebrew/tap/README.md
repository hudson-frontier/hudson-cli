# Hudson Homebrew tap

Install the published Hudson CLI and shell completions:

```sh
brew install hudson-frontier/tap/hudson
hudson --version
```

Upgrade or remove the Homebrew-managed installation:

```sh
brew update
brew upgrade hudson-frontier/tap/hudson
brew uninstall hudson
```

Profiles, credentials, request keys and service operations are managed separately
and remain after removing the client. This formula installs `hudson`; optional
`hudson-cli` and `hudsonbox` compatibility names are available in the upstream
archives or through Cargo source installation.

The formula pins immutable CLI source bytes and native bottle checksums. Matching
bottles provide macOS Apple silicon and Linux x86_64 installations, with a pinned
source build available on those platforms. Intel Mac, Windows and Linux aarch64
archives remain available from
[Hudson CLI releases](https://github.com/hudson-frontier/hudson-cli/releases).

## Maintenance

Prepare formulae and tested bottles through the CLI repository's qualified
main-branch CI and manual Homebrew preparation workflow. Publish only candidates
matching an explicitly authorized, published CLI release. Never move an existing
release or replace its bytes. Review the prepared formula, checksums and source
manifest before publishing. Wait for all tap branch checks before creating a PR,
then squash merge after current-head checks pass and discussions are resolved.

See the [CLI release procedure](https://github.com/hudson-frontier/hudson-cli/blob/main/docs/homebrew.md).
