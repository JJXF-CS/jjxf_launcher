import {
  getCurrentPhase,
  setCurrentPhase,
  getProgressHideTimer,
  setProgressHideTimer,
  setLoadingActivePhase,
} from "../state/state.js";
import { setPrimaryButtonLoading } from "./button-loading.js";
import { updateButtonState } from "../launcher/button-state.js";
import { listen } from "../tauri.js";

// ============== 进度条控制 ==============

// phase -> 左侧默认文案
const PHASE_LABEL = {
  init: "正在获取版本信息…",
  verify: "正在校验文件…",
  download: "正在下载更新…",
  legacy: "正在下载…",
};

// phase -> 主按钮 loading 态显示的文字
const PHASE_BTN_LABEL = {
  init: "获取中…",
  verify: "校验中…",
  download: "下载中…",
  legacy: "下载中…",
};

function getProgressEls() {
  return {
    section: document.getElementById("progress-section"),
    left: document.getElementById("progress-label-left"),
    right: document.getElementById("progress-label-right"),
    fill: document.getElementById("progress-fill"),
  };
}

function formatBytes(n) {
  if (n < 1024) return `${n} B`;
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} KB`;
  return `${(n / 1024 / 1024).toFixed(2)} MB`;
}

/// 显示进度条并设置阶段文案
export function showProgress(phase) {
  if (getProgressHideTimer()) {
    clearTimeout(getProgressHideTimer());
    setProgressHideTimer(null);
  }
  setCurrentPhase(phase);
  const { section, left, right, fill } = getProgressEls();
  section.classList.add("visible");
  left.textContent = PHASE_LABEL[phase] || "处理中…";
  right.textContent = "0%";
  fill.style.width = "0%";
  // 同步锁定主按钮与菜单按钮
  setPrimaryButtonLoading(true, PHASE_BTN_LABEL[phase] || "处理中…");
}

/// 仅更新进度条数值（不影响显示/隐藏）
function setProgress(percent, downloaded, total, attempt) {
  const { left, right, fill } = getProgressEls();
  // 在第一次拿到 0% 时，左侧文案已经设为阶段默认；后续保持
  if (typeof percent === "number" && !Number.isNaN(percent)) {
    const clamped = Math.max(0, Math.min(100, percent));
    right.textContent = `${clamped.toFixed(0)}%`;
    fill.style.width = `${clamped}%`;
  } else {
    // 未知大小：显示已下载字节数
    right.textContent = `${formatBytes(downloaded || 0)}`;
    fill.style.width = "100%";
    fill.classList.add("indeterminate");
  }

  // 重试时给个提示
  if (attempt && attempt > 1) {
    const tag = `(第 ${attempt} 次尝试)`;
    if (!left.textContent.includes(tag)) {
      left.textContent = `${left.textContent} ${tag}`;
    }
  }
}

/// 标记成功
export function finishProgress(message) {
  const { section, left, right, fill } = getProgressEls();
  fill.classList.remove("indeterminate");
  if (message) left.textContent = message;
  right.textContent = "100%";
  fill.style.width = "100%";
  // 短暂停留再隐藏
  if (getProgressHideTimer()) clearTimeout(getProgressHideTimer());
  setProgressHideTimer(
    setTimeout(() => {
      section.classList.remove("visible");
      fill.classList.remove("indeterminate");
      setCurrentPhase(null);
      // 进度条隐藏后解除按钮 loading，并根据版本状态刷新按钮文字
      setPrimaryButtonLoading(false);
      updateButtonState();
      setLoadingActivePhase(null);
    }, 600)
  );
}

/// 隐藏进度条（不显示完成态）
export function hideProgressImmediately() {
  const { section, fill } = getProgressEls();
  if (getProgressHideTimer()) {
    clearTimeout(getProgressHideTimer());
    setProgressHideTimer(null);
  }
  section.classList.remove("visible");
  fill.classList.remove("indeterminate");
  setCurrentPhase(null);
  // 立即解除按钮 loading
  setPrimaryButtonLoading(false);
  updateButtonState();
  setLoadingActivePhase(null);
}

/// 监听后端事件
export async function setupProgressListeners() {
  try {
    await listen("manifest:progress", (event) => {
      const p = event.payload || {};
      // 仅响应当前阶段的进度，避免重试/旧任务污染
      if (getCurrentPhase() && p.phase && p.phase !== getCurrentPhase() && p.phase !== "legacy") return;
      setProgress(p.percent, p.downloaded, p.total, p.attempt);
    });
    await listen("manifest:done", (event) => {
      const p = event.payload || {};
      if (getCurrentPhase() && p.phase && p.phase !== getCurrentPhase() && p.phase !== "legacy") return;
      finishProgress("下载完成");
    });
    await listen("manifest:error", (event) => {
      const p = event.payload || {};
      if (getCurrentPhase() && p.phase && p.phase !== getCurrentPhase() && p.phase !== "legacy") return;
      // 错误事件不立刻隐藏进度条（可能马上会重试下一域名）
      console.warn("[Launcher] 进度错误事件:", p);
    });
  } catch (e) {
    console.error("[Launcher] 注册进度事件监听失败:", e);
  }
}
