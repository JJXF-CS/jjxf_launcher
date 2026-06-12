// ============== 按钮点击处理 (下载/更新/开始游戏) ==============

import { invoke } from "../tauri.js";
import {
  getCurrentButtonState,
  getServerManifestContent,
  getServerManifestVersion,
  getLocalManifestVersion,
  setServerManifestContent,
  setServerManifestVersion,
  setLocalManifestVersion,
  setLocalManifestExists,
} from "../state/state.js";
import { showProgress, finishProgress, hideProgressImmediately } from "../progress/progress.js";
import { showAlert } from "../modal/modal.js";
import { refreshLauncherState } from "./init.js";

export async function handlePrimaryAction() {
  if (getCurrentButtonState() === "play") {
    console.log("[Launcher] 开始游戏 (TODO: 启动游戏逻辑)");
    await showAlert("开始游戏功能待实现", { title: "提示", type: "info" });
    return;
  }

  if (!getServerManifestContent()) {
    console.error("[Launcher] 没有可用的服务端 manifest，无法落盘");
    await showAlert("尚未获取到服务端 manifest.json，请检查网络", {
      title: "网络异常",
      type: "warn",
    });
    return;
  }

  // 下载/更新时也走带进度的拉取，再保存
  showProgress("download");
  try {
    const fresh = await invoke("fetch_manifest_with_fallback", {
      phase: "download",
    });
    setServerManifestContent(fresh);
    const version = await invoke("parse_manifest_version", {
      content: fresh,
    });
    setServerManifestVersion(version);

    const savedPath = await invoke("save_manifest", { content: fresh });
    console.log("[Launcher] manifest.json 已保存到:", savedPath);

    const local = await invoke("read_local_manifest");
    setLocalManifestExists(local.exists);
    setLocalManifestVersion(local.version);

    finishProgress("更新完成");
    if (
      getServerManifestVersion() &&
      getLocalManifestVersion() &&
      getServerManifestVersion() === getLocalManifestVersion()
    ) {
      await showAlert("更新完成，现在可以开始游戏", {
        title: "更新完成",
        type: "success",
      });
    } else {
      await showAlert("manifest.json 已下载", { title: "提示", type: "info" });
    }
    refreshLauncherState();
  } catch (e) {
    console.error("[Launcher] 下载/更新失败:", e);
    hideProgressImmediately();
    await showAlert("下载/更新失败: " + e, { title: "下载失败", type: "error" });
  }
}
