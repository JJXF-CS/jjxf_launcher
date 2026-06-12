// ============== 启动器功能管理入口 ==============
// 这里是启动器所有功能模块的统一注册入口。
// 各功能模块都位于 ./js/ 下对应的子目录中：
//   - state/     : 全局状态 (state.js)
//   - progress/  : 进度条与主按钮 loading 控制
//   - menu/      : 菜单卡片控制
//   - modal/     : 自制弹窗 (替代 alert / confirm)
//   - launcher/  : 启动器主流程 (初始化 / 下载 / 校验 / 卸载)
//
// 入口只负责：
//   1) 引入各功能模块
//   2) 绑定 DOM 事件 (在 DOMContentLoaded 之后)
//   3) 启动主流程 (initLauncher)
// 任何具体实现都不放在本文件中。

import { getCurrentWindow } from "./tauri.js";

import { initLauncher } from "./launcher/init.js";
import { handlePrimaryAction } from "./launcher/primary-action.js";
import { handleVerify } from "./launcher/verify.js";
import { handleUninstall } from "./launcher/uninstall.js";

import { toggleMenu, closeMenu, isMenuOpen } from "./menu/menu.js";

import { setupProgressListeners } from "./progress/progress.js";
import { setupModalListeners } from "./modal/modal.js";

window.addEventListener("DOMContentLoaded", () => {
  const appWindow = getCurrentWindow();

  // 窗口控制
  document.querySelector("#btn-min").addEventListener("click", () => {
    appWindow.minimize();
  });
  document.querySelector("#btn-close").addEventListener("click", () => {
    appWindow.close();
  });

  // 启动时初始化：根据本地/服务端版本号决定按钮状态
  initLauncher();

  // 绑定下载/更新/开始游戏按钮（统一入口，避免重复绑定）
  document
    .getElementById("btn-download")
    .addEventListener("click", handlePrimaryAction);

  // 绑定菜单按钮
  document.getElementById("btn-menu").addEventListener("click", (e) => {
    e.stopPropagation();
    toggleMenu();
  });

  // 菜单卡片内部点击不应关闭（用 stopPropagation 阻止冒泡到 document）
  const menuCard = document.getElementById("menu-card");
  menuCard.addEventListener("click", (e) => {
    e.stopPropagation();
  });

  // 点击页面其他区域关闭菜单
  document.addEventListener("click", () => {
    if (isMenuOpen()) closeMenu();
  });

  // ESC 键关闭
  document.addEventListener("keydown", (e) => {
    if (e.key === "Escape" && isMenuOpen()) closeMenu();
  });

  // 绑定菜单项
  document.getElementById("menu-verify").addEventListener("click", () => {
    handleVerify();
    closeMenu();
  });
  document.getElementById("menu-uninstall").addEventListener("click", () => {
    handleUninstall();
    closeMenu();
  });

  // 监听后端发来的进度事件
  setupProgressListeners();

  // 绑定弹窗按钮 + 全局快捷键
  setupModalListeners();
});
