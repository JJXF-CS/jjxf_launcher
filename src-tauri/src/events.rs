// ============== 进度事件相关 ==============

use serde::Serialize;
use tauri::{AppHandle, Emitter};

use crate::config::{EVT_DONE, EVT_ERROR, EVT_PROGRESS};

/// 推送到前端的进度事件 payload
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

/// 通过 Tauri 把事件 emit 到前端；前端不在线也不影响逻辑。
pub fn emit_progress(app: &AppHandle, payload: ProgressPayload) {
    let _ = app.emit(EVT_PROGRESS, payload);
}

pub fn emit_done(app: &AppHandle, payload: DonePayload) {
    let _ = app.emit(EVT_DONE, payload);
}

pub fn emit_error(app: &AppHandle, payload: ErrorPayload) {
    let _ = app.emit(EVT_ERROR, payload);
}
