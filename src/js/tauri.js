// ============== Tauri API 集中导出 ==============
// 在这里统一获取 Tauri 提供的全局 API 并导出，
// 其余模块从这里导入，避免每个文件都重复解构 window.__TAURI__。

const { getCurrentWindow } = window.__TAURI__.window;
const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;

export { getCurrentWindow, invoke, listen };
