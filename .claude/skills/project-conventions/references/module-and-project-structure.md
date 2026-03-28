# Module & Project Structure — boilerplate-rust

> **Shared patterns**: See `~/.claude/skills/rust-project-conventions/references/module-structure.md`
> for visibility rules, mod.rs re-export pattern, size limits, CLI design, and clippy configuration.

## Project Source Layout

```
src/
  main.rs              # CLI entry point (clap derive)
  libs.rs              # Top-level library module
  libs/
    hello.rs           # Greeting logic (example module)
tests/
  integration_test.rs  # Integration tests (assert_cmd)
ast-rules/
  *.yml                # Custom ast-grep lint rules
```

## OTel / Tracing Setup

- OTel is enabled by default (`default = ["otel"]`).
- Set `OTEL_EXPORTER_OTLP_ENDPOINT` env var to activate OTLP export.
- Without the env var (or empty), only the `fmt` layer is active.
- Build without OTel: `mise run build -- --no-default-features`.
- Test tasks automatically set `OTEL_EXPORTER_OTLP_ENDPOINT=""` to prevent OTel panics.
- Feature flag in `Cargo.toml`:
  ```toml
  [features]
  default = ["otel"]
  otel = [
  	"dep:opentelemetry",
  	"dep:opentelemetry_sdk",
  	"dep:opentelemetry-otlp",
  	"dep:tracing-opentelemetry",
  ]
  ```
