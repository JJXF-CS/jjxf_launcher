// ============== 弹窗核心 (替代 alert / confirm) ==============
//
// 提供两类 API：
//   showAlert(message, options)               -> Promise<void>      (替代 alert)
//   showConfirm(message, options)             -> Promise<boolean>   (替代 confirm)
//
// options 字段：
//   title        弹窗标题，默认 "提示"
//   type         info / success / warn / error  (决定图标颜色)
//   okText       "确定"按钮文案
//   cancelText   "取消"按钮文案
//   danger       true 时把主按钮样式改为红色（适用于危险操作）

import { getModalIconSvg } from "./icons.js";

let modalState = null; // 记录当前打开的弹窗状态，用于 ESC 关闭、点击外部关闭等

export function setupModalListeners() {
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

/**
 * 替代原生 alert()。返回一个 Promise，弹窗关闭后 resolve。
 * @param {string} message
 * @param {{ title?: string, type?: 'info'|'success'|'warn'|'error', okText?: string }} [options]
 */
export function showAlert(message, options = {}) {
  // kind 决定弹窗类别（alert/confirm），type 决定图标颜色，两者不能冲突
  return openModal({ kind: "alert", type: "info", message, ...options });
}

/**
 * 替代原生 confirm()。返回一个 Promise，resolve 传入 true / false。
 * @param {string} message
 * @param {{ title?: string, type?: 'info'|'success'|'warn'|'error', okText?: string, cancelText?: string, danger?: boolean }} [options]
 */
export function showConfirm(message, options = {}) {
  // kind 决定弹窗类别（alert/confirm），type 决定图标颜色，两者不能冲突
  return openModal({ kind: "confirm", type: "info", message, ...options });
}
