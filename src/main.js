const { getCurrentWindow } = window.__TAURI__.window;
const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;

// 当前缓存的服务端 manifest 文本（点击下载/更新时用于落盘）
let serverManifestContent = null;
let serverManifestVersion = null;
let localManifestVersion = null;
let localManifestExists = false;

// 按钮状态：download / update / play
let currentButtonState = "download";

// 菜单卡片相关
let isMenuOpen = false;

// 进度条相关
let currentPhase = null; // init / verify / download / null
let progressHideTimer = null;

// phase -> 左侧默认文案
const PHASE_LABEL = {
  init: "正在获取版本信息…",
  verify: "正在校验文件…",
  download: "正在下载更新…",
  legacy: "正在下载…",
};

window.addEventListener("DOMContentLoaded", () => {
  const appWindow = getCurrentWindow();

  document.querySelector("#btn-min").addEventListener("click", () => {
    appWindow.minimize();
  });
  document.querySelector("#btn-close").addEventListener("click", () => {
    appWindow.close();
  });

  // 启动时初始化：根据本地/服务端版本号决定按钮状态
  initLauncher();

  // 绑定下载/更新/开始游戏按钮（统一入口，避免重复绑定）
  document
    .getElementById("btn-download")
    .addEventListener("click", handlePrimaryAction);

  // 绑定菜单按钮
  document.getElementById("btn-menu").addEventListener("click", (e) => {
    e.stopPropagation();
    toggleMenu();
  });

  // 菜单卡片内部点击不应关闭（用 stopPropagation 阻止冒泡到 document）
  const menuCard = document.getElementById("menu-card");
  menuCard.addEventListener("click", (e) => {
    e.stopPropagation();
  });

  // 点击页面其他区域关闭菜单
  document.addEventListener("click", () => {
    if (isMenuOpen) closeMenu();
  });

  // ESC 键关闭
  document.addEventListener("keydown", (e) => {
    if (e.key === "Escape" && isMenuOpen) closeMenu();
  });

  // 绑定菜单项
  document.getElementById("menu-verify").addEventListener("click", () => {
    handleVerify();
    closeMenu();
  });
  document.getElementById("menu-uninstall").addEventListener("click", () => {
    handleUninstall();
    closeMenu();
  });

  // 监听后端发来的进度事件
  setupProgressListeners();
});

// ============== 进度条控制 ==============
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
function showProgress(phase) {
  if (progressHideTimer) {
    clearTimeout(progressHideTimer);
    progressHideTimer = null;
  }
  currentPhase = phase;
  const { section, left, right, fill } = getProgressEls();
  section.classList.add("visible");
  left.textContent = PHASE_LABEL[phase] || "处理中…";
  right.textContent = "0%";
  fill.style.width = "0%";
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
function finishProgress(message) {
  const { section, left, right, fill } = getProgressEls();
  fill.classList.remove("indeterminate");
  if (message) left.textContent = message;
  right.textContent = "100%";
  fill.style.width = "100%";
  // 短暂停留再隐藏
  if (progressHideTimer) clearTimeout(progressHideTimer);
  progressHideTimer = setTimeout(() => {
    section.classList.remove("visible");
    fill.classList.remove("indeterminate");
    currentPhase = null;
  }, 600);
}

/// 隐藏进度条（不显示完成态）
function hideProgressImmediately() {
  const { section, fill } = getProgressEls();
  if (progressHideTimer) {
    clearTimeout(progressHideTimer);
    progressHideTimer = null;
  }
  section.classList.remove("visible");
  fill.classList.remove("indeterminate");
  currentPhase = null;
}

/// 监听后端事件
async function setupProgressListeners() {
  try {
    await listen("manifest:progress", (event) => {
      const p = event.payload || {};
      // 仅响应当前阶段的进度，避免重试/旧任务污染
      if (currentPhase && p.phase && p.phase !== currentPhase && p.phase !== "legacy") return;
      setProgress(p.percent, p.downloaded, p.total, p.attempt);
    });
    await listen("manifest:done", (event) => {
      const p = event.payload || {};
      if (currentPhase && p.phase && p.phase !== currentPhase && p.phase !== "legacy") return;
      finishProgress("下载完成");
    });
    await listen("manifest:error", (event) => {
      const p = event.payload || {};
      if (currentPhase && p.phase && p.phase !== currentPhase && p.phase !== "legacy") return;
      // 错误事件不立刻隐藏进度条（可能马上会重试下一域名）
      console.warn("[Launcher] 进度错误事件:", p);
    });
  } catch (e) {
    console.error("[Launcher] 注册进度事件监听失败:", e);
  }
}

// ============== 启动逻辑 ==============
async function initLauncher() {
  try {
    const cwd = await invoke("get_working_dir");
    console.log("[Launcher] 工作目录:", cwd);
  } catch (e) {
    console.error("[Launcher] 获取工作目录失败:", e);
  }

  // 1) 读取本地 manifest.json
  try {
    const local = await invoke("read_local_manifest");
    localManifestExists = local.exists;
    localManifestVersion = local.version;
    console.log("[Launcher] 本地 manifest:", local);
  } catch (e) {
    console.error("[Launcher] 读取本地 manifest 失败:", e);
    localManifestExists = false;
    localManifestVersion = null;
  }

  // 2) 拉取服务端 manifest.json
  //    使用后端内置的「主用 oss.jjxf.cc -> 备用 update.jjxf.cc + 最多 3 次重试」逻辑
  //    启动时也要显示进度条
  showProgress("init");
  try {
    serverManifestContent = await invoke("fetch_manifest_with_fallback", {
      phase: "init",
    });
    serverManifestVersion = await invoke("parse_manifest_version", {
      content: serverManifestContent,
    });
    console.log("[Launcher] 服务端 manifest:", serverManifestContent);
    console.log("[Launcher] 服务端版本号:", serverManifestVersion);
    finishProgress("已获取最新版本信息");
  } catch (e) {
    console.error("[Launcher] 拉取服务端 manifest 失败:", e);
    serverManifestContent = null;
    serverManifestVersion = null;
    hideProgressImmediately();
  }

  // 3) 根据本地/服务端版本号决定按钮文字 + 刷新菜单
  refreshLauncherState();
}

function updateButtonState() {
  const btn = document.getElementById("btn-download");
  const textEl = btn.querySelector(".btn-text");

  if (!localManifestExists) {
    currentButtonState = "download";
    textEl.textContent = "下载游戏";
  } else if (
    serverManifestVersion &&
    localManifestVersion &&
    serverManifestVersion === localManifestVersion
  ) {
    currentButtonState = "play";
    textEl.textContent = "开始游戏";
  } else {
    currentButtonState = "update";
    textEl.textContent = "更新游戏";
  }

  console.log(
    `[Launcher] 按钮状态: ${currentButtonState} | 本地版本: ${localManifestVersion} | 服务端版本: ${serverManifestVersion}`
  );
}

function refreshLauncherState() {
  updateButtonState();
  updateMenuVisibility();
}

/// 卸载按钮仅在游戏已经安装时显示
function updateMenuVisibility() {
  const uninstallBtn = document.getElementById("menu-uninstall");
  if (localManifestExists) {
    uninstallBtn.hidden = false;
  } else {
    uninstallBtn.hidden = true;
  }
}

// ============== 菜单卡片控制 ==============
function toggleMenu() {
  if (isMenuOpen) {
    closeMenu();
  } else {
    openMenu();
  }
}

function openMenu() {
  const card = document.getElementById("menu-card");
  const btn = document.getElementById("btn-menu");
  card.classList.add("visible");
  card.setAttribute("aria-hidden", "false");
  btn.classList.add("active");
  isMenuOpen = true;
}

function closeMenu() {
  const card = document.getElementById("menu-card");
  const btn = document.getElementById("btn-menu");
  card.classList.remove("visible");
  card.setAttribute("aria-hidden", "true");
  btn.classList.remove("active");
  isMenuOpen = false;
}

// ============== 按钮点击处理 ==============
async function handlePrimaryAction() {
  if (currentButtonState === "play") {
    console.log("[Launcher] 开始游戏 (TODO: 启动游戏逻辑)");
    alert("开始游戏功能待实现");
    return;
  }

  if (!serverManifestContent) {
    console.error("[Launcher] 没有可用的服务端 manifest，无法落盘");
    alert("尚未获取到服务端 manifest.json，请检查网络");
    return;
  }

  // 下载/更新时也走带进度的拉取，再保存
  showProgress("download");
  try {
    const fresh = await invoke("fetch_manifest_with_fallback", {
      phase: "download",
    });
    serverManifestContent = fresh;
    serverManifestVersion = await invoke("parse_manifest_version", {
      content: fresh,
    });

    const savedPath = await invoke("save_manifest", { content: fresh });
    console.log("[Launcher] manifest.json 已保存到:", savedPath);

    const local = await invoke("read_local_manifest");
    localManifestExists = local.exists;
    localManifestVersion = local.version;

    finishProgress("更新完成");
    if (
      serverManifestVersion &&
      localManifestVersion &&
      serverManifestVersion === localManifestVersion
    ) {
      alert("更新完成，现在可以开始游戏");
    } else {
      alert("manifest.json 已下载");
    }
    refreshLauncherState();
  } catch (e) {
    console.error("[Launcher] 下载/更新失败:", e);
    hideProgressImmediately();
    alert("下载/更新失败: " + e);
  }
}

// ============== 菜单：校验文件 ==============
async function handleVerify() {
  console.log("[Launcher] 校验文件");

  showProgress("verify");
  try {
    // 同样使用主备域名 + 最多 3 次重试
    const serverContent = await invoke("fetch_manifest_with_fallback", {
      phase: "verify",
    });
    const serverVer = await invoke("parse_manifest_version", { content: serverContent });

    const local = await invoke("read_local_manifest");
    if (!local.exists) {
      finishProgress("本地未安装游戏");
      alert("本地未安装游戏，无需校验");
      return;
    }

    if (serverVer && local.version && serverVer === local.version) {
      finishProgress("校验通过");
      alert(`校验通过\n本地版本: ${local.version}\n服务端版本: ${serverVer}`);
    } else {
      finishProgress("版本不一致");
      alert(
        `校验失败：版本不一致\n本地版本: ${local.version}\n服务端版本: ${serverVer}\n请重新下载或更新游戏`
      );
    }
  } catch (e) {
    console.error("[Launcher] 校验失败:", e);
    hideProgressImmediately();
    alert("校验失败: " + e);
  }
}

// ============== 菜单：卸载游戏 ==============
async function handleUninstall() {
  if (!localManifestExists) {
    alert("本地未安装游戏");
    return;
  }
  if (!confirm("确定要卸载游戏吗？")) {
    return;
  }
  try {
    const removed = await invoke("delete_manifest");
    console.log("[Launcher] 卸载结果: removed =", removed);
    alert(removed ? "已卸载游戏" : "本地未找到 game 目录");

    // 卸载后刷新状态
    localManifestExists = false;
    localManifestVersion = null;
    refreshLauncherState();
  } catch (e) {
    console.error("[Launcher] 卸载失败:", e);
    alert("卸载失败: " + e);
  }
}
