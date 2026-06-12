// ============== 菜单：卸载游戏 ==============

import { invoke } from "../tauri.js";
import {
  getLocalManifestExists,
  setLocalManifestVersion,
  setLocalManifestExists,
} from "../state/state.js";
import { showAlert, showConfirm } from "../modal/modal.js";
import { refreshLauncherState } from "./init.js";

export async function handleUninstall() {
  if (!getLocalManifestExists()) {
    await showAlert("本地未安装游戏", { title: "提示", type: "info" });
    return;
  }
  const ok = await showConfirm("确定要卸载游戏吗？", {
    title: "卸载游戏",
    type: "warn",
    okText: "卸载",
    cancelText: "取消",
    danger: true,
  });
  if (!ok) {
    return;
  }
  try {
    const removed = await invoke("delete_manifest");
    console.log("[Launcher] 卸载结果: removed =", removed);
    await showAlert(removed ? "已卸载游戏" : "本地未找到 game 目录", {
      title: removed ? "卸载完成" : "提示",
      type: removed ? "success" : "info",
    });

    // 卸载后刷新状态
    setLocalManifestExists(false);
    setLocalManifestVersion(null);
    refreshLauncherState();
  } catch (e) {
    console.error("[Launcher] 卸载失败:", e);
    await showAlert("卸载失败: " + e, { title: "卸载失败", type: "error" });
  }
}
