# 🌅 Dawnland Launcher

[中文](README_zh.md) | [Official Website](https://dlml.app)

Dawnland Launcher is a high-performance cross-platform game launcher built with **Tauri v2 + Vue 3 + TypeScript**, designed specifically for Minecraft players and related game ecosystems.

## ✨ Core Features

- **🚀 Extremely Fast and Lightweight**: Built on Rust & Tauri, memory footprint is far lower than Electron, opens in seconds.
- **🔄 Incremental Hot Updates**: Built-in seamless incremental update mechanism based on Tauri Updater, accelerated by Cloudflare R2 nodes for seconds-level updates in mainland China.
- **💻 Cross-Platform Support**: Supports Windows (x64/arm64), ~~macOS~~, and Linux.
- **🤖 Automated Build**: Configured with a complete GitHub Actions CI/CD pipeline, automatically compiling, signing, and publishing upon code commit.
- **🔐 Secure Authentication**: Deeply integrated with Microsoft OAuth login mechanism and official authentication.
- **🎨 Modern UI**: Fluid responsive frontend interface driven by Vue 3 + Vite.

## 📦 Project Architecture

Some modules of this project need to run in conjunction with supporting backend services.
The backend service is written in Go (Gin + GORM), mainly responsible for providing version detection, server modules, and R2 accelerated node distribution.

## 🛠️ Local Development Guide

### 1. Environment Preparation
Make sure your computer has the following dependencies installed:
- **Node.js** (v20 LTS or above recommended)
- **pnpm** (`npm install -g pnpm`)
- **Rust** and its compilation environment ([rustup](https://rustup.rs/))
- *(Windows)* C++ compilation toolchain (Visual Studio Build Tools)
- *(Linux)* `libwebkit2gtk-4.1-dev`, `build-essential`, `curl`, `wget`, `file`, `libxdo-dev`, `libssl-dev`, `libayatana-appindicator3-dev`, `librsvg2-dev`

### 2. Install Dependencies
```bash
pnpm install
```

### 3. Start Local Development Server
```bash
pnpm tauri dev
```

### 4. Production Build
```bash
pnpm tauri build
```
The compiled artifacts will be generated in the `src-tauri/target/release/bundle/` directory (including the portable `.exe` and installer `.msi`).

## 📝 License

This project is open-sourced under the **Apache License 2.0**. Issues and PRs are welcome!
