// ============== 主按钮状态更新 ==============

import {
  getCurrentButtonState,
  getLocalManifestExists,
  getLocalManifestVersion,
  getServerManifestVersion,
  setCurrentButtonState,
} from "../state/state.js";

export function updateButtonState() {
  const btn = document.getElementById("btn-download");
  const textEl = btn.querySelector(".btn-text");

  if (!getLocalManifestExists()) {
    setCurrentButtonState("download");
    textEl.textContent = "下载游戏";
  } else if (
    getServerManifestVersion() &&
    getLocalManifestVersion() &&
    getServerManifestVersion() === getLocalManifestVersion()
  ) {
    setCurrentButtonState("play");
    textEl.textContent = "开始游戏";
  } else {
    setCurrentButtonState("update");
    textEl.textContent = "更新游戏";
  }

  console.log(
    `[Launcher] 按钮状态: ${getCurrentButtonState()} | 本地版本: ${getLocalManifestVersion()} | 服务端版本: ${getServerManifestVersion()}`
  );
}
