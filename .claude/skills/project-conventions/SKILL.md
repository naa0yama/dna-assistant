---
name: project-conventions
description: >-
  Project-specific conventions for dna-assistant, a Tauri v2 desktop app
  for monitoring Duet Night Abyss. Multi-crate workspace with platform
  separation: dna-detector (cross-platform), dna-capture (Windows only),
  src-tauri (Windows only). Use when writing, reviewing, or modifying
  .rs files, running builds/tests, or creating commits.
license: AGPL-3.0
metadata:
  updated: "2026-03-28T22:48:18+09:00"
---

# Project Conventions — dna-assistant (Override)

> **Base rules**: See `~/.claude/skills/rust-project-conventions/SKILL.md` for
> shared conventions (error context, logging, imports, workflow, comments,
> commits, async rules, ast-grep rules).

## Commands: mise Only

Never run `cargo` directly. All tasks go through `mise run`:

| Task                  | Command                  | Platform |
| --------------------- | ------------------------ | -------- |
| Build                 | `mise run build`         | Any      |
| Build (Tauri release) | `mise run build:release` | Windows  |
| Check (workspace)     | `mise run check`         | Windows  |
| Check (core)          | `mise run check:core`    | Any      |
| Test (workspace)      | `mise run test`          | Windows  |
| Test (core)           | `mise run test:core`     | Any      |
| TDD watch (core)      | `mise run test:watch`    | Any      |
| Doc tests             | `mise run test:doc`      | Any      |
| Format                | `mise run fmt`           | Any      |
| Format check          | `mise run fmt:check`     | Any      |
| Lint (clippy)         | `mise run clippy`        | Windows  |
| Lint strict           | `mise run clippy:strict` | Windows  |
| Lint core             | `mise run clippy:core`   | Any      |
| AST rules             | `mise run ast-grep`      | Any      |
| Pre-commit            | `mise run pre-commit`    | Any      |
| Coverage              | `mise run coverage`      | Windows  |
| Deny                  | `mise run deny`          | Any      |
| Miri (workspace)      | `mise run miri`          | Windows  |
| Miri (core)           | `mise run miri:core`     | Any      |
| Tauri dev             | `mise run tauri:dev`     | Windows  |
| Tauri build           | `mise run tauri:build`   | Windows  |

## Reference Files

| Topic                      | File                                                                       |
| -------------------------- | -------------------------------------------------------------------------- |
| Testing patterns & Miri    | `references/testing-patterns.md`                                           |
| Project source layout      | `references/module-and-project-structure.md`                               |
| Module structure (shared)  | `~/.claude/skills/rust-project-conventions/references/module-structure.md` |
| ast-grep rules (shared)    | `~/.claude/skills/rust-project-conventions/references/ast-grep-rules.md`   |
| Testing templates (shared) | `~/.claude/skills/rust-coding/references/testing-templates.md`             |
