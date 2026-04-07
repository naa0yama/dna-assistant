# Testing Patterns — Project-Specific

> **Shared templates**: See `~/.claude/skills/rust-coding/references/testing-templates.md`
> for unit test, async test, integration test templates, fixtures, coverage rules,
> and ETXTBSY workaround.

## Miri Compatibility

For universal Miri rules and decision flowchart, see
`~/.claude/skills/rust-implementation/references/testing.md` → "Miri" section.

### Crate-Level Exclusions

| Crate                       | Reason                                     | Tests |
| --------------------------- | ------------------------------------------ | ----- |
| `dna-capture`               | Windows FFI (`windows`, `windows-capture`) | 0     |
| `dna-assistant` (src-tauri) | Tauri runtime, Windows APIs                | 18    |

### Per-Crate Miri Strategy

| Crate                       | Miri | Reason                                                             |
| --------------------------- | ---- | ------------------------------------------------------------------ |
| `dna-detector`              | Yes  | Pure Rust. PNG fixture tests skipped (image crate I/O too slow).   |
| `dna-capture`               | No   | Windows FFI (`windows`, `windows-capture`). Entire crate excluded. |
| `dna-assistant` (src-tauri) | No   | Tauri runtime, Windows APIs. Entire crate excluded.                |

### Per-Test Skip Categories (dna-detector)

1. **Image I/O** — Tests loading PNG fixtures via `image::open()`. PNG decoding under Miri is extremely slow. Skipped with `#[cfg_attr(miri, ignore)]`. Detection logic itself is covered by unit tests using `RgbaImage::new()`.
2. **Floating-point edge cases** — HSV conversion tests. Miri-safe (no FFI).

### Statistics

| Metric                      | Count |
| --------------------------- | ----- |
| Total tests                 | 94    |
| Miri-compatible             | 64    |
| Miri-ignored (per-test)     | 12    |
| Miri-excluded (crate-level) | 18    |
