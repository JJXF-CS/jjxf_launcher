// ============== 主按钮 loading 状态控制 ==============

import {
  getLastPrimaryButtonText,
  setLastPrimaryButtonText,
} from "../state/state.js";

export function setPrimaryButtonLoading(loading, label) {
  const btn = document.getElementById("btn-download");
  const textEl = btn.querySelector(".btn-text");
  const menuBtn = document.getElementById("btn-menu");
  const verifyItem = document.getElementById("menu-verify");
  const uninstallItem = document.getElementById("menu-uninstall");

  if (loading) {
    if (getLastPrimaryButtonText() === null) {
      setLastPrimaryButtonText(textEl.textContent);
    }
    textEl.textContent = label || "处理中…";
    btn.classList.add("loading");
    btn.disabled = true;
    // 菜单按钮也锁定，避免用户在下载/校验中重新触发
    if (menuBtn) menuBtn.disabled = true;
    if (verifyItem) verifyItem.classList.add("disabled");
    if (uninstallItem) uninstallItem.classList.add("disabled");
  } else {
    btn.classList.remove("loading");
    btn.disabled = false;
    if (menuBtn) menuBtn.disabled = false;
    if (verifyItem) verifyItem.classList.remove("disabled");
    if (uninstallItem) uninstallItem.classList.remove("disabled");
    // 不在这里恢复文案，恢复由 updateButtonState 决定
    setLastPrimaryButtonText(null);
  }
}
