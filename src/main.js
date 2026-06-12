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

// phase -> 主按钮 loading 态显示的文字
const PHASE_BTN_LABEL = {
  init: "获取中…",
  verify: "校验中…",
  download: "下载中…",
  legacy: "下载中…",
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

  // 绑定弹窗按钮 + 全局快捷键
  setupModalListeners();
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

// 记住主按钮进入 loading 之前的原始文案，恢复时使用
let lastPrimaryButtonText = null;

function setPrimaryButtonLoading(loading, label) {
  const btn = document.getElementById("btn-download");
  const textEl = btn.querySelector(".btn-text");
  const menuBtn = document.getElementById("btn-menu");
  const verifyItem = document.getElementById("menu-verify");
  const uninstallItem = document.getElementById("menu-uninstall");

  if (loading) {
    if (lastPrimaryButtonText === null) {
      lastPrimaryButtonText = textEl.textContent;
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
    lastPrimaryButtonText = null;
  }
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

// 记录上一次进度条上的 phase，用于 finishProgress / hideProgressImmediately 后恢复按钮文案
let _loadingActivePhase = null;

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
    // 进度条隐藏后解除按钮 loading，并根据版本状态刷新按钮文字
    setPrimaryButtonLoading(false);
    updateButtonState();
    _loadingActivePhase = null;
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
  // 立即解除按钮 loading
  setPrimaryButtonLoading(false);
  updateButtonState();
  _loadingActivePhase = null;
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
    await showAlert("开始游戏功能待实现", { title: "提示", type: "info" });
    return;
  }

  if (!serverManifestContent) {
    console.error("[Launcher] 没有可用的服务端 manifest，无法落盘");
    await showAlert("尚未获取到服务端 manifest.json，请检查网络", {
      title: "网络异常",
      type: "warn",
    });
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
      await showAlert("更新完成，现在可以开始游戏", {
        title: "更新完成",
        type: "success",
      });
    } else {
      await showAlert("manifest.json 已下载", { title: "提示", type: "info" });
    }
    refreshLauncherState();
  } catch (e) {
    console.error("[Launcher] 下载/更新失败:", e);
    hideProgressImmediately();
    await showAlert("下载/更新失败: " + e, { title: "下载失败", type: "error" });
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
    }
  } catch (e) {
    console.error("[Launcher] 校验失败:", e);
    hideProgressImmediately();
    await showAlert("校验失败: " + e, { title: "校验失败", type: "error" });
  }
}

// ============== 菜单：卸载游戏 ==============
async function handleUninstall() {
  if (!localManifestExists) {
    await showAlert("本地未安装游戏", { title: "提示", type: "info" });
    return;
  }
  const ok = await showConfirm("确定要卸载游戏吗？", {
    title: "卸载游戏",
    type: "warn",
    okText: "卸载",
    cancelText: "取消",
    danger: true,
  });
  if (!ok) {
    return;
  }
  try {
    const removed = await invoke("delete_manifest");
    console.log("[Launcher] 卸载结果: removed =", removed);
    await showAlert(removed ? "已卸载游戏" : "本地未找到 game 目录", {
      title: removed ? "卸载完成" : "提示",
      type: removed ? "success" : "info",
    });

    // 卸载后刷新状态
    localManifestExists = false;
    localManifestVersion = null;
    refreshLauncherState();
  } catch (e) {
    console.error("[Launcher] 卸载失败:", e);
    await showAlert("卸载失败: " + e, { title: "卸载失败", type: "error" });
  }
}

// ============== 自制弹窗 (替代 alert / confirm) ==============
//
// 提供两类 API：
//   showAlert(message, options)               -> Promise<void>      (替代 alert)
//   showConfirm(message, options)             -> Promise<boolean>   (替代 confirm)
//
// options 字段：
//   title        弹窗标题，默认 "提示"
//   type         info / success / warn / error  (决定图标颜色)
//   okText       “确定”按钮文案
//   cancelText   “取消”按钮文案
//   danger       true 时把主按钮样式改为红色（适用于危险操作）

let modalState = null; // 记录当前打开的弹窗状态，用于 ESC 关闭、点击外部关闭等

function setupModalListeners() {
  const okBtn = document.getElementById("modal-btn-ok");
  const cancelBtn = document.getElementById("modal-btn-cancel");
  const overlay = document.getElementById("modal-overlay");

  okBtn.addEventListener("click", () => resolveModal(true));
  cancelBtn.addEventListener("click", () => resolveModal(false));

  // 点击遮罩外部 = 取消（confirm 场景下默认不关闭，alert 场景下视为确定）
  overlay.addEventListener("click", (e) => {
    if (e.target === overlay && modalState) {
      // 默认行为：confirm -> false; alert -> true
      resolveModal(modalState.kind === "alert");
    }
  });

  // ESC 键关闭
  document.addEventListener("keydown", (e) => {
    if (e.key === "Escape" && modalState) {
      // confirm 视为取消；alert 视为确定
      resolveModal(modalState.kind === "alert");
    }
  });
}

function resolveModal(value) {
  if (!modalState) return;
  const { resolver, kind } = modalState;
  modalState = null;

  const overlay = document.getElementById("modal-overlay");
  overlay.classList.remove("visible");
  overlay.setAttribute("aria-hidden", "true");

  //  alert 类型不管 value 是 true/false 都 resolve
  if (kind === "alert") {
    resolver(true);
  } else {
    resolver(Boolean(value));
  }
}

function openModal({ kind, title, message, okText, cancelText, danger, type }) {
  // kind  : "alert" | "confirm"  —— 决定是否显示取消按钮
  // type  : "info" | "success" | "warn" | "error"  —— 决定图标颜色
  const overlay = document.getElementById("modal-overlay");
  const titleEl = document.getElementById("modal-title");
  const messageEl = document.getElementById("modal-message");
  const iconEl = document.getElementById("modal-icon");
  const okBtn = document.getElementById("modal-btn-ok");
  const cancelBtn = document.getElementById("modal-btn-cancel");

  titleEl.textContent = title || "提示";
  messageEl.textContent = message ?? "";

  // 替换图标 (用 type, 不是 kind)
  const iconKind = type || "info";
  iconEl.className = "modal-icon " + iconKind;
  iconEl.innerHTML = getModalIconSvg(iconKind);

  // 按钮
  okBtn.textContent = okText || "确定";
  cancelBtn.textContent = cancelText || "取消";

  // 重置按钮样式
  okBtn.classList.remove("modal-btn-primary", "modal-btn-danger");
  cancelBtn.classList.remove("modal-btn-default");
  cancelBtn.classList.add("modal-btn-default");
  okBtn.classList.add(danger ? "modal-btn-danger" : "modal-btn-primary");

  // confirm 才有取消按钮
  cancelBtn.hidden = kind !== "confirm";

  overlay.setAttribute("aria-hidden", "false");
  // 下一帧再加 visible，触发过渡动画
  requestAnimationFrame(() => overlay.classList.add("visible"));

  // 默认焦点在主按钮上
  setTimeout(() => okBtn.focus(), 0);

  return new Promise((resolve) => {
    modalState = { resolver: resolve, kind };
  });
}

function getModalIconSvg(type) {
  // 使用内联 SVG，避免依赖外部资源
  const common =
    'viewBox="0 0 24 24" width="22" height="22" stroke="currentColor" stroke-width="2" fill="none" stroke-linecap="round" stroke-linejoin="round"';
  switch (type) {
    case "success":
      return `<svg ${common}><path d="M20 6L9 17l-5-5"></path></svg>`;
    case "warn":
      return `<svg ${common}><path d="M10.29 3.86L1.82 18a2 2 0 0 0 1.71 3h16.94a2 2 0 0 0 1.71-3L13.71 3.86a2 2 0 0 0-3.42 0z"></path><line x1="12" y1="9" x2="12" y2="13"></line><line x1="12" y1="17" x2="12.01" y2="17"></line></svg>`;
    case "error":
      return `<svg ${common}><circle cx="12" cy="12" r="10"></circle><line x1="15" y1="9" x2="9" y2="15"></line><line x1="9" y1="9" x2="15" y2="15"></line></svg>`;
    case "info":
    case "alert":
    case "confirm":
    default:
      return `<svg ${common}><circle cx="12" cy="12" r="10"></circle><line x1="12" y1="8" x2="12" y2="12"></line><line x1="12" y1="16" x2="12.01" y2="16"></line></svg>`;
  }
}

/**
 * 替代原生 alert()。返回一个 Promise，弹窗关闭后 resolve。
 * @param {string} message
 * @param {{ title?: string, type?: 'info'|'success'|'warn'|'error', okText?: string }} [options]
 */
function showAlert(message, options = {}) {
  // kind 决定弹窗类别（alert/confirm），type 决定图标颜色，两者不能冲突
  return openModal({ kind: "alert", type: "info", message, ...options });
}

/**
 * 替代原生 confirm()。返回一个 Promise，resolve 传入 true / false。
 * @param {string} message
 * @param {{ title?: string, type?: 'info'|'success'|'warn'|'error', okText?: string, cancelText?: string, danger?: boolean }} [options]
 */
function showConfirm(message, options = {}) {
  // kind 决定弹窗类别（alert/confirm），type 决定图标颜色，两者不能冲突
  return openModal({ kind: "confirm", type: "info", message, ...options });
}
