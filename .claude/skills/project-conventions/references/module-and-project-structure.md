# Module & Project Structure — dna-assistant

> **Shared patterns**: See `~/.claude/skills/rust-project-conventions/references/module-structure.md`
> for visibility rules, mod.rs re-export pattern, size limits, and clippy configuration.

## Workspace Layout

```
dna-assistant/
├── crates/
│   ├── dna-detector/          # Cross-platform (DevContainer/WSL2 OK)
│   │   └── src/
│   │       ├── lib.rs         # Module declarations
│   │       ├── roi.rs         # ROI ratio-based definition + crop
│   │       ├── color.rs       # RGB→HSV conversion, color matching
│   │       ├── config.rs      # Detection config types (serde)
│   │       ├── event.rs       # DetectionEvent enum
│   │       ├── state.rs       # Debounce / state machine
│   │       └── detector/
│   │           ├── mod.rs     # Detector trait
│   │           ├── skill.rs   # Skill ON/OFF (gold HSV ratio)
│   │           ├── ally_hp.rs # Ally HP (HP bar color area)
│   │           └── round.rs   # Round (white pixel density)
│   └── dna-capture/           # Windows only
│       └── src/
│           ├── lib.rs
│           ├── wgc.rs         # Windows Graphics Capture
│           ├── printwindow.rs # PrintWindow fallback
│           ├── window.rs      # Game window discovery
│           └── ocr.rs         # Windows.Media.Ocr
├── src-tauri/                 # Windows only (Tauri v2)
│   └── src/
│       ├── main.rs            # Entry point
│       ├── lib.rs             # Tauri builder + commands
│       ├── commands.rs        # IPC command handlers
│       ├── monitor.rs         # Capture → detect → notify loop
│       └── notification.rs    # Toast notification manager
├── ui/                        # Static frontend (DevContainer OK)
│   ├── index.html
│   ├── main.js
│   └── styles.css
└── ast-rules/                 # Custom ast-grep lint rules
```

## Crate Dependencies (Data Flow)

```
dna-capture ──→ image::RgbaImage ──→ dna-detector ──→ DetectionEvent
     (Windows API)    (shared type)     (pure logic)      (result)

src-tauri depends on both; dna-capture and dna-detector do NOT depend on each other.
```
