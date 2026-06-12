// ============== 进度事件相关 ==============

use serde::Serialize;
use tauri::{AppHandle, Emitter};

use crate::config::{EVT_DONE, EVT_DOWNLOAD_DONE, EVT_DOWNLOAD_ERROR, EVT_DOWNLOAD_PROGRESS, EVT_ERROR, EVT_PROGRESS};

/// 推送到前端的进度事件 payload（manifest 拉取阶段）
#[derive(Clone, Serialize)]
pub struct ProgressPayload {
    /// 阶段：init(启动时拉取) / verify(校验) / download(下载/更新)
    pub phase: String,
    /// 当前正在请求的 URL
    pub url: String,
    /// 当前第几次尝试（1-based）
    pub attempt: u32,
    /// 已下载字节数
    pub downloaded: u64,
    /// 总字节数（无 Content-Length 时为 None）
    pub total: Option<u64>,
    /// 0.0 ~ 100.0，total 未知时为 None
    pub percent: Option<f32>,
}

#[derive(Clone, Serialize)]
pub struct DonePayload {
    pub phase: String,
    pub url: String,
    pub total_bytes: u64,
}

#[derive(Clone, Serialize)]
pub struct ErrorPayload {
    pub phase: String,
    pub url: String,
    pub message: String,
}

// ============== 新下载流程事件 payload ==============

/// 单阶段（exe 或 packs）总进度事件
#[derive(Clone, Serialize)]
pub struct DownloadProgressPayload {
    /// 阶段："exe" | "packs" | "verify"
    pub stage: String,
    /// 当前正在处理的文件名（exe 阶段固定为 "game.exe"，packs 阶段为 pack 名称）
    pub current_file: String,
    /// 已完成文件数
    pub files_done: u32,
    /// 总文件数（exe 阶段固定为 1）
    pub files_total: u32,
    /// 当前文件已下载字节数
    pub file_downloaded: u64,
    /// 当前文件总字节数（manifest 中给出，exe 阶段服务端 Content-Length 已知时也会覆盖）
    pub file_total: Option<u64>,
    /// 当前文件 0.0~100.0
    pub file_percent: Option<f32>,
    /// 阶段整体 0.0~100.0（已包含「跨文件累计」）
    pub stage_percent: f32,
    /// 整体（exe + packs 两段加权后）0.0~100.0
    pub overall_percent: f32,
    /// 当前第几次尝试（1-based），用于前端显示「重试中」
    pub attempt: u32,
    /// 当前正在请求的 URL（可选）
    pub url: Option<String>,
    /// 跨所有文件的累计已下载字节数（exe 阶段 = 当前已下载字节，packs 阶段 = 之前所有 pack + 当前已下载字节）
    #[serde(default)]
    pub total_downloaded: u64,
    /// 所有文件的总字节数（exe 大小 + 所有 pack 大小之和）
    #[serde(default)]
    pub total_bytes: u64,
}

/// 单个文件下载完成事件
#[derive(Clone, Serialize)]
pub struct DownloadFileDonePayload {
    pub stage: String,
    /// 相对路径：exe 阶段为 "game.exe"，packs 阶段为 "Arts.pck"
    pub file: String,
    pub ok: bool,
    /// 校验状态："verified" / "mismatch" / "missing" / "ok"
    pub verify_status: String,
    /// sha256（如果 ok=true 才会有意义）
    pub sha256: Option<String>,
    pub size: u64,
}

/// 下载整流程结束事件
#[derive(Clone, Serialize)]
pub struct DownloadDonePayload {
    pub ok: bool,
    pub message: String,
    /// 失败的文件列表（如果有）
    pub failed_files: Vec<String>,
}

/// 错误事件
#[derive(Clone, Serialize)]
pub struct DownloadErrorPayload {
    pub stage: String,
    pub file: Option<String>,
    pub url: Option<String>,
    pub message: String,
}

// ============== Emit 辅助 ==============

pub fn emit_progress(app: &AppHandle, payload: ProgressPayload) {
    let _ = app.emit(EVT_PROGRESS, payload);
}

pub fn emit_done(app: &AppHandle, payload: DonePayload) {
    let _ = app.emit(EVT_DONE, payload);
}

pub fn emit_error(app: &AppHandle, payload: ErrorPayload) {
    let _ = app.emit(EVT_ERROR, payload);
}

pub fn emit_download_progress(app: &AppHandle, payload: DownloadProgressPayload) {
    let _ = app.emit(EVT_DOWNLOAD_PROGRESS, payload);
}

pub fn emit_download_file_done(app: &AppHandle, payload: DownloadFileDonePayload) {
    let _ = app.emit("download:file_done", payload);
}

pub fn emit_download_error(app: &AppHandle, payload: DownloadErrorPayload) {
    let _ = app.emit(EVT_DOWNLOAD_ERROR, payload);
}
