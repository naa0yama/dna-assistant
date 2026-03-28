# Testing Patterns — Project-Specific

> **Shared templates**: See `~/.claude/skills/rust-coding/references/testing-templates.md`
> for unit test, async test, integration test templates, fixtures, coverage rules,
> and ETXTBSY workaround.

## Miri Compatibility

For universal Miri rules and decision flowchart, see
`~/.claude/skills/rust-implementation/references/testing.md` → "Miri" section.

### Per-Test Skip Categories

1. **File system (tempfile)** — Tests using `tempfile::tempdir()` or real file I/O. Miri has limited file system support.
2. **FFI / C bindings (rusqlite)** — All tests use SQLite via C FFI. Entire crate excluded from Miri CI.
3. **Network I/O (reqwest, wiremock)** — HTTP client and mock server use unsupported socket syscalls.
4. **Process spawning (Command)** — Tests that execute external tools via `std::process::Command`.
5. **TLS / Crypto (reqwest + rustls)** — included in Network I/O count. TLS initialization is extremely slow under Miri (~10 min/call).
6. **Regex compilation** — included in tests that indirectly trigger `regex::Regex::new()`. DFA construction under interpretation is extremely slow (~2-6 min/test).
7. **Environment variables** — Tests calling `std::env::set_var` or relying on `HOME`/`current_dir`.
