# Security

Do not include credentials or private endpoint details in a public issue. Use
GitHub private vulnerability reporting when it is enabled for this repository;
otherwise request a private reporting channel from an organization maintainer.

Agent bearer tokens are read from a named environment variable. Sandbox
credentials and trust configuration are read from private operator-owned files.
Product adapters keep their authentication boundaries separate. Neither client
grants authority beyond the configured service credential.

Sandbox requests use HTTPS, reject redirects and disable automatic retries.
Platform support must preserve the SDK's credential-file protections. Windows
sandbox profile loading currently fails closed; do not work around it by loading
credentials through an unchecked path.
