# Project Summary

DNA Assistant — Duet Night Abyss screen monitor with Windows Toast notifications.

- Think in English, explain, and respond to chat in Japanese.
- Use half-width brackets instead of full-width brackets in the Japanese explanations output.
- When writing Japanese and half-width alphanumeric characters or codes in one sentence, please enclose the half-width alphanumeric characters in backquotes and leave half-width spaces before and after them.

## Architecture

Multi-crate workspace with platform separation:

| Crate           | Path                   | Platform       | Purpose                                 |
| --------------- | ---------------------- | -------------- | --------------------------------------- |
| `dna-detector`  | `crates/dna-detector/` | Cross-platform | Detection logic (ROI, color, detectors) |
| `dna-capture`   | `crates/dna-capture/`  | Windows only   | Screen capture (WGC, PrintWindow), OCR  |
| `dna-assistant` | `src-tauri/`           | Windows only   | Tauri v2 app (IPC, notifications, tray) |

Frontend: `ui/` — static HTML + HTMX (CDN) + DaisyUI (CDN), no Node.js.

Data flow: `dna-capture` → `image::RgbaImage` → `dna-detector` → `DetectionEvent` → `src-tauri` (notifications/UI).

## Commands

All tasks use `mise run <task>`:

| Task                          | Command                       |
| ----------------------------- | ----------------------------- |
| Setup                         | `mise run setup`              |
| Build                         | `mise run build`              |
| Build (release/Tauri)         | `mise run build:release`      |
| Build (timings)               | `mise run build:timings`      |
| Check (workspace)             | `mise run check`              |
| Check (core, DevContainer OK) | `mise run check:core`         |
| Test (workspace)              | `mise run test`               |
| Test (core, DevContainer OK)  | `mise run test:core`          |
| TDD watch (core)              | `mise run test:watch`         |
| Doc tests                     | `mise run test:doc`           |
| Format                        | `mise run fmt`                |
| Format check                  | `mise run fmt:check`          |
| Lint (clippy)                 | `mise run clippy`             |
| Lint strict                   | `mise run clippy:strict`      |
| Lint core (DevContainer OK)   | `mise run clippy:core`        |
| Lint                          | `mise run lint`               |
| Lint (GitHub Actions)         | `mise run lint:gh`            |
| AST rules                     | `mise run ast-grep`           |
| Pre-commit (required)         | `mise run pre-commit`         |
| Pre-push                      | `mise run pre-push`           |
| Coverage                      | `mise run coverage`           |
| Coverage (HTML)               | `mise run coverage:html`      |
| Audit                         | `mise run audit`              |
| Deny (licenses/deps)          | `mise run deny`               |
| Miri (workspace)              | `mise run miri`               |
| Miri (core, DevContainer OK)  | `mise run miri:core`          |
| Tauri dev (Windows only)      | `mise run tauri:dev`          |
| Tauri build (Windows only)    | `mise run tauri:build`        |
| Clean (full)                  | `mise run clean`              |
| Clean (sweep)                 | `mise run clean:sweep`        |
| Clean (cache)                 | `mise run clean:cache`        |
| Trace test (OTel)             | `mise run test:trace`         |
| Badges (init)                 | `mise run badges:init`        |
| Claude Code (install)         | `mise run claudecode:install` |
| O2 (install)                  | `mise run o2:install`         |
| O2 (start)                    | `mise run o2`                 |
| O2 (stop)                     | `mise run o2:stop`            |
| CodeQL (install)              | `mise run codeql:install`     |
| CodeQL (analyze)              | `mise run codeql`             |

## Commit Convention

Conventional Commits: `<type>: <description>` or `<type>(<scope>): <description>`

Allowed types: feat, update, fix, style, refactor, docs, perf, test, build, ci, chore, remove, revert

## Workflow

1. Write tests (for new features / bug fixes)
2. Implement
3. Run `mise run test` — all tests must pass
4. Stage only the relevant files
5. Run `mise run pre-commit` (runs fmt:check, clippy:strict, ast-grep, lint:gh)
6. If errors, fix → re-stage → re-run `mise run pre-commit`

## Code Comments

- Write all code comments (doc comments, inline comments) in concise English.

## Skill Maintenance

- **Global skills** (`~/.claude/skills/`): Shared across all Rust projects. Update these when changing rules that apply universally (error handling, import grouping, test templates, ast-grep rules, workflow agents).
  - `rust-implementation/` — idiomatic Rust patterns (naming, types, errors, testing, CLI design)
  - `rust-project-conventions/` — shared base rules (error context, logging, imports, async)
  - `rust-qa/`, `rust-review/`, `rust-docs/` — QA / review / docs agents
  - `deps-sync/`, `deps-sync-mise/` — dependency sync (language-agnostic)
  - `rust-deps-sync/`, `rust-deps-sync-crates/`, `rust-deps-sync-tests/` — Rust dependency sync
  - `jaeger-trace/`, `o2-trace/` — trace analysis agents
- **Project skills** (`.claude/skills/`): Project-specific overrides only.
  - `project-conventions/` — project name, command table, OTel config, Miri categories, module layout
  - `lib-*/`, `tool-*/` — auto-generated by `/deps-sync`
- When modifying coding rules in `CLAUDE.md`, update the corresponding skill files:
  - Universal rules → `~/.claude/skills/rust-project-conventions/`
  - Project-specific rules → `.claude/skills/project-conventions/`
