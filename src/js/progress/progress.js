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

/// 下载速度追踪器（跨事件计算速度）
const downloadSpeedTracker = {};

// phase -> 左侧默认文案
const PHASE_LABEL = {
  init: "正在获取版本信息…",
  verify: "正在校验文件…",
  download: "正在下载更新…",
  exe: "正在下载主程序…",
  packs: "正在下载资源包…",
  legacy: "正在下载…",
};

// phase -> 主按钮 loading 态显示的文字
const PHASE_BTN_LABEL = {
  init: "获取中…",
  verify: "校验中…",
  download: "下载中…",
  exe: "下载中…",
  packs: "下载中…",
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
  setPrimaryButtonLoading(true, PHASE_BTN_LABEL[phase] || "处理中…");
}

/// 仅更新进度条数值
function setProgress(percent, downloaded, total, attempt, leftOverride, speed) {
  const { left, right, fill } = getProgressEls();
  if (leftOverride) {
    left.textContent = leftOverride;
  }
  if (typeof percent === "number" && !Number.isNaN(percent)) {
    const clamped = Math.max(0, Math.min(100, percent));
    let rightText = `${clamped.toFixed(0)}%`;
    if (total && total > 0 && downloaded != null) {
      rightText += `    ${formatBytes(downloaded)} / ${formatBytes(total)}`;
    } else if (downloaded != null && downloaded > 0) {
      rightText += `    ${formatBytes(downloaded)}`;
    }
    // 显示实时速度
    if (speed && speed > 0) {
      rightText += `    ${formatBytes(speed)}/s`;
    }
    right.textContent = rightText;
    fill.style.width = `${clamped}%`;
  } else {
    right.textContent = `${formatBytes(downloaded || 0)}`;
    fill.style.width = "100%";
    fill.classList.add("indeterminate");
  }

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
  if (getProgressHideTimer()) clearTimeout(getProgressHideTimer());
  setProgressHideTimer(
    setTimeout(() => {
      section.classList.remove("visible");
      fill.classList.remove("indeterminate");
      setCurrentPhase(null);
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
  setPrimaryButtonLoading(false);
  updateButtonState();
  setLoadingActivePhase(null);
}

/// 监听后端事件（旧 manifest 拉取 + 新下载流程）
export async function setupProgressListeners() {
  try {
    // ======== 旧 manifest 拉取事件（init / legacy） ========
    await listen("manifest:progress", (event) => {
      const p = event.payload || {};
      if (getCurrentPhase() && p.phase && p.phase !== getCurrentPhase() && p.phase !== "legacy") return;
      setProgress(p.percent, p.downloaded, p.total, p.attempt);
    });
    await listen("manifest:done", (event) => {
      const p = event.payload || {};
      // 如果当前没有活跃的进度阶段（currentPhase 为 null），忽略此事件
      if (!getCurrentPhase()) return;
      if (p.phase && p.phase !== getCurrentPhase() && p.phase !== "legacy") return;
      // init 阶段拉取 manifest 完成时不要关闭进度条——后面还要切到 exe 阶段
      if (getCurrentPhase() === "init") {
        console.log("[Launcher] init 阶段 manifest:done 收到，保留进度条以备切到 exe 阶段");
        return;
      }
      finishProgress("下载完成");
    });

    await listen("manifest:error", (event) => {
      const p = event.payload || {};
      if (!getCurrentPhase()) return;
      if (p.phase && p.phase !== getCurrentPhase() && p.phase !== "legacy") return;
      console.warn("[Launcher] 进度错误事件:", p);
    });

    // ======== 新下载流程事件（exe / packs / verify） ========
    await listen("download:progress", (event) => {
      const p = event.payload || {};
      const stage = p.stage;
      let leftText = "";
      if (stage === "exe") {
        leftText = "正在下载主程序…";
      } else if (stage === "packs") {
        const currentFile = p.current_file || "";
        if (currentFile) {
          leftText = `正在下载 ${currentFile}…`;
        } else {
          leftText = `正在下载资源包…`;
        }
      } else if (stage === "verify") {
        leftText = "正在校验文件…";
      }
      // 使用 total_downloaded / total_bytes 显示跨文件累计进度
      // 百分比基于实际字节计算（而不是加权公式），进度条实时平滑
      const totalDL = p.total_downloaded || 0;
      const totalBytes = p.total_bytes || 0;
      const bytePercent = totalBytes > 0 ? (totalDL / totalBytes * 100) : p.overall_percent;

      // 计算下载速度：利用连续事件的 total_downloaded 增量 / 时间差
      const now = performance.now();
      const pumperKey = stage + "__speed";
      if (!downloadSpeedTracker[pumperKey]) {
        downloadSpeedTracker[pumperKey] = { lastBytes: totalDL, lastTime: now, speed: null };
      } else {
        const t = downloadSpeedTracker[pumperKey];
        const dt = (now - t.lastTime) / 1000; // 秒
        if (dt > 0.2 && totalDL > t.lastBytes) {
          t.speed = (totalDL - t.lastBytes) / dt;
          t.lastBytes = totalDL;
          t.lastTime = now;
        } else if (totalDL < t.lastBytes) {
          // 发生在重试回退或阶段切换，重置
          t.lastBytes = totalDL;
          t.lastTime = now;
          t.speed = null;
        }
      }
      const speed = downloadSpeedTracker[pumperKey]?.speed || null;

      setProgress(bytePercent, totalDL, totalBytes || null, p.attempt, leftText, speed);
    });

    // 单文件下载完成事件（仅打印日志，不隐藏进度条）
    await listen("download:file_done", (event) => {
      const p = event.payload || {};
      console.log("[Launcher] 文件下载完成:", p.file, "ok=" + p.ok, "size=" + p.size);
    });

    // 整个下载流程完成事件（所有文件都处理完了）
    await listen("download:done", (event) => {
      const p = event.payload || {};
      if (p.ok) {
        finishProgress("下载完成");
      } else {
        finishProgress(`下载完成（${p.message || "部分失败"}）`);
      }
    });

    await listen("download:error", (event) => {
      const p = event.payload || {};
      console.warn("[Launcher] 下载错误:", p.stage, p.file, p.message);
    });
  } catch (e) {
    console.error("[Launcher] 注册进度事件监听失败:", e);
  }
}