// Learn more about Tauri commands at https://tauri.app/develop/calling-rust/
use std::fs;
use std::path::PathBuf;
use std::time::Duration;

use futures_util::StreamExt;
use serde::Serialize;
use tauri::{AppHandle, Emitter};

/// 单次请求的连接超时
const CONNECT_TIMEOUT: Duration = Duration::from_secs(8);
/// 整体请求（包含建连 + 等待响应头）的超时
const REQUEST_TIMEOUT: Duration = Duration::from_secs(15);
/// 单个 chunk 读取之间的最大间隔（防止服务器挂起不返回数据）
const READ_CHUNK_TIMEOUT: Duration = Duration::from_secs(20);
/// 整体下载的最大允许耗时（兜底）
const OVERALL_TIMEOUT: Duration = Duration::from_secs(60);

#[tauri::command]
fn greet(name: &str) -> String {
    format!("Hello, {}! You've been greeted from Rust!", name)
}

/// 获取用于存放 /game 的根目录：
/// - 调试时（debug build）：项目根目录下的 `run` 子目录
/// - 打包运行时（release build）：程序可执行文件所在目录
fn app_root_dir() -> Result<PathBuf, String> {
    #[cfg(debug_assertions)]
    {
        let manifest_dir = std::env::var("CARGO_MANIFEST_DIR")
            .map_err(|e| format!("读取 CARGO_MANIFEST_DIR 失败: {}", e))?;
        let project_root = PathBuf::from(manifest_dir)
            .parent()
            .ok_or_else(|| "无法解析项目根目录".to_string())?
            .to_path_buf();
        Ok(project_root.join("run"))
    }

    #[cfg(not(debug_assertions))]
    {
        let exe = std::env::current_exe()
            .map_err(|e| format!("获取可执行文件路径失败: {}", e))?;
        let dir = exe
            .parent()
            .ok_or_else(|| "无法解析可执行文件目录".to_string())?
            .to_path_buf();
        Ok(dir)
    }
}

/// 获取 /game 目录的完整路径
fn game_dir() -> Result<PathBuf, String> {
    let root = app_root_dir()?;
    Ok(root.join("game"))
}

/// 读取本地 manifest.json 的版本号与原始内容
#[tauri::command]
fn read_local_manifest() -> Result<LocalManifestInfo, String> {
    let path = game_dir()?.join("manifest.json");

    if !path.exists() {
        return Ok(LocalManifestInfo {
            exists: false,
            version: None,
            content: None,
            path: path.to_string_lossy().to_string(),
        });
    }

    let content =
        fs::read_to_string(&path).map_err(|e| format!("读取 manifest.json 失败: {}", e))?;
    let version = parse_version(&content);

    Ok(LocalManifestInfo {
        exists: true,
        version,
        content: Some(content),
        path: path.to_string_lossy().to_string(),
    })
}

#[derive(serde::Serialize)]
struct LocalManifestInfo {
    exists: bool,
    version: Option<String>,
    content: Option<String>,
    path: String,
}

/// 从 manifest.json 文本中解析 version 字段
fn parse_version(content: &str) -> Option<String> {
    let value: serde_json::Value = serde_json::from_str(content).ok()?;
    value
        .get("version")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
}

/// 服务端域名配置：主用 oss.jjxf.cc，备用 update.jjxf.cc
const PRIMARY_HOST: &str = "https://oss.jjxf.cc";
const BACKUP_HOST: &str = "https://update.jjxf.cc";
/// manifest.json 相对路径
const MANIFEST_PATH: &str = "/True_Pcks/manifest.json";
/// 最多重试次数（包含主用域名 + 备用域名，最多 3 次整体尝试）
const MAX_ATTEMPTS: usize = 3;

/// 按顺序拼接 URL：主用 -> 备用
fn build_manifest_urls() -> Vec<String> {
    vec![
        format!("{}{}", PRIMARY_HOST, MANIFEST_PATH),
        format!("{}{}", BACKUP_HOST, MANIFEST_PATH),
    ]
}

// ============== 进度事件相关 ==============

/// 推送到前端的进度事件 payload
#[derive(Clone, Serialize)]
struct ProgressPayload {
    /// 阶段：init(启动时拉取) / verify(校验) / download(下载/更新)
    phase: String,
    /// 当前正在请求的 URL
    url: String,
    /// 当前第几次尝试（1-based）
    attempt: u32,
    /// 已下载字节数
    downloaded: u64,
    /// 总字节数（无 Content-Length 时为 None）
    total: Option<u64>,
    /// 0.0 ~ 100.0，total 未知时为 None
    percent: Option<f32>,
}

/// 进度事件名（前端用 listen("manifest:progress", ...) 监听）
const EVT_PROGRESS: &str = "manifest:progress";
const EVT_DONE: &str = "manifest:done";
const EVT_ERROR: &str = "manifest:error";

#[derive(Clone, Serialize)]
struct DonePayload {
    phase: String,
    url: String,
    total_bytes: u64,
}

#[derive(Clone, Serialize)]
struct ErrorPayload {
    phase: String,
    url: String,
    message: String,
}

/// 通过 Tauri 把事件 emit 到前端；前端不在线也不影响逻辑。
fn emit_progress(app: &AppHandle, payload: ProgressPayload) {
    let _ = app.emit(EVT_PROGRESS, payload);
}

fn emit_done(app: &AppHandle, payload: DonePayload) {
    let _ = app.emit(EVT_DONE, payload);
}

fn emit_error(app: &AppHandle, payload: ErrorPayload) {
    let _ = app.emit(EVT_ERROR, payload);
}

/// 构建一个带超时的 HTTP 客户端
fn build_http_client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .user_agent("jjxf_launcher/0.1.0")
        .connect_timeout(CONNECT_TIMEOUT)
        .timeout(REQUEST_TIMEOUT)
        .build()
        .map_err(|e| format!("构建 HTTP 客户端失败: {}", e))
}

/// 流式下载（async）：边读 chunk 边 emit 进度；返回完整 body
async fn download_with_progress(
    app: &AppHandle,
    client: &reqwest::Client,
    url: &str,
    phase: &str,
    attempt: u32,
) -> Result<String, String> {
    // 1) 发送请求（受 REQUEST_TIMEOUT 约束）
    let response = client
        .get(url)
        .send()
        .await
        .map_err(|e| format!("请求 {} 失败: {}", url, e))?;

    let status = response.status();
    if !status.is_success() {
        return Err(format!("HTTP 状态码错误: {} (url={})", status, url));
    }

    let total: Option<u64> = response.content_length();
    // 立即先发一条 0% 进度，让前端立刻显示进度条
    emit_progress(
        app,
        ProgressPayload {
            phase: phase.to_string(),
            url: url.to_string(),
            attempt,
            downloaded: 0,
            total,
            percent: Some(0.0),
        },
    );

    // 2) 流式读取 body，每隔一个 chunk 推一次进度
    let mut stream = response.bytes_stream();
    let mut body: Vec<u8> = Vec::with_capacity(total.unwrap_or(0) as usize);
    let mut downloaded: u64 = 0;

    loop {
        // 单次读取受 READ_CHUNK_TIMEOUT 约束，防止对端挂起导致无限等待
        let next = match tokio::time::timeout(READ_CHUNK_TIMEOUT, stream.next()).await {
            Ok(Some(item)) => item,
            // 流正常结束（服务端关闭连接）视为成功 EOF
            Ok(None) => break,
            Err(_) => {
                return Err(format!(
                    "读取响应内容超时（>{}s）: {}",
                    READ_CHUNK_TIMEOUT.as_secs(),
                    url
                ));
            }
        };

        let chunk = next.map_err(|e| format!("读取响应内容失败: {}", e))?;
        let n = chunk.len();
        if n == 0 {
            break;
        }
        body.extend_from_slice(&chunk);
        downloaded += n as u64;

        let percent = total.map(|t| {
            if t == 0 {
                100.0
            } else {
                (downloaded as f32 / t as f32) * 100.0
            }
        });

        emit_progress(
            app,
            ProgressPayload {
                phase: phase.to_string(),
                url: url.to_string(),
                attempt,
                downloaded,
                total,
                percent,
            },
        );
    }

    let body_str = String::from_utf8(body).map_err(|e| format!("响应不是合法 UTF-8: {}", e))?;
    Ok(body_str)
}

/// 带重试 + 多域名 + 进度事件 的 manifest 拉取（async）
/// 1) 优先尝试主用域名
/// 2) 主用失败时切换到备用域名
/// 3) 整个过程最多重试 3 次（主备用域名按需轮换）
/// 4) 全程通过事件把下载进度推给前端
/// 5) 整个流程受 OVERALL_TIMEOUT 兜底，再也不会无限阻塞
async fn fetch_with_fallback_async(app: &AppHandle, phase: &str) -> Result<String, String> {
    let client = build_http_client()?;
    let urls = build_manifest_urls();
    let mut last_err: Option<String> = None;

    let overall = tokio::time::timeout(OVERALL_TIMEOUT, async {
        for attempt in 1..=MAX_ATTEMPTS {
            // 第 1 次用主用，之后轮询主->备->主
            let url = urls[(attempt - 1) % urls.len()].clone();
            println!(
                "[Launcher] 第 {}/{} 次尝试拉取 manifest: {}",
                attempt, MAX_ATTEMPTS, url
            );

            match download_with_progress(&app, &client, &url, phase, attempt as u32).await {
                Ok(body) => {
                    println!("[Launcher] 第 {} 次拉取成功: {}", attempt, url);
                    emit_done(
                        app,
                        DonePayload {
                            phase: phase.to_string(),
                            url: url.clone(),
                            total_bytes: body.len() as u64,
                        },
                    );
                    return Ok(body);
                }
                Err(e) => {
                    eprintln!("[Launcher] 第 {} 次拉取失败: {}", attempt, e);
                    emit_error(
                        app,
                        ErrorPayload {
                            phase: phase.to_string(),
                            url: url.clone(),
                            message: e.clone(),
                        },
                    );
                    last_err = Some(e);
                }
            }
        }

        Err(format!(
            "已重试 {} 次仍无法获取 manifest.json，最后错误: {}",
            MAX_ATTEMPTS,
            last_err.unwrap_or_else(|| "未知错误".to_string())
        ))
    })
    .await;

    match overall {
        Ok(result) => result,
        Err(_) => Err(format!(
            "获取 manifest.json 整体超时（>{}s），已放弃",
            OVERALL_TIMEOUT.as_secs()
        )),
    }
}

/// 兼容旧签名：仍允许前端传入 url 走单次请求（带进度事件 + 超时）
#[tauri::command]
async fn fetch_manifest(app: AppHandle, url: String) -> Result<String, String> {
    let client = build_http_client()?;
    match tokio::time::timeout(
        OVERALL_TIMEOUT,
        download_with_progress(&app, &client, &url, "legacy", 1),
    )
    .await
    {
        Ok(result) => result,
        Err(_) => Err(format!(
            "获取 {} 整体超时（>{}s），已放弃",
            url,
            OVERALL_TIMEOUT.as_secs()
        )),
    }
}

/// 使用内置主备域名 + 最多 3 次重试拉取 manifest.json，并把下载进度推给前端
/// phase: "init" / "verify" / "download"
#[tauri::command]
async fn fetch_manifest_with_fallback(
    app: AppHandle,
    phase: Option<String>,
) -> Result<String, String> {
    let phase = phase.unwrap_or_else(|| "init".to_string());
    fetch_with_fallback_async(&app, &phase).await
}

/// 解析服务端 manifest.json 的版本号
#[tauri::command]
fn parse_manifest_version(content: String) -> Option<String> {
    parse_version(&content)
}

/// 将服务端 manifest.json 内容保存到 <app_root>/game/manifest.json
#[tauri::command]
fn save_manifest(content: String) -> Result<String, String> {
    let dir = game_dir()?;
    fs::create_dir_all(&dir).map_err(|e| format!("创建 game 目录失败: {}", e))?;
    let path = dir.join("manifest.json");
    fs::write(&path, content).map_err(|e| format!("写入 manifest.json 失败: {}", e))?;
    Ok(path.to_string_lossy().to_string())
}

/// 卸载游戏：清空整个 game 目录
/// 返回是否真的执行了清理（即 game 目录存在过）
#[tauri::command]
fn delete_manifest() -> Result<bool, String> {
    let dir = game_dir()?;
    if !dir.exists() {
        return Ok(false);
    }
    fs::remove_dir_all(&dir).map_err(|e| format!("清空 game 目录失败: {}", e))?;
    Ok(true)
}

/// 返回当前 app root 路径
#[tauri::command]
fn get_working_dir() -> Result<String, String> {
    let dir = app_root_dir()?;
    Ok(dir.to_string_lossy().to_string())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            greet,
            get_working_dir,
            read_local_manifest,
            fetch_manifest,
            fetch_manifest_with_fallback,
            parse_manifest_version,
            save_manifest,
            delete_manifest
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
