# Package registry policy

Selected workspace crates publish to the Cargo registry named `proa`,
configured in [`.cargo/config.toml`](../.cargo/config.toml). That file contains
the registry endpoint, not credentials.

A source checkout does not need registry credentials. Workspace crates resolve
one another through in-repository path dependencies. For dependency changes
when upgrading, see the [0.3 migration guide](migrations/0.3-standalone.md).

## Adding or changing a package

For a publishable crate:

1. Inherit the workspace's explicit `publish` policy.
2. Give publishable path dependencies matching version and registry metadata.
3. Add the crate to [`release-plz.toml`](../release-plz.toml).
4. Run the publication checks below and inspect the archive contents.

Set `publish = false` for demos, benchmarks, and other repository-only packages.

```bash
python3 scripts/check_repository_boundary.py
python3 scripts/check_publish_policy.py
python3 scripts/check_crate_archives.py
cargo package -p <crate> --list
```

The policy check verifies package and release configuration. The archive check
verifies the packaged source boundary, metadata, and canonical Apache-2.0
license. See [`scripts/README.md`](../scripts/README.md) for the release tools.

## Maintainer operations

Release sequencing, bootstrap records, infrastructure details, and credential
issuance or rotation procedures belong in the private knowledge base maintained
by the release owner. They are not requirements for using or contributing to
Proa.

Publishers supply credentials through Cargo's standard credential mechanisms
or the CI secret store. Never commit tokens, private account identifiers, or
workstation-specific credential paths. Report vulnerabilities through
[`SECURITY.md`](../SECURITY.md).
