// ============== 启动逻辑 ==============

import { invoke } from "../tauri.js";
import {
  setServerManifestContent,
  setServerManifestVersion,
  setLocalManifestVersion,
  setLocalManifestExists,
} from "../state/state.js";
import { showProgress, finishProgress, hideProgressImmediately } from "../progress/progress.js";
import { updateButtonState } from "./button-state.js";
import { updateMenuVisibility } from "./menu-visibility.js";

export async function initLauncher() {
  try {
    const cwd = await invoke("get_working_dir");
    console.log("[Launcher] 工作目录:", cwd);
  } catch (e) {
    console.error("[Launcher] 获取工作目录失败:", e);
  }

  // 1) 读取本地 manifest.json
  try {
    const local = await invoke("read_local_manifest");
    setLocalManifestExists(local.exists);
    setLocalManifestVersion(local.version);
    console.log("[Launcher] 本地 manifest:", local);
  } catch (e) {
    console.error("[Launcher] 读取本地 manifest 失败:", e);
    setLocalManifestExists(false);
    setLocalManifestVersion(null);
  }

  // 2) 拉取服务端 manifest.json
  //    使用后端内置的「主用 oss.jjxf.cc -> 备用 update.jjxf.cc + 最多 3 次重试」逻辑
  //    启动时也要显示进度条
  showProgress("init");
  try {
    const content = await invoke("fetch_manifest_with_fallback", {
      phase: "init",
    });
    setServerManifestContent(content);
    const version = await invoke("parse_manifest_version", {
      content: content,
    });
    setServerManifestVersion(version);
    console.log("[Launcher] 服务端 manifest:", content);
    console.log("[Launcher] 服务端版本号:", version);
    finishProgress("已获取最新版本信息");
  } catch (e) {
    console.error("[Launcher] 拉取服务端 manifest 失败:", e);
    setServerManifestContent(null);
    setServerManifestVersion(null);
    hideProgressImmediately();
  }

  // 3) 根据本地/服务端版本号决定按钮文字 + 刷新菜单
  refreshLauncherState();
}

export function refreshLauncherState() {
  updateButtonState();
  updateMenuVisibility();
}
