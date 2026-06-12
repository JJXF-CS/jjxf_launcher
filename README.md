# 机甲旋風启动器 (jjxf_launcher)

基于 Tauri 2 + Vanilla JS/Rust 的游戏启动器。

## Recommended IDE Setup

- [VS Code](https://code.visualstudio.com/) + [Tauri](https://marketplace.visualstudio.com/items?itemName=tauri-apps.tauri-vscode) + [rust-analyzer](https://marketplace.visualstudio.com/items?itemName=rust-lang.rust-analyzer)

## 项目结构

```
jjxf_launcher/
├── README.md
├── package.json
├── pnpm-lock.yaml
├── run/                          # 调试运行时的工作目录 (debug build)
│   └── game/
│       └── manifest.json
├── src/                          # 前端
│   ├── index.html
│   ├── _preview.html
│   ├── styles.css
│   ├── assets/
│   │   └── img/
│   │       └── bg.jpg
│   └── js/                       ← 前端按功能拆分的 JS 模块
│       ├── main.js               ← 入口 (只做模块注册 + DOM 事件绑定)
│       ├── tauri.js              ← Tauri API 集中导出
│       ├── state/
│       │   └── state.js          ← 全局状态 (manifest、按钮状态、菜单状态、进度条 phase)
│       ├── progress/
│       │   ├── progress.js       ← 进度条 + 阶段文案 + 后端事件监听
│       │   └── button-loading.js ← 主按钮 loading 状态
│       ├── menu/
│       │   └── menu.js           ← 菜单卡片 open/close/toggle
│       ├── modal/
│       │   ├── modal.js          ← 自制弹窗核心 (openModal/resolveModal/showAlert/showConfirm)
│       │   └── icons.js          ← 弹窗图标 SVG
│       └── launcher/
│           ├── init.js           ← initLauncher / refreshLauncherState
│           ├── primary-action.js ← 主按钮点击 (下载/更新/开始游戏)
│           ├── verify.js         ← 校验文件
│           ├── uninstall.js      ← 卸载游戏
│           ├── button-state.js   ← updateButtonState
│           └── menu-visibility.js← 卸载按钮显隐
└── src-tauri/                    # 后端
    ├── Cargo.toml
    ├── build.rs
    ├── tauri.conf.json
    ├── capabilities/
    │   └── default.json
    ├── icons/
    └── src/                      ← 后端按功能拆分的 Rust 模块
        ├── lib.rs                ← 入口 (模块声明 + commands 注册 + run())
        ├── main.rs               ← Tauri bin 入口
        ├── config.rs             ← 全局常量 (超时时间 / 域名 / manifest 路径 / 事件名)
        ├── paths.rs              ← 路径解析 (app_root_dir / game_dir)
        ├── events.rs             ← 进度事件 payload + emit 函数
        ├── manifest.rs           ← manifest (read_local / parse_version / save_manifest / delete_manifest)
        └── network.rs            ← 网络 (build_http_client / download_with_progress / fetch_with_fallback)
```

## 前端模块 (src/js/)

启动器入口位于 `src/js/main.js`，它只做：
1. 引入各功能模块
2. 在 `DOMContentLoaded` 之后绑定窗口控制、按钮、菜单、弹窗的 DOM 事件
3. 调用 `initLauncher()` 启动主流程

各子目录职责：

| 目录 | 职责 |
|------|------|
| `state/` | 全局状态变量 (manifest 内容、按钮状态、菜单状态、进度条 phase) |
| `progress/` | 进度条控制 + 主按钮 loading 状态 |
| `menu/` | 菜单卡片显示/隐藏 |
| `modal/` | 自制弹窗，替代 `alert` / `confirm` |
| `launcher/` | 启动器主流程：初始化、下载/更新、校验、卸载 |

## 后端模块 (src-tauri/src/)

后端入口位于 `src-tauri/src/lib.rs`，它只做：
1. 声明各功能模块 (`mod config;` 等)
2. 注册 Tauri command handlers
3. 启动 `tauri::Builder`

各模块职责：

| 模块 | 职责 |
|------|------|
| `config` | 常量 (连接/请求/分块/整体超时、主备域名、manifest 路径、最大重试、事件名) |
| `paths` | 解析 app 根目录与 `game` 目录 (debug 与 release 路径不同) |
| `events` | 进度事件 payload (`ProgressPayload` / `DonePayload` / `ErrorPayload`) 与 emit 函数 |
| `manifest` | 本地 manifest 读取、版本号解析、保存、卸载 (清空 game 目录) |
| `network` | HTTP 客户端构建、流式下载 (带进度事件)、主备域名重试拉取 |
| `lib.rs` | 入口：模块声明 + commands 注册 + Tauri Builder 启动 |
| `main.rs` | Tauri 二进制入口，调用 `jjxf_launcher_lib::run()` |
