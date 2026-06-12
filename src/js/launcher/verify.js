// ============== 菜单：校验文件 ==============

import { invoke } from "../tauri.js";
import {
  setServerManifestContent,
  setServerManifestVersion,
  setLocalManifestVersion,
  setLocalManifestExists,
} from "../state/state.js";
import { showProgress, finishProgress, hideProgressImmediately } from "../progress/progress.js";
import { showAlert } from "../modal/modal.js";
import { refreshLauncherState } from "./init.js";

export async function handleVerify() {
  console.log("[Launcher] 校验文件");

  showProgress("verify");
  try {
    // 同样使用主备域名 + 最多 3 次重试
    const serverContent = await invoke("fetch_manifest_with_fallback", {
      phase: "verify",
    });
    const serverVer = await invoke("parse_manifest_version", { content: serverContent });

    // 同步模块级状态，后续 updateButtonState() 才能拿到最新值
    setServerManifestContent(serverContent);
    setServerManifestVersion(serverVer);

    const local = await invoke("read_local_manifest");
    // 同步本地 manifest 状态（万一期间被改动）
    setLocalManifestExists(local.exists);
    setLocalManifestVersion(local.version);

    if (!local.exists) {
      finishProgress("本地未安装游戏");
      await showAlert("本地未安装游戏，无需校验", { title: "提示", type: "info" });
      return;
    }

    if (serverVer && local.version && serverVer === local.version) {
      finishProgress("校验通过");
      await showAlert(
        `校验通过\n本地版本: ${local.version}\n服务端版本: ${serverVer}`,
        { title: "校验通过", type: "success" }
      );
    } else {
      finishProgress("版本不一致");
      await showAlert(
        `校验失败：版本不一致\n本地版本: ${local.version}\n服务端版本: ${serverVer}\n请重新下载或更新游戏`,
        { title: "校验失败", type: "error" }
      );
      // 版本不一致：强制刷新按钮状态，把"开始游戏"切换为"更新游戏"
      refreshLauncherState();
    }
  } catch (e) {
    console.error("[Launcher] 校验失败:", e);
    hideProgressImmediately();
    await showAlert("校验失败: " + e, { title: "校验失败", type: "error" });
  }
}
