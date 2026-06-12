// ============== 主按钮状态更新 ==============
// 按钮状态逻辑（优先级从高到低）：
//   1. 本地无 manifest → "下载游戏"
//   2. 本地有 manifest + verify.json 存在但部分文件未完成 → "继续下载"
//   3. 本地有 manifest + 所有文件 ok + 服务端版本 > 本地版本 → "更新游戏"
//   4. 本地有 manifest + 所有文件 ok + 版本一致 → "开始游戏"
//   5. 本地有 manifest 但无 verify.json → "下载游戏"（文件可能不完整）

import {
  getCurrentButtonState,
  getLocalManifestExists,
  getLocalManifestVersion,
  getServerManifestVersion,
  setCurrentButtonState,
  getVerifyState,
} from "../state/state.js";

export function updateButtonState() {
  const btn = document.getElementById("btn-download");
  const textEl = btn.querySelector(".btn-text");

  if (!getLocalManifestExists()) {
    setCurrentButtonState("download");
    textEl.textContent = "下载游戏";
  } else {
    const vs = getVerifyState();
    if (vs && vs.exists) {
      // verify.json 存在：检查是否有未完成的文件
      const hasIncomplete = !vs.exe_ok || Object.values(vs.packs).some(s => s !== "ok");
      if (hasIncomplete) {
        setCurrentButtonState("continue_download");
        textEl.textContent = "继续下载";
      } else if (
        getServerManifestVersion() &&
        getLocalManifestVersion() &&
        getServerManifestVersion() !== getLocalManifestVersion()
      ) {
        setCurrentButtonState("update");
        textEl.textContent = "更新游戏";
      } else {
        setCurrentButtonState("play");
        textEl.textContent = "开始游戏";
      }
    } else {
      // 有 manifest 但没有 verify.json → 需要下载
      setCurrentButtonState("download");
      textEl.textContent = "下载游戏";
    }
  }

  console.log(
    `[Launcher] 按钮状态: ${getCurrentButtonState()} | 本地版本: ${getLocalManifestVersion()} | 服务端版本: ${getServerManifestVersion()}`
  );
}