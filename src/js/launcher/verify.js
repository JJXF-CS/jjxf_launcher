// ============== 菜单：校验文件 ==============
// 校验本地所有文件的 sha256 是否与 manifest.json 一致。
// 调用后端 verify_local_files：逐个文件计算 sha256 + 写 verify.json + 发事件。

import { invoke } from "../tauri.js";
import {
  setServerManifestContent,
  setServerManifestVersion,
  setLocalManifestVersion,
  setLocalManifestExists,
  setVerifyState,
} from "../state/state.js";
import { showProgress, finishProgress, hideProgressImmediately } from "../progress/progress.js";
import { showAlert } from "../modal/modal.js";
import { refreshLauncherState } from "./init.js";

export async function handleVerify() {
  console.log("[Launcher] 校验文件");

  // 1) 先从服务器拉取最新 manifest.json（确保校验用的是最新 sha256）
  showProgress("init");
  try {
    const fresh = await invoke("fetch_manifest_with_fallback", {
      phase: "verify",
    });
    setServerManifestContent(fresh);
    const version = await invoke("parse_manifest_version", {
      content: fresh,
    });
    setServerManifestVersion(version);

    // 保存到本地，供 verify_local_files 读取
    const savedPath = await invoke("save_manifest", { content: fresh });
    console.log("[Launcher] 已拉取最新 manifest.json 并保存到:", savedPath);
  } catch (e) {
    console.error("[Launcher] 拉取服务端 manifest 失败:", e);
    hideProgressImmediately();
    await showAlert("无法连接服务器获取版本信息", { title: "网络异常", type: "warn" });
    return;
  }

  // 2) 检查本地安装状态
  try {
    const local = await invoke("read_local_manifest");
    setLocalManifestExists(local.exists);
    setLocalManifestVersion(local.version);

    if (!local.exists) {
      hideProgressImmediately();
      await showAlert("本地未安装游戏，无需校验", { title: "提示", type: "info" });
      return;
    }
  } catch (e) {
    console.error("[Launcher] 读取本地 manifest 失败:", e);
    hideProgressImmediately();
    await showAlert("读取本地 manifest 失败: " + e, { title: "校验失败", type: "error" });
    return;
  }

  // 3) 开始校验（用最新的 manifest 内容）
  showProgress("verify");
  try {
    const result = await invoke("verify_local_files");
    console.log("[Launcher] verify_local_files 结果:", result);

    // 刷新 verify.json 状态
    try {
      const vs = await invoke("read_verify_state");
      setVerifyState(vs);
    } catch (e) {
      console.warn("[Launcher] 读取 verify_state 失败:", e);
    }

    if (result.ok) {
      finishProgress("校验通过");
      await showAlert(
        `校验通过\n所有文件完整且与 manifest.json 一致`,
        { title: "校验通过", type: "success" }
      );
    } else {
      finishProgress(`校验完成（${result.failed_files?.length || 0} 个文件异常）`);
      await showAlert(
        `校验完成，但有 ${result.failed_files?.length || 0} 个文件需要重新下载：\n${(result.failed_files || []).join("\n")}`,
        { title: "校验异常", type: "warn" }
      );
      refreshLauncherState();
    }
  } catch (e) {
    console.error("[Launcher] 校验失败:", e);
    hideProgressImmediately();
    await showAlert("校验失败: " + e, { title: "校验失败", type: "error" });
  }
}