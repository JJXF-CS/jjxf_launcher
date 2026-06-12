// ============== 网络下载 (HTTP client / 流式下载 / 主备域名重试拉取) ==============

use futures_util::StreamExt;
use tauri::AppHandle;

use crate::config::{
    BACKUP_HOST, CONNECT_TIMEOUT, MANIFEST_PATH, MAX_ATTEMPTS, OVERALL_TIMEOUT, PRIMARY_HOST,
    READ_CHUNK_TIMEOUT, REQUEST_TIMEOUT,
};
use crate::events::{emit_done, emit_error, emit_progress, DonePayload, ErrorPayload, ProgressPayload};

/// 按顺序拼接 URL：主用 -> 备用
fn build_manifest_urls() -> Vec<String> {
    vec![
        format!("{}{}", PRIMARY_HOST, MANIFEST_PATH),
        format!("{}{}", BACKUP_HOST, MANIFEST_PATH),
    ]
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
pub async fn fetch_manifest(app: AppHandle, url: String) -> Result<String, String> {
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
pub async fn fetch_manifest_with_fallback(
    app: AppHandle,
    phase: Option<String>,
) -> Result<String, String> {
    let phase = phase.unwrap_or_else(|| "init".to_string());
    fetch_with_fallback_async(&app, &phase).await
}
