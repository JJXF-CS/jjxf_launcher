// ============== 菜单项可见性 ==============

import { getLocalManifestExists } from "../state/state.js";

/// 卸载按钮仅在游戏已经安装时显示
export function updateMenuVisibility() {
  const uninstallBtn = document.getElementById("menu-uninstall");
  if (getLocalManifestExists()) {
    uninstallBtn.hidden = false;
  } else {
    uninstallBtn.hidden = true;
  }
}
