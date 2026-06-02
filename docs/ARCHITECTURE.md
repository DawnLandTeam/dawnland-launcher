# Dawnland Launcher 项目规划书

## 1. 项目概述
**项目名称**：Dawnland Launcher
**项目愿景**：打造一款极速、轻量、现代化的跨平台 Minecraft 启动器。结合 Rust 的底层性能与现代前端的 UI 表现力，为玩家提供一站式的整合包安装、环境部署以及行业首创的 **AI 崩溃分析** 体验。
**目标平台**：Windows, macOS (Intel & Apple Silicon), Linux

---

## 2. 技术选型 (前后端分离架构)
本项目采用桌面端经典的前后端分离方案（依托 Tauri）：

* **底层后端 (Core/Backend)**：**Rust**
    * **框架**：Tauri (负责跨平台窗口管理与系统原生 API 调用)
    * **网络请求**：`reqwest` (用于高速并发下载游戏文件和 API 请求)
    * **序列化**：`serde` / `serde_json`
    * **异步运行时**：`tokio`
* **展现前端 (UI/Frontend)**：**Vue 3** 或 **React** (推荐使用 TypeScript)
    * **构建工具**：Vite
    * **样式框架**：Tailwind CSS (极速构建现代、紧凑型 UI)
    * **组件库**：Radix UI 或 Shadcn-Vue / Shadcn-UI (提供无障碍且易于深度定制的紧凑现代组件)
* **前后端通信**：Tauri IPC (Inter-Process Communication) 命令调用与事件系统。

---

## 3. 核心功能需求

### 3.1 账号与身份验证 (Auth Module)
* **微软登录**：对接 Microsoft OAuth 2.0 与 Xbox Live 鉴权接口。
* **离线账号**：支持本地生成 UUID 的离线游戏模式。
* **第三方登录 (Authlib-injector)**：支持外置登录器协议（如 LittleSkin 等），允许自定义 Yggdrasil API 节点。

### 3.2 游戏核心部署 (Core Installation)
* **一键原版安装**：解析 Mojang 官方 `version_manifest_v2.json`，一键并发下载核心、Asset、Libraries 等资源。
* **Loader 自动化安装**：
    * 支持 **Forge** / **NeoForge** (解析并执行其 Installer 或自动部署)。
    * 支持 **Fabric** / **Quilt** (拉取 meta 接口自动生成对应版本 JSON)。

### 3.3 整合包生态支持 (Modpack Module)
* **CurseForge 支持**：对接 CurseForge API，支持输入项目 ID 或导入 `.zip` 压缩包一键解析与下载（处理 manifest.json 和 override 覆盖）。
* **Modrinth 支持 (扩展建议)**：对接 Modrinth API，支持其 `.mrpack` 格式（目前社区趋势）。

### 3.4 行业创新：AI 崩溃分析 (AI Diagnostics)
* **日志监听**：实时捕获 Minecraft 进程的 `stdout`/`stderr` 和 `crash-reports` 文件夹。
* **智能诊断**：当检测到退出代码异常时，自动提取关键堆栈信息（剔除冗余日志）。
* **AI 接口对接**：调用大语言模型（如 OpenAI、Claude 或自定义本地模型），对报错进行自然语言分析。
* **可视化输出**：用人类可读的方式告诉玩家“为什么崩溃”（如：*“内存不足”*、*“Optifine 与 Create 模组冲突”*、*“缺少前置模组 Kotlin”*），并给出解决建议（如一键禁用某模组）。

### 3.5 现代化 UI 设计 (UI/UX)
* **紧凑布局**：采用侧边栏导航 + 主内容区设计，优化信息密度，避免过度留白，适合展示大量模组和复杂的游戏版本列表。
* **视觉风格**：亚克力/毛玻璃效果（macOS Vibrant / Win11 Mica），支持全局亮/暗色模式切换。
* **状态反馈**：全局的任务/下载托盘，悬浮展示并发下载进度和网速。

---

## 4. 架构设计图 (简述)

```text
[ 前端 Vue/React ] <====== (Tauri IPC 通信) ======> [ 后端 Rust Core ]
       |                                                 |
       ├─ UI 渲染与状态管理                                 ├─ Auth 鉴权模块
       ├─ 紧凑型设置面板                                   ├─ Download 下载调度引擎 (Tokio)
       ├─ AI 对话/分析结果展示                              ├─ Process 进程管理 (启动/监听游戏)
       └─ 版本/模组包浏览视图                               ├─ FileSystem 文件/目录管理
                                                         └─ AI API 请求客户端
```

---

## 5. 项目开发路线图 (Agile & Granular Roadmap)

为了利于开发与测试，本项目采用敏捷开发模式，将总任务拆解为 11 个微小阶段（Milestones）。每一阶段完成后都可以独立运行并验证功能。

### 阶段 1：项目骨架与窗口基础 (Project Init & Window)
**开发重点**：打通 Tauri 的前后端基础通信。
* [ ] 使用 `create-tauri-app` 初始化项目（Rust + Vite + TS + Vue3/React）。
* [ ] 配置无边框窗口（Frameless）与自定义标题栏（最小化、最大化、关闭）。
* [ ] 编写第一个 Rust `command` 并由前端成功调用（如：获取系统架构和当前时间）。
* [ ] 配置全局日志系统（Rust 端的 `tracing` 或 `log` 写入到本地文件）。
* **测试目标**：应用能正常编译打开，窗口控件可用，前后端能双向打印 "Hello World" 级别的日志。

### 阶段 2：基础 UI 布局与主题系统 (UI Layout & Theme)
**开发重点**：搭建前端的架子，不涉及复杂后端逻辑。
* [ ] 引入 Tailwind CSS 与 Shadcn-UI/Radix 基础组件。
* [ ] 开发整体布局：左侧边栏（导航） + 顶部状态栏 + 主内容路由视图。
* [ ] 实现亮/暗色主题切换机制。
* [ ] 搭建几个核心空页面路由（“启动游戏”、“实例管理”、“下载面板”、“设置”）。
* **测试目标**：前端页面能流畅切换，调整系统主题时 UI 能正确响应，侧边栏路由无 BUG。

### 阶段 3：高性能下载调度引擎 (Download Engine)
**开发重点**：最核心的底层基建，必须极其稳定。
* [ ] 封装 Rust 下载器，基于 `reqwest` 和 `tokio`。
* [ ] 实现基于任务池的并发控制（例如限制最大并发 32 个连接）。
* [ ] 实现断点续传（通过检查文件大小或 Hash）。
* [ ] 监听下载进度，通过 Tauri Event `Window::emit` 高频将总进度、网速推送到前端。
* **测试目标**：编写测试用例，从远程同时下载 100 个小文件和 1 个大文件，UI 进度条平滑且文件 SHA1 校验全部通过。

### 阶段 4：账号鉴权模块 (Auth System)
**开发重点**：获取合法的启动 Token 和 UUID。
* [ ] 实现离线账号生成算法（UUID 持久化），并在前端完成“离线登录”页面。
* [ ] 接入微软登录（OAuth 2.0），实现完整的重定向流与 Token 交换。
* [ ] 实现获取玩家名称、UUID 和验证 Token 有效性的接口。
* **测试目标**：在本地 `accounts.json` 中能看到正确的微软凭证，前端能展示玩家的真实游戏 ID。

### 阶段 5：原版资产部署 (Vanilla Deployment)
**开发重点**：解析巨型 JSON，下载万级细碎文件。
* [ ] 请求并解析 `version_manifest_v2.json`，在前端列表展示所有 Minecraft 历史版本。
* [ ] 解析特定版本的 JSON，提取 `client.jar` 及其对应的 `assets`、`libraries` 列表。
* [ ] 实现系统架构规则匹配（Rule Parser），过滤掉不需要的 Native 库。
* [ ] 调起“阶段3”的下载引擎，将所有文件下载到统一的 `.minecraft` 目录结构中。
* **测试目标**：点击某个版本（如 1.20.4），等待下载完成后，比对本地文件夹结构是否与官方或第三方启动器完全一致。

### 阶段 6：游戏启动与进程管理 (Launch Engine)
**开发重点**：真正的 Milestone！能够看到游戏画面。
* [ ] 扫描本地已安装的 Java 环境，并提供手动指定路径功能。
* [ ] 解析库文件依赖并动态拼接复杂的 JVM 参数（`-cp`, `-Xmx` 等）与游戏启动参数。
* [ ] 使用 `std::process::Command` 启动 Java 进程，拦截其 `stdout` 和 `stderr` 输出到控制台。
* **测试目标**：成功启动无任何 Mod 的原版游戏，能进入主菜单并正常游玩。

### 阶段 7：实例/版本隔离管理 (Instance Management)
**开发重点**：从全局目录重构为版本隔离架构。
* [ ] 设计实例数据结构（每个实例拥有独立的 `mods`, `config`, `saves`，共享核心 `assets` 和 `libraries`）。
* [ ] 前端开发实例卡片墙：支持创建实例、修改图标、独立配置内存大小、重命名和删除。
* **测试目标**：创建两个不同的游戏实例，验证它们的存档和配置文件完全独立，互不干扰。

### 阶段 8：Mod Loaders 自动化 (Loaders Deployment)
**开发重点**：支持加载器，为模组铺平道路。
* [ ] 接入 Fabric/Quilt Meta API，解析并自动生成修改版的启动 JSON（跳过 Installer 安装）。
* [ ] 实现 Forge/NeoForge 的自动化安装（执行 `.jar` 安装器或解析其 processor）。
* **测试目标**：创建并启动一个 Fabric 实例和一个 Forge 实例，游戏内能看到对应的 Loader 版本信息。

### 阶段 9：第三方整合包支持 (Modpacks Support)
**开发重点**：对接生态，提升实用性。
* [ ] 接入 CurseForge 或 Modrinth API，支持输入 ID 获取整合包信息。
* [ ] 解析本地上传的 `.zip` / `.mrpack` 包，提取 `manifest.json`。
* [ ] 批量下载所需的 Mod，并将压缩包内的 `overrides` 目录完全覆盖至实例目录。
* **测试目标**：下载一个包含几十个 Mod 的热门整合包（如 RLcraft 或 Fabric 优化包），能一键解析、下载并成功启动。

### 阶段 10：AI 崩溃诊断 (AI Diagnostics - 核心亮点)
**开发重点**：打造差异化竞争优势。
* [ ] 编写日志监控模块：当检测到游戏进程状态码非 0（异常崩溃）时触发。
* [ ] 日志清洗：读取 `crash-reports` 并在 Rust 端过滤掉无用的堆栈，脱敏本地文件路径。
* [ ] 接入 LLM API（如 OpenAI），传入脱敏日志和预设 Prompt。
* [ ] 前端构建“崩溃诊断面板”，渲染大模型的分析结果和操作建议。
* **测试目标**：故意放入两个冲突的 Mod 导致游戏崩溃，验证启动器能否自动弹出诊断面板，并准确告知是哪两个 Mod 冲突。

### 阶段 11：产品打磨与 CI/CD 打包 (Polish & Distribution)
**开发重点**：准备交付给最终用户。
* [ ] 完善各种异常情况的 UI 提示（网络断开、文件被占用、未安装 Java）。
* [ ] 集成 Tauri Updater，实现启动器自身的热更新提示与下载。
* [ ] 配置 GitHub Actions 矩阵流。
* **测试目标**：通过云端 CI/CD 自动编译出 Windows (`.exe`)、macOS (`.dmg`) 和 Linux (`AppImage`)，在三台不同系统的裸机上安装并成功运行。
```