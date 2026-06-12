// ============== 全局状态 ==============

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

// 记录上一次进度条上的 phase，用于 finishProgress / hideProgressImmediately 后恢复按钮文案
let _loadingActivePhase = null;

// 记住主按钮进入 loading 之前的原始文案，恢复时使用
let lastPrimaryButtonText = null;

export function getServerManifestContent() {
  return serverManifestContent;
}

export function setServerManifestContent(v) {
  serverManifestContent = v;
}

export function getServerManifestVersion() {
  return serverManifestVersion;
}

export function setServerManifestVersion(v) {
  serverManifestVersion = v;
}

export function getLocalManifestVersion() {
  return localManifestVersion;
}

export function setLocalManifestVersion(v) {
  localManifestVersion = v;
}

export function getLocalManifestExists() {
  return localManifestExists;
}

export function setLocalManifestExists(v) {
  localManifestExists = v;
}

export function getCurrentButtonState() {
  return currentButtonState;
}

export function setCurrentButtonState(v) {
  currentButtonState = v;
}

export function getIsMenuOpen() {
  return isMenuOpen;
}

export function setIsMenuOpen(v) {
  isMenuOpen = v;
}

export function getCurrentPhase() {
  return currentPhase;
}

export function setCurrentPhase(v) {
  currentPhase = v;
}

export function getProgressHideTimer() {
  return progressHideTimer;
}

export function setProgressHideTimer(v) {
  progressHideTimer = v;
}

export function getLoadingActivePhase() {
  return _loadingActivePhase;
}

export function setLoadingActivePhase(v) {
  _loadingActivePhase = v;
}

export function getLastPrimaryButtonText() {
  return lastPrimaryButtonText;
}

export function setLastPrimaryButtonText(v) {
  lastPrimaryButtonText = v;
}
