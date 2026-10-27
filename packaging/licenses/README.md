# Upstream license supplements

Some published crate archives omit the license file at their monorepo root.
These files preserve the exact upstream notices at the revisions recorded in
each crate's `.cargo_vcs_info.json`.

`manifest.json` binds each supplement to its package name, version, repository,
license declaration, VCS commit and path. It also records the immutable source
URL and SHA-256 of each notice. Packaging verifies those bindings offline and
still rejects any unknown missing notice or changed provenance.

The objc2 notice is its upstream licensing declaration, including the offered
license choices and Apple SDK context. It is retained verbatim.

When updating a dependency, inspect its published archive and corresponding
upstream revision before updating this manifest. Keep original copyright text
and exact bytes. The attributes for this directory disable line-ending
conversion so the checksums remain stable on every supported platform.
