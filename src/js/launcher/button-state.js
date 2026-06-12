// ============== 主按钮状态更新 ==============
// 按钮状态逻辑（优先级从高到低）：
//   1. 本地无 manifest → "下载游戏"
//   2. 本地有 manifest + verify.json 存在但部分文件未完成 → "继续下载"
//   3. 本地有 manifest + 所有文件 ok + 调用后端 check_update_needed 检查 sha256/path 有变更 → "更新游戏"
//   4. 本地有 manifest + 所有文件 ok + sha256/path 均一致 → "开始游戏"
//   5. 本地有 manifest 但无 verify.json → "下载游戏"（文件可能不完整）

import { invoke } from "../tauri.js";
import {
  getCurrentButtonState,
  getLocalManifestExists,
  getServerManifestContent,
  setCurrentButtonState,
  getVerifyState,
  setNeedsUpdate,
  setOutdatedFiles,
} from "../state/state.js";

export async function updateButtonState() {
  const btn = document.getElementById("btn-download");
  const textEl = btn.querySelector(".btn-text");

  if (!getLocalManifestExists()) {
    setCurrentButtonState("download");
    textEl.textContent = "下载游戏";
    return;
  }

  const vs = getVerifyState();
  const serverContent = getServerManifestContent();

  if (vs && vs.exists) {
    // verify.json 存在：检查是否有未完成的文件
    const hasIncomplete = !vs.exe_ok || Object.values(vs.packs).some(s => s !== "ok");
    if (hasIncomplete) {
      setCurrentButtonState("continue_download");
      textEl.textContent = "继续下载";
      return;
    }

    // 所有文件都 ok，但还需要检查服务端是否有 sha256 更新
    // （即使 version 相同，sha256 也可能不同——小版本热更新）
    if (serverContent) {
      try {
        const check = await invoke("check_update_needed", {
          serverManifestContent: serverContent,
        });
        console.log("[Launcher] check_update_needed:", check);

        if (check.needs_update) {
          setNeedsUpdate(true);
          setOutdatedFiles(check.outdated_files);
          setCurrentButtonState("update");
          textEl.textContent = "更新游戏";
          return;
        }
        setNeedsUpdate(false);
        setOutdatedFiles([]);
      } catch (e) {
        console.error("[Launcher] check_update_needed 调用失败:", e);
        // 回退到纯版本比较
        if (getLocalManifestExists()) {
          const localVersion = vs.manifest_version;
          const serverVersion = check?.server_version;
          if (serverVersion && localVersion && serverVersion !== localVersion) {
            setCurrentButtonState("update");
            textEl.textContent = "更新游戏";
            return;
          }
        }
      }
    }

    setCurrentButtonState("play");
    textEl.textContent = "开始游戏";
  } else {
    // 有 manifest 但没有 verify.json → 需要下载
    setCurrentButtonState("download");
    textEl.textContent = "下载游戏";
  }

  console.log(
    `[Launcher] 按钮状态: ${getCurrentButtonState()}`
  );
}
