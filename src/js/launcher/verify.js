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

  // 先读取本地 manifest 是否存在
  try {
    const local = await invoke("read_local_manifest");
    setLocalManifestExists(local.exists);
    setLocalManifestVersion(local.version);

    if (!local.exists) {
      await showAlert("本地未安装游戏，无需校验", { title: "提示", type: "info" });
      return;
    }
  } catch (e) {
    console.error("[Launcher] 读取本地 manifest 失败:", e);
    await showAlert("读取本地 manifest 失败: " + e, { title: "校验失败", type: "error" });
    return;
  }

  // 显示进度条（校验阶段）
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