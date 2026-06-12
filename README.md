# Tauri + Vanilla

This template should help get you started developing with Tauri in vanilla HTML, CSS and Javascript.

## Recommended IDE Setup

- [VS Code](https://code.visualstudio.com/) + [Tauri](https://marketplace.visualstudio.com/items?itemName=tauri-apps.tauri-vscode) + [rust-analyzer](https://marketplace.visualstudio.com/items?itemName=rust-lang.rust-analyzer)

```
src/
├── index.html
├── _preview.html
├── styles.css
├── assets/
└── js/
    ├── main.js                  ← 入口 (只做模块注册 + DOM 事件绑定)
    ├── tauri.js                 ← Tauri API 集中导出
    ├── state/
    │   └── state.js             ← 全局状态 (manifest、按钮状态、菜单状态、进度条 phase)
    ├── progress/
    │   ├── progress.js          ← 进度条 + 阶段文案 + 后端事件监听
    │   └── button-loading.js    ← 主按钮 loading 状态
    ├── menu/
    │   └── menu.js              ← 菜单卡片 open/close/toggle
    ├── modal/
    │   ├── modal.js             ← 自制弹窗核心 (openModal/resolveModal/showAlert/showConfirm)
    │   └── icons.js             ← 弹窗图标 SVG
    └── launcher/
        ├── init.js              ← initLauncher / refreshLauncherState
        ├── primary-action.js    ← 主按钮点击 (下载/更新/开始游戏)
        ├── verify.js            ← 校验文件
        ├── uninstall.js         ← 卸载游戏
        ├── button-state.js      ← updateButtonState
        └── menu-visibility.js   ← 卸载按钮显隐

```