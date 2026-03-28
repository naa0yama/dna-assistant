---
name: project-conventions
description: >-
  Project-specific conventions for the boilerplate-rust Rust CLI. Overrides
  and extends the shared rust-project-conventions skill with project-specific
  commands, OTel configuration, and project structure. Use when writing,
  reviewing, or modifying .rs files, running builds/tests, or creating commits.
  Complements rust-implementation with project-specific rules.
license: AGPL-3.0
---

# Project Conventions — boilerplate-rust (Override)

> **Base rules**: See `~/.claude/skills/rust-project-conventions/SKILL.md` for
> shared conventions (error context, logging, imports, workflow, comments,
> commits, async rules, ast-grep rules).

## Commands: mise Only

Never run `cargo` directly. All tasks go through `mise run`:

| Task           | Command                                   |
| -------------- | ----------------------------------------- |
| Build          | `mise run build`                          |
| Test           | `mise run test`                           |
| TDD watch      | `mise run test:watch`                     |
| Doc tests      | `mise run test:doc`                       |
| Trace test     | `mise run test:trace`                     |
| Format         | `mise run fmt`                            |
| Format check   | `mise run fmt:check`                      |
| Lint (clippy)  | `mise run clippy`                         |
| Lint strict    | `mise run clippy:strict`                  |
| AST rules      | `mise run ast-grep`                       |
| Pre-commit     | `mise run pre-commit`                     |
| Coverage       | `mise run coverage`                       |
| Deny           | `mise run deny`                           |
| Build w/o OTel | `mise run build -- --no-default-features` |

## Reference Files

| Topic                      | File                                                                       |
| -------------------------- | -------------------------------------------------------------------------- |
| Testing patterns & Miri    | `references/testing-patterns.md`                                           |
| Project source layout      | `references/module-and-project-structure.md`                               |
| Module structure (shared)  | `~/.claude/skills/rust-project-conventions/references/module-structure.md` |
| ast-grep rules (shared)    | `~/.claude/skills/rust-project-conventions/references/ast-grep-rules.md`   |
| Testing templates (shared) | `~/.claude/skills/rust-coding/references/testing-templates.md`             |
