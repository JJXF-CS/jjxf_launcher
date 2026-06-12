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
  setVerifyState,
} from "../state/state.js";
import { showProgress, finishProgress, hideProgressImmediately } from "../progress/progress.js";
import { showAlert } from "../modal/modal.js";
import { refreshLauncherState } from "./init.js";

export async function handlePrimaryAction() {
  const state = getCurrentButtonState();

  // ======== 开始游戏 ========
  if (state === "play") {
    console.log("[Launcher] 开始游戏 (TODO: 启动游戏逻辑)");
    await showAlert("开始游戏功能待实现", { title: "提示", type: "info" });
    return;
  }

  // ======== 下载/继续下载/更新：共用同一套流程 ========
  // 步骤：
  //   1) 从服务器拉取最新 manifest.json 并保存到本地
  //   2) 调用 start_download：按 manifest.json 里的文件列表逐个下载 + 校验
  //   3) 刷新 verify.json 状态
  //   4) 刷新按钮文案

  if (!getServerManifestContent()) {
    console.error("[Launcher] 没有可用的服务端 manifest，无法落盘");
    await showAlert("尚未获取到服务端 manifest.json，请检查网络", {
      title: "网络异常",
      type: "warn",
    });
    return;
  }

  try {
    // 1) 先显示“获取版本信息”进度条，拉取最新 manifest 并保存
    showProgress("init");
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

    // 2) manifest 拉取完成后切换到“下载主程序”阶段
    showProgress("exe");
    const result = await invoke("start_download");

    console.log("[Launcher] start_download 结果:", result);

    // 3) 刷新 verify.json 状态
    try {
      const vs = await invoke("read_verify_state");
      setVerifyState(vs);
    } catch (e) {
      console.warn("[Launcher] 读取 verify_state 失败:", e);
    }

    // 4) 刷新按钮
    if (result.ok) {
      finishProgress("下载完成");
      await showAlert("下载完成，现在可以开始游戏", {
        title: "下载完成",
        type: "success",
      });
    } else {
      finishProgress(`下载完成（${result.failed_files?.length || 0} 个文件失败）`);
      await showAlert(
        `下载完成，但有 ${result.failed_files?.length || 0} 个文件失败：\n${(result.failed_files || []).join("\n")}`,
        { title: "部分下载失败", type: "warn" }
      );
    }
    refreshLauncherState();
  } catch (e) {
    console.error("[Launcher] 下载/更新失败:", e);
    hideProgressImmediately();
    await showAlert("下载/更新失败: " + e, { title: "下载失败", type: "error" });
  }
}