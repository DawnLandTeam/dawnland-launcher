# Dawnland Launcher — Agent Guidelines

## Project State
Pre-scaffold. No `Cargo.toml`, `package.json`, or source code yet. See `docs/ARCHITECTURE.md` for the full roadmap and planned module boundaries.

## Tech Stack
- **Backend**: Rust + Tauri v2 + Tokio
- **Frontend**: Vue 3 (`<script setup lang="ts">` only) + TypeScript + Vite
- **Styling**: Tailwind CSS + Shadcn-Vue
- **IPC**: Tauri v2 commands and events

## Critical Constraints

### Frontend
- **No Node.js APIs** — Tauri has no Node runtime. Do not use `fs`, `path`, or `child_process`.
- **Tauri v2 Plugins** — Tauri v2 moved many APIs to plugins! Use `@tauri-apps/api/core` for `invoke`. For files and paths, you MUST use `@tauri-apps/plugin-fs` and `@tauri-apps/plugin-path`.
- **No Options API** — only `<script setup lang="ts">`. No `data()`, `methods`, `created()`.
- **No `any` type** — all IPC return values and external API shapes must have explicit TS types.
- **No raw CSS or `<style scoped>` blocks** — use Tailwind utility classes.
- **State**: `ref`/`reactive` first, Pinia for complex global state.

### Backend
- **No blocking calls in async context** — use `tokio::fs` and `tokio::time::sleep`, not `std::fs`/`std::thread::sleep`.
- **Process Spawning** — For launching Java/Minecraft, prefer `tokio::process::Command` to avoid blocking the async runtime.
- **No `println!`/`dbg!`** — use `tracing` macros (`info!`, `warn!`, `error!`, `debug!`).
- **No `panic!`/`unwrap()` in commands** — all Tauri commands must return `Result<T, String>` or `Result<T, AppError>`. (Note: `AppError` must derive `serde::Serialize`).
- **Path joining**: always use `PathBuf`, not string concatenation.

### Architecture Boundary
- **Rust owns all OS operations** (filesystem, processes, network, config). Frontend only dispatches via `invoke` and receives state.
- **Long tasks use events, not polling** — Rust pushes progress via `Window::emit`, frontend listens via `@tauri-apps/api/event`.

## Workflow
1. **Read before modify** — understand existing IPC bindings and state management before changing modules.
2. **Minimal changes** — only modify what the task requires.
3. **No placeholder code** — if you can't implement fully, flag it explicitly rather than leaving `// TODO`.
4. **Ask when uncertain** — if facing ambiguous choices around Rust lifetimes, Tauri v2 plugin configurations, or Vue component communication, present options and ask.
```