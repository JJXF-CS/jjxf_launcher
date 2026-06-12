// ============== 全局状态 ==============

// 服务端 manifest
let serverManifestContent = null;
let serverManifestVersion = null;

// 本地 manifest
let localManifestVersion = null;
let localManifestExists = false;

// verify.json 状态（从后端 read_verify_state 获取）
let verifyState = null;

// 按钮状态：download / continue_download / update / play
let currentButtonState = "download";

// 是否需要更新（通过 check_update_needed 检测 sha256 / path 变更）
let needsUpdate = false;

// 需要更新的文件列表（如 ["game.exe", "Arts.pck"]）
let outdatedFiles = [];

// 菜单卡片相关
let isMenuOpen = false;

// 进度条相关
let currentPhase = null; // init / verify / download / exe / packs / null
let progressHideTimer = null;

// 记录上一次进度条上的 phase
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

export function getVerifyState() {
  return verifyState;
}

export function setVerifyState(v) {
  verifyState = v;
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

export function getNeedsUpdate() {
  return needsUpdate;
}

export function setNeedsUpdate(v) {
  needsUpdate = v;
}

export function getOutdatedFiles() {
  return outdatedFiles;
}

export function setOutdatedFiles(v) {
  outdatedFiles = v;
}
