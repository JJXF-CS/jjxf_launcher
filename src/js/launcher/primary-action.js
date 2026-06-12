// ============== 按钮点击处理 (下载/更新/开始游戏/结束游戏) ==============

import { invoke } from "../tauri.js";
import {
  getCurrentButtonState,
  getServerManifestContent,
  getServerManifestVersion,
  getLocalManifestVersion,
  setServerManifestContent,
  setServerManifestVersion,
  setLocalManifestVersion,
  setLocalManifestExists,
  setVerifyState,
  setCurrentButtonState,
} from "../state/state.js";
import { showProgress, finishProgress, hideProgressImmediately } from "../progress/progress.js";
import { showAlert } from "../modal/modal.js";
import { refreshLauncherState } from "./init.js";

// 进程监控定时器
let processWatchInterval = null;
let wasRunning = false;

export async function handlePrimaryAction() {
  const state = getCurrentButtonState();

  // ======== 开始游戏 ========
  if (state === "play") {
    await launchGameAndWatch();
    return;
  }

  // ======== 结束游戏 ========
  if (state === "stop") {
    await stopGame();
    return;
  }

  // ======== 下载/继续下载/更新：共用同一套流程 ========
  // 步骤：
  //   1) 从服务器拉取最新 manifest.json 并保存到本地
  //   2) 调用 start_download：按 manifest.json 里的文件列表逐个下载 + 校验
  //   3) 刷新 verify.json 状态
  //   4) 刷新按钮文案

  if (!getServerManifestContent()) {
    console.error("[Launcher] 没有可用的服务端 manifest，无法落盘");
    await showAlert("尚未获取到服务端 manifest.json，请检查网络", {
      title: "网络异常",
      type: "warn",
    });
    return;
  }

  try {
    // 1) 先显示“获取版本信息”进度条，拉取最新 manifest 并保存
    showProgress("init");
    const fresh = await invoke("fetch_manifest_with_fallback", {
      phase: "download",
    });
    setServerManifestContent(fresh);
    const version = await invoke("parse_manifest_version", {
      content: fresh,
    });
    setServerManifestVersion(version);
    const savedPath = await invoke("save_manifest", { content: fresh });
    console.log("[Launcher] manifest.json 已保存到:", savedPath);

    const local = await invoke("read_local_manifest");
    setLocalManifestExists(local.exists);
    setLocalManifestVersion(local.version);

    // 2) manifest 拉取完成后切换到“下载主程序”阶段
    showProgress("exe");
    const result = await invoke("start_download");

    console.log("[Launcher] start_download 结果:", result);

    // 3) 刷新 verify.json 状态
    try {
      const vs = await invoke("read_verify_state");
      setVerifyState(vs);
    } catch (e) {
      console.warn("[Launcher] 读取 verify_state 失败:", e);
    }

    // 4) 刷新按钮
    if (result.ok) {
      finishProgress("下载完成");
      await showAlert("下载完成，现在可以开始游戏", {
        title: "下载完成",
        type: "success",
      });
    } else {
      finishProgress(`下载完成（${result.failed_files?.length || 0} 个文件失败）`);
      await showAlert(
        `下载完成，但有 ${result.failed_files?.length || 0} 个文件失败：\n${(result.failed_files || []).join("\n")}`,
        { title: "部分下载失败", type: "warn" }
      );
    }
    refreshLauncherState();
  } catch (e) {
    console.error("[Launcher] 下载/更新失败:", e);
    hideProgressImmediately();
    await showAlert("下载/更新失败: " + e, { title: "下载失败", type: "error" });
  }
}

// ============== 游戏启动 + 进程监控 ==============

async function launchGameAndWatch() {
  const btn = document.getElementById("btn-download");
  const textEl = btn.querySelector(".btn-text");

  // 按钮变灰显示"启动中…"
  btn.disabled = true;
  textEl.textContent = "启动中…";
  setCurrentButtonState("launching");

  try {
    await invoke("launch_game");
  } catch (e) {
    console.error("[Launcher] 启动游戏失败:", e);
    btn.disabled = false;
    setCurrentButtonState("play");
    textEl.textContent = "开始游戏";
    await showAlert("启动游戏失败: " + e, { title: "启动失败", type: "error" });
    return;
  }

  // 轮询检查进程状态（每秒一次，最多30秒）
  let elapsed = 0;
  let started = false;

  while (elapsed < 30) {
    await sleep(1000);
    elapsed++;

    try {
      const status = await invoke("check_game_running");
      if (status.running) {
        started = true;
        break;
      }
    } catch (e) {
      console.warn("[Launcher] 检查进程状态失败:", e);
    }
  }

  if (started) {
    // 启动成功：最小化启动器，按钮变为"结束运行"
    textEl.textContent = "结束运行";
    btn.disabled = false;
    setCurrentButtonState("stop");

    try {
      const { getCurrentWindow } = await import("../tauri.js");
      const win = getCurrentWindow();
      await win.minimize();
    } catch (e) {
      console.warn("[Launcher] 最小化窗口失败:", e);
    }

    // 启动持续监控
    startProcessWatch();
  } else {
    // 30秒超时：恢复按钮
    btn.disabled = false;
    setCurrentButtonState("play");
    textEl.textContent = "开始游戏";
    await showAlert("游戏启动超时（30秒），请检查 game.exe 是否正常", {
      title: "启动超时",
      type: "warn",
    });
  }
}

async function stopGame() {
  try {
    await invoke("kill_game");
  } catch (e) {
    console.error("[Launcher] 结束游戏失败:", e);
  }
  stopProcessWatch();

  const btn = document.getElementById("btn-download");
  const textEl = btn.querySelector(".btn-text");
  textEl.textContent = "开始游戏";
  setCurrentButtonState("play");
}

function startProcessWatch() {
  stopProcessWatch();
  wasRunning = true;

  processWatchInterval = setInterval(async () => {
    try {
      const status = await invoke("check_game_running");
      if (status.running && !wasRunning) {
        // 进程从无到有：最小化启动器，按钮变为结束
        wasRunning = true;
        setCurrentButtonState("stop");
        document.getElementById("btn-download").querySelector(".btn-text").textContent = "结束运行";

        const { getCurrentWindow } = await import("../tauri.js");
        try { await getCurrentWindow().minimize(); } catch (_) {}
      }

      if (!status.running && wasRunning) {
        // 进程从有到无：还原窗口，按钮变为开始
        wasRunning = false;
        setCurrentButtonState("play");
        document.getElementById("btn-download").querySelector(".btn-text").textContent = "开始游戏";

        const { getCurrentWindow } = await import("../tauri.js");
        const win = getCurrentWindow();
        try { await win.setFocus(); } catch (_) {}
        try { await win.show(); } catch (_) {}
      }
    } catch (e) {
      // 静默忽略
    }
  }, 10000);
}

function stopProcessWatch() {
  if (processWatchInterval) {
    clearInterval(processWatchInterval);
    processWatchInterval = null;
  }
}

function sleep(ms) {
  return new Promise(resolve => setTimeout(resolve, ms));
}
