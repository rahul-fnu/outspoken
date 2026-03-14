---
tracker:
  kind: github
  repo: rahul-fnu/outspoken
  labels: [sub-issue]
  auto_close: true
  in_progress_label: in-progress
  done_label: "forge:done"
agent:
  type: claude-code
  timeout: 30m
concurrency:
  max_agents: 1
skills: [gsd, get-shit-done]
validation:
  steps:
    - name: cargo-check
      command: "cd src-tauri && cargo check 2>&1 | tail -30 || true"
      retries: 2
      description: "Verify Rust code compiles"
    - name: frontend-build
      command: "npm run build 2>&1 | tail -20 || true"
      retries: 1
      description: "Verify React frontend builds"
  on_failure: output-wip
---
You are an autonomous coding agent building **Outspoken**, a cross-platform AI dictation app.

## Issue: {{issue.title}}

{{issue.description}}

## Tech Stack
- **Desktop framework:** Tauri v2 (Rust backend + web frontend)
- **Backend:** Rust (audio capture via cpal, transcription via whisper-rs)
- **Frontend:** React + TypeScript + Vite
- **Speech-to-text:** whisper-rs (Rust bindings for whisper.cpp)
- **Storage:** SQLite via rusqlite
- **Build:** cargo (Rust) + npm (frontend) + Tauri CLI

## Project Structure
```
outspoken/
├── src-tauri/           # Rust backend
│   ├── src/
│   │   ├── main.rs      # Tauri app entry
│   │   ├── audio.rs     # Microphone capture (cpal)
│   │   ├── transcription.rs  # Whisper service
│   │   ├── models.rs    # Model download/management
│   │   └── lib.rs       # Tauri commands
│   ├── Cargo.toml
│   └── tauri.conf.json
├── src/                 # React frontend
│   ├── App.tsx
│   ├── components/
│   └── main.tsx
├── package.json
└── Dockerfile
```

## Instructions
1. Read existing source files to understand what's already built before making changes.
2. Follow Rust conventions: use Result for errors, async with tokio, traits for abstraction.
3. Tauri commands go in `src-tauri/src/lib.rs` or dedicated modules, exposed via `#[tauri::command]`.
4. React frontend communicates with Rust via `@tauri-apps/api/core` invoke calls.
5. Use `tokio::task::spawn_blocking` for CPU-heavy work (whisper transcription).
6. Write minimal, focused code. Don't over-abstract.
7. Commit your changes with clear messages describing what was built.
8. If this is the first issue (#60), scaffold the full Tauri v2 project with `cargo create-tauri-app`.
