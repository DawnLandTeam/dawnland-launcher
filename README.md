# 🌅 Dawnland Launcher

Dawnland Launcher 是一款基于 **Tauri v2 + Vue 3 + TypeScript** 打造的高性能跨平台游戏启动器，专为Minecraft玩家及相关游戏生态设计。

## ✨ 核心特性

- **🚀 极速轻量**：基于 Rust & Tauri 构建，内存占用远低于 Electron，启动秒开。
- **🔄 增量热更**：内置基于 Tauri Updater 的无感增量更新机制，配合 Cloudflare R2 节点加速，实现国内秒级更新。
- **💻 跨平台支持**：支持 Windows (x64/arm64)、~~macOS~~ 和 Linux。
- **🤖 全自动构建**：配置了完整的 GitHub Actions CI/CD 流水线，提交代码自动编译、签名并发布。
- **🔐 安全认证**：深度整合 Microsoft OAuth 登录机制与正版验证。
- **🎨 现代 UI**：基于 Vue 3 + Vite 驱动的流畅响应式前端界面。

## 📦 项目架构

本项目部分模块需要与配套的后端服务配合运行。
后端服务采用 Go (Gin + GORM) 编写，主要负责提供版本检测、服务器模块及 R2 加速节点分发。

## 🛠️ 本地开发指南

### 1. 环境准备
确保您的计算机已安装以下依赖：
- **Node.js** (推荐 v20 LTS 或以上)
- **pnpm** (`npm install -g pnpm`)
- **Rust** 及其编译环境 ([rustup](https://rustup.rs/))
- *(Windows)* C++ 编译工具链 (Visual Studio Build Tools)
- *(Linux)* `libwebkit2gtk-4.1-dev`, `build-essential`, `curl`, `wget`, `file`, `libxdo-dev`, `libssl-dev`, `libayatana-appindicator3-dev`, `librsvg2-dev`

### 2. 安装依赖
```bash
pnpm install
```

### 3. 本地启动开发服务器
```bash
pnpm tauri dev
```

### 4. 生产环境构建
```bash
pnpm tauri build
```
编译产物将会生成在 `src-tauri/target/release/bundle/` 目录下（包含便携版 `.exe` 与安装版 `.msi`）。

## 📝 开源协议

本项目基于 **Apache License 2.0** 协议开源，欢迎提交 Issue 和 PR！
