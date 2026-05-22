# Dawnland Launcher — Agent 指令规范

## 项目当前状态
处于脚手架搭建前（Pre-scaffold）。目前还没有 `Cargo.toml`、`package.json` 或任何源代码。请参阅 `docs/ARCHITECTURE.md` 了解完整的开发路线图和模块边界规划。

## 核心技术栈
- **后端**: Rust + Tauri v2 + Tokio
- **前端**: Vue 3 (仅限 `<script setup lang="ts">`) + TypeScript + Vite
- **样式**: Tailwind CSS + Shadcn-Vue
- **IPC**: Tauri v2 commands 和 events

## 核心硬性约束 (Critical Constraints)

### 前端开发 (Frontend)
- **禁止使用 Node.js API** — Tauri 环境下没有 Node 运行时。绝不允许使用 `fs`、`path` 或 `child_process`。
- **注意 Tauri v2 插件化变更** — Tauri v2 将大量核心 API 移至了独立插件！必须使用 `@tauri-apps/api/core` 调用 `invoke`。进行文件和路径操作时，**必须**使用 `@tauri-apps/plugin-fs` 和 `@tauri-apps/plugin-path`。
- **禁止使用 Options API** — 仅允许使用 `<script setup lang="ts">`。绝对不要写 `data()`、`methods` 或 `created()`。
- **禁止使用 `any` 类型** — 所有的 IPC 返回值和外部 API 数据结构都必须具备明确的 TS 类型定义。
- **禁止使用原生 CSS 或 `<style scoped>`** — 强制使用 Tailwind 工具类进行样式排版。
- **状态管理**：优先使用 `ref`/`reactive`，只有面对复杂的全局状态时才使用 Pinia。

### 后端开发 (Backend)
- **禁止在异步上下文中阻塞** — 必须使用 `tokio::fs` 和 `tokio::time::sleep`，严禁使用 `std::fs` 或 `std::thread::sleep`。
- **进程启动** — 启动 Java/Minecraft 进程时，优先使用 `tokio::process::Command`，防止阻塞异步运行时。
- **禁止使用 `println!`/`dbg!`** — 生产环境日志强制使用 `tracing` 宏 (`info!`, `warn!`, `error!`, `debug!`)。
- **Tauri Command 中禁止 `panic!` 或 `unwrap()`** — 所有抛出给前端的命令必须返回 `Result<T, String>` 或 `Result<T, AppError>`。（注意：自定义的 `AppError` 必须实现 `serde::Serialize` 才能跨 IPC 传输）。
- **路径拼接**：所有路径处理必须使用 `PathBuf`，严禁使用字符串拼接路径。

### 架构边界 (Architecture Boundary)
- **Rust 掌控所有系统级操作**（文件系统、进程管理、网络请求、配置读写）。前端是纯粹的展现层，仅通过 `invoke` 下发指令并接收状态。
- **耗时任务必须使用事件驱动，禁止轮询** — 对于耗时任务，Rust 必须通过 `Window::emit` 主动推送进度，前端通过 `@tauri-apps/api/event` 监听。

## 执行工作流 (Workflow)
1. **先阅读后修改** — 在修改模块前，必须先理解现有的 IPC 绑定逻辑和状态管理链路。
2. **最小侵入原则** — 仅修改完成当前任务所必须的代码，不要为了炫技重构无关代码。
3. **禁止幽灵代码** — 如果你无法写出完整的实现，请明确指出并询问，严禁用 `// TODO` 或 `...` 敷衍占位。
4. **遇事不决先询问** — 当面临 Rust 生命周期冲突、Tauri v2 插件配置歧义或 Vue 架构层面的艰难选择时，请先列出你的几个方案，询问用户的决定，不要盲目乱猜。
```