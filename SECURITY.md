# Security

horizon-porter reads mod files and writes Rust source that becomes part of a Horizon
build. Generated native mods are not sandboxed, so treat a problem that could make the
tool write unexpected code, or write outside the folder you chose, as a security issue.

Please report it privately through GitHub's
[security advisories](https://github.com/kevinbonomi2-bit/horizon-porter/security/advisories/new)
rather than a public issue.
