// ============== 游戏下载核心 ==============
// 完整流程：
//   1) 读取本地 manifest.json
//   2) 阶段 1: 下载 game.exe (单文件进度 = 阶段进度)
//   3) 阶段 2: 顺序下载所有 pck，跨文件累计
//   4) 每个文件下载完成后：写本地文件 + 校验 sha256 + 更新 verify.json
//
// 设计原则：
//   - 支持断点续传：检测已下载的字节数，用 HTTP Range 头从断点处继续
//   - 单文件最大 FILE_MAX_ATTEMPTS 次重试（主备域名轮换）
//   - 失败的 pck 不打断后续下载；最终 DownloadDonePayload 汇总失败列表

use std::fs;
use std::io::{Read, Seek, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, atomic::{AtomicU64, AtomicU32, Ordering}};
use futures_util::StreamExt;
use futures_util::stream;
use log::{error, info, warn};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use tauri::AppHandle;
use crate::config::{
    BACKUP_HOST, CHUNK_MAX_CONSECUTIVE_FAILS, CHUNK_MIN_FILE_SIZE, CHUNK_MIN_SIZE,
    CHUNK_RETRY_BASE_MS, CONNECT_TIMEOUT, DEFAULT_PACKS_PATH, FILE_CHUNK_CONCURRENCY,
    FILE_MAX_ATTEMPTS, GAME_EXE_NAME, PACK_FILE_EXT, PACKS_PARALLEL_DOWNLOADS,
    PRIMARY_HOST, READ_CHUNK_TIMEOUT,
};


use crate::events::{
    emit_download_error, emit_download_file_done, emit_download_progress, DownloadDonePayload,
    DownloadErrorPayload, DownloadFileDonePayload, DownloadProgressPayload,
};
use crate::paths::{game_dir, game_exe_path, hot_update_dir, pack_file_path};
use crate::verify_state::{self, FileVerifyRecord};

// ============== manifest 结构 ==============

#[derive(Debug, Deserialize)]
struct PackInfo {
    sha256: String,
    size: u64,
    #[serde(default)]
    source: Option<String>,
    #[serde(default)]
    version: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ExeInfo {
    sha256: String,
    size: u64,
    #[serde(default)]
    version: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ManifestFile {
    packs: std::collections::BTreeMap<String, PackInfo>,
    #[serde(default)]
    exe: Option<ExeInfo>,
    #[serde(default)]
    version: Option<String>,
    #[serde(default)]
    path: Option<String>,
}

#[derive(Debug, Clone)]
struct PackEntry {
    name: String,
    url: String,
    local_path: PathBuf,
    expected_sha256: String,
    expected_size: u64,
}

#[derive(Debug, Clone)]
struct ExeEntry {
    url: String,
    local_path: PathBuf,
}

// ============== 阶段权重 ==============
const EXE_STAGE_WEIGHT: f32 = 5.0;
const PACKS_STAGE_WEIGHT: f32 = 95.0;
/// 同时下载的 pck 文件数（保留为历史常量；现在为 1，即单文件顺序下载）
const DOWNLOAD_CONCURRENCY: usize = 1;

// ============== 线程状态统计 ==============
// 后端控制台每 1 秒打印一次实时线程状态：
//   requesting: 正在发 Range 请求的线程数
//   receiving : 正在接收数据块的线程数
//   failed    : 失败总数（按重试累加）
//   total     : 当前文件总分片数
//   speed     : 近 1 秒平均下载速率

#[derive(Default)]
struct ThreadStats {
    requesting: AtomicU32,
    receiving: AtomicU32,
    failed: AtomicU32,
    total: AtomicU32,
    /// 由 pull_chunk 累加的实时已下载字节总数
    last_bytes: AtomicU64,
    /// 记录上次报告速度时的已下载字节（与 last_bytes 拆分避免被并发 swap）
    last_report: AtomicU64,
    last_print: std::sync::Mutex<Option<std::time::Instant>>,
    started_at: std::sync::Mutex<Option<std::time::Instant>>,
}


impl ThreadStats {
    fn new() -> Self {
        Self::default()
    }
    fn reset(&self, total: u32) {
        self.requesting.store(0, Ordering::Relaxed);
        self.receiving.store(0, Ordering::Relaxed);
        self.failed.store(0, Ordering::Relaxed);
        self.total.store(total, Ordering::Relaxed);
        self.last_bytes.store(0, Ordering::Relaxed);
        self.last_report.store(0, Ordering::Relaxed);
        *self.last_print.lock().unwrap() = None;
        *self.started_at.lock().unwrap() = Some(std::time::Instant::now());
    }

    fn finish(&self) {
        self.requesting.store(0, Ordering::Relaxed);
        self.receiving.store(0, Ordering::Relaxed);
    }
    fn inc_requesting(&self) {
        self.requesting.fetch_add(1, Ordering::Relaxed);
    }
    fn dec_requesting(&self) {
        self.requesting.fetch_sub(1, Ordering::Relaxed);
    }
    fn inc_receiving(&self) {
        self.receiving.fetch_add(1, Ordering::Relaxed);
    }
    fn dec_receiving(&self) {
        self.receiving.fetch_sub(1, Ordering::Relaxed);
    }
    fn inc_failed(&self) {
        self.failed.fetch_add(1, Ordering::Relaxed);
    }
    /// 记录已下载字节并计算速率
    /// total_bytes: 当前从 last_bytes 读到的总字节
    fn record_bytes(&self, total_bytes: u64) -> Option<(f64, String)> {
        let now = std::time::Instant::now();
        let mut last_print_guard = self.last_print.lock().unwrap();
        let should_print = match *last_print_guard {
            Some(t) => now.duration_since(t) >= std::time::Duration::from_secs(1),
            None => true,
        };
        if !should_print {
            return None;
        }
        // 从 last_report 中拿出上次报告的字节
        let prev = self.last_report.load(Ordering::Relaxed);
        let delta_bytes = total_bytes.saturating_sub(prev);
        let elapsed = match *last_print_guard {
            Some(t) => now.duration_since(t).as_secs_f64().max(0.001),
            None => 1.0,
        };
        *last_print_guard = Some(now);
        self.last_report.store(total_bytes, Ordering::Relaxed);
        let speed_bps = delta_bytes as f64 / elapsed;
        let speed_str = format_speed(speed_bps);
        Some((speed_bps, speed_str))
    }

}

fn format_speed(bps: f64) -> String {
    if bps < 1024.0 {
        format!("{:.0} B/s", bps)
    } else if bps < 1024.0 * 1024.0 {
        format!("{:.1} KB/s", bps / 1024.0)
    } else {
        format!("{:.2} MB/s", bps / 1024.0 / 1024.0)
    }
}

/// 启动一个后台任务：每 1 秒打印一次线程状态。
/// 返回一个 oneshot 发送端，调用者完成后发送 () 停止打印。
fn spawn_stats_monitor(stats: Arc<ThreadStats>, file_label: String) -> tokio::sync::oneshot::Sender<()> {
    let (stop_tx, mut stop_rx) = tokio::sync::oneshot::channel::<()>();
    tokio::spawn(async move {
        loop {
            tokio::select! {
                _ = tokio::time::sleep(std::time::Duration::from_millis(500)) => {
                    let req = stats.requesting.load(Ordering::Relaxed);
                    let rec = stats.receiving.load(Ordering::Relaxed);
                    let fail = stats.failed.load(Ordering::Relaxed);
                    let total = stats.total.load(Ordering::Relaxed);
                    let bytes = stats.last_bytes.load(Ordering::Relaxed);
                    let speed_str = stats
                        .record_bytes(bytes)
                        .map(|(_, s)| s)
                        .unwrap_or_else(|| "测量中…".to_string());
                    let line = format!(
                        "[Downloader] {}  总分片={}  请求中={}  接收中={}  失败={}  速度={}",
                        file_label, total, req, rec, fail, speed_str
                    );
                    //下载日志
                    //println!("{}", line);
                }
                _ = &mut stop_rx => {
                    break;
                }
            }
        }
    });
    stop_tx
}



fn percent_of_packs(downloaded: u64, total: u64) -> f32 {
    if total == 0 {
        100.0
    } else {
        (downloaded as f32 / total as f32) * 100.0
    }
}

fn compute_overall_percent(exe_done: bool, packs_pct: f32) -> f32 {
    let exe_pct = if exe_done { EXE_STAGE_WEIGHT } else { 0.0 };
    let packs_pct = packs_pct.clamp(0.0, 100.0);
    exe_pct + (packs_pct / 100.0) * PACKS_STAGE_WEIGHT
}

// ============== 主入口 ==============

#[tauri::command]
pub async fn start_download(app: AppHandle) -> Result<DownloadDonePayload, String> {
    start_download_internal(&app).await
}

async fn start_download_internal(app: &AppHandle) -> Result<DownloadDonePayload, String> {
    // 1) 读本地 manifest
    let local_manifest_path = game_dir()?.join("manifest.json");
    let content = fs::read_to_string(&local_manifest_path)
        .map_err(|e| format!("读取本地 manifest.json 失败: {}", e))?;
    let manifest: ManifestFile = serde_json::from_str(&content)
        .map_err(|e| format!("解析本地 manifest.json 失败: {}", e))?;

    // 2) 确定服务端资源路径前缀
    //    新格式: manifest 中 `path` 字段指定了所有资源所在的子目录（如 "0.8.2.9_6_13"）
    //    旧格式回退: 若无 path 字段则使用 DEFAULT_PACKS_PATH（"True_Pcks"）
    let packs_path = manifest
        .path
        .as_deref()
        .unwrap_or(DEFAULT_PACKS_PATH);

    // 3) 构建下载任务列表
    let exe = ExeEntry {
        url: format!("{}/{}/{}", PRIMARY_HOST, packs_path, GAME_EXE_NAME),
        local_path: game_exe_path()?,
    };
    let packs: Vec<PackEntry> = manifest
        .packs
        .iter()
        .map(|(name, info)| PackEntry {
            name: name.clone(),
            url: format!(
                "{}/{}/{}{}",
                PRIMARY_HOST, packs_path, name, PACK_FILE_EXT
            ),
            local_path: pack_file_path(name).unwrap(),
            expected_sha256: info.sha256.to_lowercase(),
            expected_size: info.size,
        })
        .collect();

    // 3) 计算 pack 总大小（HEAD 请求 + 后续下载都需要）
    let total_pack_bytes: u64 = packs.iter().map(|p| p.expected_size).sum();

    // 4) 读取 verify.json 现状，并用 manifest 中的 sha256 做精确比对
    //    仅在 verify.json status=="ok" 且 sha256 与 manifest 一致时才认为该文件已完成
    let verify = verify_state::load();
    let exe_already_ok = if let Some(ref exe_info) = manifest.exe {
        verify_state::is_exe_sha256_match(&verify, &exe_info.sha256)
            && verify_state::is_manifest_path_match(&verify, packs_path)
    } else {
        // 旧格式 manifest 无 exe 字段，回退到仅检查 status
        verify_state::is_exe_ok(&verify)
    };
    let pack_already_ok: std::collections::HashSet<String> = packs
        .iter()
        .filter(|p| verify_state::is_pack_sha256_match(&verify, &p.name, &p.expected_sha256))
        .map(|p| p.name.clone())
        .collect();

    // 同时清除那些 status=="ok" 但 sha256 已变、或 path 已变的旧记录
    // （这批文件会在本轮下载中重新拉取并覆盖记录）
    let manifest_path_changed = !verify_state::is_manifest_path_match(&verify, packs_path);
    if manifest_path_changed {
        eprintln!("[Downloader] manifest path 已变更 (旧={:?} 新={})，将重新下载所有文件", verify.manifest_path, packs_path);
    }

    if let Some(v) = manifest.version.as_ref() {
        let _ = verify_state::set_manifest_version_and_path(v, packs_path);
    }

    // ========== 阶段 1: 下载 game.exe ==========
    let mut failed_files: Vec<String> = Vec::new();

    // exe 阶段：获取 exe 大小（如果 HEAD 返回 0 则用 Range GET 抓首块推算）
    let exe_url = exe.url.clone();
    let exe_total: u64 = {
        let probe_client = reqwest::Client::builder()
            .user_agent("jjxf_launcher/0.1.0")
            .connect_timeout(CONNECT_TIMEOUT)
            .build()
            .map_err(|e| format!("构建 HTTP 客户端失败: {}", e))?;
        // 先 HEAD
        let head_size = match probe_client.head(&exe_url).send().await {
            Ok(resp) => resp.content_length().unwrap_or(0),
            Err(_) => 0,
        };
        // 如果 HEAD 返回 0，用 Range: bytes=0-0 探测实际文件总大小
        if head_size == 0 {
            match probe_client
                .get(&exe_url)
                .header("Range", "bytes=0-0")
                .send()
                .await
            {
                Ok(resp) => {
                    // 206 时 Content-Range: bytes 0-0/total；200 时退化为单线程
                    if let Some(cr) = resp.headers().get("content-range") {
                        if let Ok(s) = cr.to_str() {
                            if let Some(pos) = s.rfind('/') {
                                if let Ok(n) = s[pos + 1..].parse::<u64>() {
                                    n
                                } else {
                                    0
                                }
                            } else {
                                0
                            }
                        } else {
                            0
                        }
                    } else {
                        resp.content_length().unwrap_or(0)
                    }
                }
                Err(_) => 0,
            }
        } else {
            head_size
        }
    };

    let total_all_bytes = exe_total + total_pack_bytes;

    // exe 下载 + sha256 校验循环（最多重试 FILE_MAX_ATTEMPTS 次）
    let mut exe_attempt = 0;
    let expected_exe_sha256 = manifest.exe.as_ref().map(|ei| ei.sha256.to_lowercase());

    if !exe_already_ok {
        loop {
            exe_attempt += 1;

            // 重试前先删除旧文件（否则断点续传短路由会跳过下载）
            if exe_attempt > 1 {
                let _ = fs::remove_file(&exe.local_path);
            }

            emit_progress(
                app, "exe", &exe.local_path, 0, 1, 0, if exe_total > 0 { Some(exe_total) } else { None }, 0.0, 0.0, exe_attempt as u32,
                Some(exe.url.clone()), 0, exe_total,
            );

            let current_file_label = "game.exe".to_string();
            let exe_shared = Arc::new(AtomicU64::new(0));

            let result = if exe_total > 0 && should_use_chunks(exe_total) {
                download_file_with_retry_chunks(
                    app, &exe.url, &exe.local_path,
                    exe_total, None,
                    "exe", &current_file_label, 0, 1, 0.0,
                    exe_shared.clone(), exe_total,
                ).await
            } else {
                download_file_with_retry(
                    app, &exe.url, &exe.local_path, None, None,
                    "exe", &current_file_label, 0, 1, 0.0,
                    0, exe_total,
                ).await
            };

            match result {
                DownloadOutcome::Ok { size, sha256 } => {
                    // 校验 sha256（如果 manifest 中有 exe.sha256）
                    let computed = compute_file_sha256(&exe.local_path).ok();
                    let sha256_ok = match (&expected_exe_sha256, &computed) {
                        (Some(expected), Some(actual)) => {
                            actual.to_lowercase() == *expected
                        }
                        _ => true, // 无预期 sha256 或无法计算，跳过校验
                    };

                    if sha256_ok {
                        let actual_sha = computed.clone().unwrap_or_default();
                        let rec = FileVerifyRecord {
                            sha256: actual_sha.clone(),
                            size, status: "ok".into(),
                        };
                        let _ = verify_state::upsert_exe(rec);
                        emit_download_file_done(app, DownloadFileDonePayload {
                            stage: "exe".into(), file: "game.exe".into(), ok: true,
                            verify_status: "ok".into(), sha256: Some(actual_sha), size,
                        });
                        emit_progress(app, "exe", &exe.local_path, 1, 1, size, Some(size), 100.0, EXE_STAGE_WEIGHT, exe_attempt as u32, None, 0, exe_total);
                        break;
                    } else {
                        eprintln!("[Downloader] game.exe sha256 不匹配 (第{}次): expected={:?} actual={:?}",
                            exe_attempt, expected_exe_sha256, computed);
                        let _ = fs::remove_file(&exe.local_path);
                        if exe_attempt >= FILE_MAX_ATTEMPTS as usize {
                            failed_files.push("game.exe".to_string());
                            emit_download_error(app, DownloadErrorPayload {
                                stage: "exe".into(), file: Some("game.exe".into()),
                                url: Some(exe.url.clone()),
                                message: format!("sha256 不匹配: expected={:?} actual={:?}", expected_exe_sha256, computed),
                            });
                            return Ok(DownloadDonePayload { ok: false, message: format!("下载 game.exe 失败: sha256 校验不匹配"), failed_files });
                        }
                    }
                }
                DownloadOutcome::Err(e) => {
                    if exe_attempt >= FILE_MAX_ATTEMPTS as usize {
                        failed_files.push("game.exe".to_string());
                        emit_download_error(app, DownloadErrorPayload {
                            stage: "exe".into(), file: Some("game.exe".into()),
                            url: Some(exe.url.clone()), message: e.clone(),
                        });
                        return Ok(DownloadDonePayload { ok: false, message: format!("下载 game.exe 失败: {}", e), failed_files });
                    }
                    eprintln!("[Downloader] game.exe 下载失败 (第{}次): {}", exe_attempt, e);
                }
            }
        }
    } else {
        let rec = verify.exe.clone().unwrap_or(FileVerifyRecord {
            sha256: String::new(), size: 0, status: "ok".into(),
        });
        let _ = verify_state::upsert_exe(rec);
        emit_download_file_done(app, DownloadFileDonePayload {
            stage: "exe".into(), file: "game.exe".into(), ok: true,
            verify_status: "ok".into(),
            sha256: verify.exe.as_ref().map(|r| r.sha256.clone()),
            size: verify.exe.as_ref().map(|r| r.size).unwrap_or(0),
        });
        let exe_size = verify.exe.as_ref().map(|r| r.size).unwrap_or(0);
        emit_progress(app, "exe", &exe.local_path, 1, 1,
            exe_size, Some(exe_size), 100.0, EXE_STAGE_WEIGHT, 1, None,
            exe_size, exe_size);
    }

    // ========== 阶段 2: 下载所有 pck ==========
    fs::create_dir_all(hot_update_dir()?)
        .map_err(|e| format!("创建 hot_update 目录失败: {}", e))?;

    let already_done_bytes: u64 = packs.iter()
        .filter(|p| pack_already_ok.contains(&p.name))
        .map(|p| p.expected_size).sum();
    let already_done_files: u32 = pack_already_ok.len() as u32;

    // 共享原子计数器，供并发任务实时更新总下载字节数
    let shared_downloaded = Arc::new(AtomicU64::new(already_done_bytes));
    let shared_done_files = Arc::new(AtomicU32::new(already_done_files));
    let total_packs = packs.len() as u32;

    // 阶段 2 初始进度
    {
        let stage_pct = percent_of_packs(already_done_bytes, total_pack_bytes);
        let overall = compute_overall_percent(true, stage_pct);
        let _ = emit_download_progress(app, DownloadProgressPayload {
            stage: "packs".into(),
            current_file: packs.first().map(|p| format!("{}{}", p.name, PACK_FILE_EXT)).unwrap_or_default(),
            files_done: already_done_files, files_total: total_packs,
            file_downloaded: 0, file_total: Some(0), file_percent: Some(100.0),
            stage_percent: stage_pct, overall_percent: overall,
            attempt: 1, url: None,
            total_downloaded: already_done_bytes, total_bytes: total_pack_bytes,
            speed: None,
        });
    }

    let pending: Vec<PackEntry> = packs.iter()
        .filter(|p| !pack_already_ok.contains(&p.name))
        .cloned().collect();

    // pck 并行下载（同时最多 PACKS_PARALLEL_DOWNLOADS 个文件）
    // 每个 pck 内部也有 FILE_CHUNK_CONCURRENCY 个 Range 分片并发拉取
    // 下载阶段不校验 sha256，只检查大小——后序统一走资源校验
    let semaphore = Arc::new(tokio::sync::Semaphore::new(PACKS_PARALLEL_DOWNLOADS));
    let concurrent_results: Arc<std::sync::Mutex<Vec<(PackEntry, String, DownloadOutcome)>>> =
        Arc::new(std::sync::Mutex::new(Vec::with_capacity(pending.len())));

    let mut handles = Vec::with_capacity(pending.len());
    for p in pending {
        let permit = semaphore.clone().acquire_owned().await.map_err(|e| format!("获取信号量失败: {}", e))?;
        let app = app.clone();
        let sd = shared_downloaded.clone();
        let sdf = shared_done_files.clone();
        let results = concurrent_results.clone();

        handles.push(tokio::spawn(async move {
            let file_label = format!("{}{}", p.name, PACK_FILE_EXT);
            let done_now = sdf.load(Ordering::Relaxed);
            let pack_pct = percent_of_packs(sd.load(Ordering::Relaxed), total_pack_bytes);
            let pack_overall = compute_overall_percent(true, pack_pct);
            let outcome = if should_use_chunks(p.expected_size) {
                download_file_with_retry_chunks(
                    &app, &p.url, &p.local_path,
                    p.expected_size, None,
                    "packs", &file_label, done_now, total_packs,
                    pack_overall, sd.clone(), total_pack_bytes,
                ).await
            } else {
                download_file_with_retry_concurrent(
                    &app, &p.url, &p.local_path,
                    Some(p.expected_size), None,
                    "packs", &file_label, done_now, total_packs,
                    pack_overall, sd.clone(), total_pack_bytes,
                ).await
            };

            drop(permit);
            results.lock().unwrap().push((p, file_label, outcome));
        }));
    }

    // 等待所有下载任务完成
    for h in handles {
        let _ = h.await;
    }

    let mut concurrent_results: Vec<(PackEntry, String, DownloadOutcome)> =
        Arc::try_unwrap(concurrent_results).unwrap().into_inner().unwrap();


    // 处理下载结果——下载阶段不写 verify.json、不校验 sha256，仅看大小
    for (_p, _file_label, outcome) in &concurrent_results {
        if let DownloadOutcome::Err(e) = outcome {
            failed_files.push(_file_label.clone());
            emit_download_error(app, DownloadErrorPayload {
                stage: "packs".into(), file: Some(_file_label.clone()),
                url: Some(_p.url.clone()), message: e.clone(),
            });
        }
    }

    // ========== 阶段 3: 资源校验（独立进度条） ==========
    // 跑所有 pck 的 sha256 校验，不通过的 pck 进入重下名单
    let mut to_redownload: Vec<PackEntry> = Vec::new();
    {
        // 进度以字节为单位：把 .pck 文件一个个边读边 hash，发字节级进度
        let total_verify_bytes: u64 = concurrent_results.iter()
            .filter_map(|(_, _, o)| if let DownloadOutcome::Ok { size, .. } = o { Some(*size) } else { None })
            .sum();
        let mut verified_bytes: u64 = 0;

        // 校验阶段初始进度
        let _ = emit_download_progress(app, DownloadProgressPayload {
            stage: "verify".into(), current_file: String::new(),
            files_done: 0, files_total: concurrent_results.len() as u32,
            file_downloaded: 0, file_total: Some(0), file_percent: Some(0.0),
            stage_percent: 0.0, overall_percent: 0.0,
            attempt: 1, url: None,
            total_downloaded: 0, total_bytes: total_verify_bytes,
            speed: None,
        });

        for (p, file_label, outcome) in &concurrent_results {
            // 跳过下载阶段就失败的 pck（不需校验）
            if let DownloadOutcome::Err(_) = outcome {
                continue;
            }
            let file_total = if let DownloadOutcome::Ok { size, .. } = outcome { *size } else { 0 };
            // 跑该 pck 的 sha256（边读边发字节级进度）
            let (ok, actual_hash) = verify_pack_with_progress(
                app, p, file_label, file_total, &mut verified_bytes, total_verify_bytes,
            );

            if ok {
                // 校验通过：更新 verify.json
                let rec = FileVerifyRecord {
                    sha256: actual_hash.clone().unwrap_or_default(),
                    size: p.expected_size, status: "ok".into(),
                };
                let _ = verify_state::upsert_pack(&p.name, rec);
                emit_download_file_done(app, DownloadFileDonePayload {
                    stage: "verify".into(), file: file_label.clone(), ok: true,
                    verify_status: "ok".into(), sha256: actual_hash, size: p.expected_size,
                });
            } else {
                // 校验失败：列入重下名单
                to_redownload.push(p.clone());
                let _ = verify_state::upsert_pack(&p.name, FileVerifyRecord {
                    sha256: actual_hash.clone().unwrap_or_default(),
                    size: p.expected_size, status: "mismatch".into(),
                });
                emit_download_file_done(app, DownloadFileDonePayload {
                    stage: "verify".into(), file: file_label.clone(), ok: false,
                    verify_status: "mismatch".into(), sha256: actual_hash, size: p.expected_size,
                });
            }
        }
    }

    // 最终 100% （资源校验阶段）
    let _ = emit_download_progress(app, DownloadProgressPayload {
        stage: "verify".into(), current_file: String::new(),
        files_done: concurrent_results.len() as u32,
        files_total: concurrent_results.len() as u32,
        file_downloaded: 0, file_total: Some(0), file_percent: Some(100.0),
        stage_percent: 100.0, overall_percent: 100.0,
        attempt: 1, url: None,
        total_downloaded: total_pack_bytes, total_bytes: total_pack_bytes,
            speed: None,
    });

    // ========== 阶段 4: 重下校验失败的 pck ==========
    if !to_redownload.is_empty() {
        eprintln!("[Downloader] 资源校验发现 {} 个文件 sha256 不匹配，开始重下", to_redownload.len());
        // 重置共享计数器（去掉这些文件“已下载”的“假”进度）
        for p in &to_redownload {
            // shared_downloaded 减去预期大小（因为它被记在了已下载部分）
            shared_downloaded.fetch_sub(p.expected_size, Ordering::Relaxed);
        }

        // 复用同一个 packs 下载逻辑
        // ⚠️ 重要：必须先删除旧文件，否则 download_*_resume 的断点续传短路
        //     会检测到文件已存在且大小匹配，直接返回旧文件不重新下载
        for p in to_redownload.iter().cloned() {
            let _ = fs::remove_file(&p.local_path);
            let file_label = format!("{}{}", p.name, PACK_FILE_EXT);
            let pack_pct = percent_of_packs(shared_downloaded.load(Ordering::Relaxed), total_pack_bytes);
            let pack_overall = compute_overall_percent(true, pack_pct);
            let outcome = if should_use_chunks(p.expected_size) {
                download_file_with_retry_chunks(
                    app, &p.url, &p.local_path,
                    p.expected_size, None,
                    "packs", &file_label, 0, total_packs,
                    pack_overall, shared_downloaded.clone(), total_pack_bytes,
                ).await
            } else {
                download_file_with_retry_concurrent(
                    app, &p.url, &p.local_path,
                    Some(p.expected_size), None,
                    "packs", &file_label, 0, total_packs,
                    pack_overall, shared_downloaded.clone(), total_pack_bytes,
                ).await
            };

            match outcome {
                DownloadOutcome::Ok { size, .. } => {
                    // 重下成功后必须重新校验 sha256 并更新 verify.json
                    // 否则 verify.json 永远停留在 "mismatch"，下次启动又会重下同一文件
                    let actual_hash = compute_file_sha256(&p.local_path).ok();
                    let ok = actual_hash.as_ref().map_or(false, |h| {
                        h.to_lowercase() == p.expected_sha256.to_lowercase()
                    });

                    shared_downloaded.fetch_add(size, Ordering::Relaxed);

                    let actual_for_log = actual_hash.clone();
                    if ok {
                        let rec = FileVerifyRecord {
                            sha256: actual_hash.clone().unwrap_or_default(),
                            size: p.expected_size,
                            status: "ok".into(),
                        };
                        let _ = verify_state::upsert_pack(&p.name, rec);
                        emit_download_file_done(app, DownloadFileDonePayload {
                            stage: "verify".into(),
                            file: file_label.clone(),
                            ok: true,
                            verify_status: "ok".into(),
                            sha256: actual_hash,
                            size: p.expected_size,
                        });
                        eprintln!("[Downloader] 重下 {} 成功，sha256 校验通过", file_label);
                    } else {
                        failed_files.push(file_label.clone());
                        let _ = verify_state::upsert_pack(&p.name, FileVerifyRecord {
                            sha256: actual_hash.clone().unwrap_or_default(),
                            size: p.expected_size,
                            status: "mismatch".into(),
                        });
                        emit_download_file_done(app, DownloadFileDonePayload {
                            stage: "verify".into(),
                            file: file_label.clone(),
                            ok: false,
                            verify_status: "mismatch".into(),
                            sha256: actual_hash,
                            size: p.expected_size,
                        });
                        eprintln!("[Downloader] 重下 {} 后 sha256 仍不匹配: expected={} actual={:?}",
                            file_label, p.expected_sha256, actual_for_log);
                    }
                }
                DownloadOutcome::Err(e) => {
                    failed_files.push(file_label.clone());
                    emit_download_error(app, DownloadErrorPayload {
                        stage: "packs".into(), file: Some(file_label),
                        url: Some(p.url.clone()), message: e,
                    });
                }
            }
        }
    }

    if failed_files.is_empty() {
        Ok(DownloadDonePayload { ok: true, message: "下载完成".into(), failed_files })
    } else {
        Ok(DownloadDonePayload { ok: false, message: format!("有 {} 个文件下载失败", failed_files.len()), failed_files })
    }
}


// ============== 通用下载 + 校验 + 写文件 ==============

#[derive(Debug)]
enum DownloadOutcome {
    Ok { size: u64, sha256: Option<String> },
    Err(String),
}

#[allow(clippy::too_many_arguments)]
fn emit_progress(
    app: &AppHandle, stage: &str, local_path: &Path,
    files_done: u32, files_total: u32, file_downloaded: u64, file_total: Option<u64>,
    stage_percent: f32, overall_percent: f32, attempt: u32, url: Option<String>,
    base_bytes: u64, total_all_bytes: u64,
) {
    let _ = emit_download_progress(app, DownloadProgressPayload {
        stage: stage.to_string(),
        current_file: local_path.file_name().and_then(|s| s.to_str()).unwrap_or("?").to_string(),
        files_done, files_total, file_downloaded, file_total,
        file_percent: file_total.map(|t| if t == 0 { 100.0 } else { (file_downloaded as f32 / t as f32) * 100.0 }),
        stage_percent, overall_percent, attempt, url,
        total_downloaded: base_bytes.saturating_add(file_downloaded),
        total_bytes: total_all_bytes,
            speed: None,
    });
}

/// 通用：单文件下载 + 重试 + 断点续传（支持 HTTP Range）
/// stage_overall_percent: 外层传入的整体进度基准值（packs 阶段由外层控制）
/// base_bytes: 已完成文件的累计字节数
/// total_all_bytes: 所有文件总字节数
#[allow(clippy::too_many_arguments)]
async fn download_file_with_retry(
    app: &AppHandle,
    primary_url: &str,
    local_path: &Path,
    expected_size: Option<u64>,
    expected_sha256: Option<&str>,
    stage: &str,
    file_label: &str,
    files_done: u32,
    files_total: u32,
    stage_overall_percent: f32,
    base_bytes: u64,
    total_all_bytes: u64,
) -> DownloadOutcome {
    let urls: Vec<String> = build_urls(primary_url);
    let mut last_err: Option<String> = None;

    for attempt in 1..=FILE_MAX_ATTEMPTS {
        let url = urls[(attempt - 1) % urls.len()].clone();

        match download_to_file_resume(
            &url, local_path, expected_size, stage, file_label,
            files_done, files_total, attempt as u32, app, stage_overall_percent,
            base_bytes, total_all_bytes,
        ).await {
            Ok((size, sha)) => {
                // sha256 校验
                if let Some(exp) = expected_sha256 {
                    if let Some(ref actual) = sha {
                        if actual.to_lowercase() != exp.to_lowercase() {
                            // 校验失败：删除文件，标记 mismatch
                            let _ = fs::remove_file(local_path);
                            return DownloadOutcome::Err(format!(
                                "sha256 不匹配: expected={} actual={}", exp, actual
                            ));
                        }
                    }
                }
                return DownloadOutcome::Ok { size, sha256: sha };
            }
            Err(e) => {
                eprintln!("[Downloader] {} 第 {}/{} 次失败: {}", file_label, attempt, FILE_MAX_ATTEMPTS, e);
                last_err = Some(e.clone());
                if attempt == FILE_MAX_ATTEMPTS {
                    return DownloadOutcome::Err(last_err.unwrap_or_else(|| "未知错误".into()));
                }
            }
        }
    }
    DownloadOutcome::Err(last_err.unwrap_or_else(|| "未知错误".into()))
}

/// 带断点续传的文件下载（HTTP Range）+ 进度回调
/// stage_overall_percent: 外层传入的整体进度基准值（packs 阶段由外层控制）
/// base_bytes: 已完成文件的累计字节数（用于计算 total_downloaded）
/// total_all_bytes: 所有文件总字节数（用于显示 total_bytes）
#[allow(clippy::too_many_arguments)]
async fn download_to_file_resume(
    url: &str,
    local_path: &Path,
    expected_size: Option<u64>,
    stage: &str,
    file_label: &str,
    files_done: u32,
    files_total: u32,
    attempt: u32,
    app: &AppHandle,
    stage_overall_percent: f32,
    base_bytes: u64,
    total_all_bytes: u64,
) -> Result<(u64, Option<String>), String> {
    if let Some(parent) = local_path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("创建目录失败: {}", e))?;
    }

    // 检查已有文件大小（断点续传起点）
    let existing_bytes: u64 = fs::metadata(local_path).map(|m| m.len()).unwrap_or(0);

    let client = reqwest::Client::builder()
        .user_agent("jjxf_launcher/0.1.0")
        .connect_timeout(CONNECT_TIMEOUT)
        .build()
        .map_err(|e| format!("构建 HTTP 客户端失败: {}", e))?;

    let mut request = client.get(url);
    // 如果已有部分数据且预期大小已知，发送 Range 头
    if existing_bytes > 0 {
        if let Some(total) = expected_size {
            if existing_bytes >= total {
                // 文件已经下载完成，直接校验 sha256
                let hash = compute_file_sha256(local_path).ok();
                return Ok((existing_bytes, hash));
            }
        }
        // 从 existing_bytes 处继续下载（范围从 existing_bytes 开始到文件尾）
        // HTTP Range: bytes=<start>-
        request = request.header("Range", format!("bytes={}-", existing_bytes));
    }

    let response = request.send().await.map_err(|e| format!("请求失败: {}", e))?;

    let status = response.status();
    let is_partial = status == 206; // Partial Content
    let is_full = status == 200;    // Full Content（服务器不支持 Range）
    if !is_partial && !is_full {
        return Err(format!("HTTP 状态码: {} (url={})", status, url));
    }

    let total: Option<u64> = response.content_length().map(|cl| {
        if is_partial {
            existing_bytes + cl // 206 时 content_length 是剩余部分大小
        } else {
            cl // 200 时 content_length 是完整文件大小
        }
    }).or(expected_size);

    // 如果是 200 但有 existing_bytes，说明服务器不支持 Range，需要从头写
    let start_offset = if is_full { 0 } else { existing_bytes };

    // 如果从头开始（start_offset=0），截断文件；否则追加
    let mut file = if start_offset == 0 {
        fs::File::create(local_path).map_err(|e| format!("创建文件失败: {}", e))?
    } else {
        fs::OpenOptions::new()
            .append(true)
            .open(local_path)
            .map_err(|e| format!("打开文件失败: {}", e))?
    };

    // 如果从头开始，需要重新计算 sha256；否则只校验新增部分（但我们从头计算完整 hash）
    // 简单方案：下载完后整体校验（需要读取整个文件）
    // 优化方案：从头开始时边下载边 hash；续传时先读已有文件的 hash，然后只 hash 新增部分
    let mut hasher = Sha256::new();
    let mut downloaded: u64 = start_offset;
    let mut stream = response.bytes_stream();

    // 如果续传，先把已有文件读入 hasher
    if start_offset > 0 {
        let mut existing_file = fs::File::open(local_path).map_err(|e| format!("读取已有文件失败: {}", e))?;
        let mut buf = [0u8; 64 * 1024];
        loop {
            let n = existing_file.read(&mut buf).map_err(|e| format!("读取已有文件失败: {}", e))?;
            if n == 0 { break; }
            hasher.update(&buf[..n]);
        }
    }

    // 发初始进度
    // 若 total_all_bytes 为 0（如 HEAD 请求失败），回退到本次 HTTP 响应的 content_length
    let effective_total_bytes = if total_all_bytes > 0 { total_all_bytes } else { total.unwrap_or(0) };
    {
        let file_pct = total.map(|t| if t == 0 { 100.0 } else { (start_offset as f32 / t as f32) * 100.0 });
        let _ = emit_download_progress(app, DownloadProgressPayload {
            stage: stage.to_string(), current_file: file_label.to_string(),
            files_done, files_total, file_downloaded: start_offset, file_total: total,
            file_percent: file_pct,
            stage_percent: 0.0, overall_percent: stage_overall_percent,
            attempt, url: Some(url.to_string()),
            total_downloaded: base_bytes.saturating_add(start_offset), total_bytes: effective_total_bytes,
            speed: None,
        });
    }

    // 进度节流：每 500ms 发一次，避免频繁更新又能实时显示 MB 数
    let throttle = std::time::Duration::from_millis(500);
    let mut last_emit = std::time::Instant::now();

    loop {
        let next = match tokio::time::timeout(READ_CHUNK_TIMEOUT, stream.next()).await {
            Ok(Some(item)) => item,
            Ok(None) => break,
            Err(_) => {
                return Err(format!("读取响应超时（>{}s）: {}", READ_CHUNK_TIMEOUT.as_secs(), url));
            }
        };
        let chunk = next.map_err(|e| format!("读取 chunk 失败: {}", e))?;
        let n = chunk.len();
        if n == 0 { break; }
        file.write_all(&chunk).map_err(|e| format!("写文件失败: {}", e))?;
        hasher.update(&chunk);
        downloaded += n as u64;

        // 每 500ms 发一次进度，或文件下载完成时强制发一次
        let is_done = total.map(|t| downloaded >= t).unwrap_or(false);
        if last_emit.elapsed() >= throttle || is_done {
            last_emit = std::time::Instant::now();
            let file_pct = total.map(|t| if t == 0 { 100.0 } else { (downloaded as f32 / t as f32) * 100.0 });
            let _ = emit_download_progress(app, DownloadProgressPayload {
                stage: stage.to_string(), current_file: file_label.to_string(),
                files_done, files_total, file_downloaded: downloaded, file_total: total,
                file_percent: file_pct,
                stage_percent: 0.0, overall_percent: stage_overall_percent,
                attempt, url: Some(url.to_string()),
                total_downloaded: base_bytes.saturating_add(downloaded), total_bytes: effective_total_bytes,
            speed: None,
            });
        }
    }

    file.flush().map_err(|e| format!("flush 文件失败: {}", e))?;
    let hash = hex::encode(hasher.finalize());
    Ok((downloaded, Some(hash)))
}

/// 并发版：单文件下载 + 重试，使用共享原子计数器追踪总进度
#[allow(clippy::too_many_arguments)]
async fn download_file_with_retry_concurrent(
    app: &AppHandle,
    primary_url: &str,
    local_path: &Path,
    expected_size: Option<u64>,
    expected_sha256: Option<&str>,
    stage: &str,
    file_label: &str,
    files_done: u32,
    files_total: u32,
    stage_overall_percent: f32,
    shared_downloaded: Arc<AtomicU64>,
    total_all_bytes: u64,
) -> DownloadOutcome {
    let urls: Vec<String> = build_urls(primary_url);
    let mut last_err: Option<String> = None;

    for attempt in 1..=FILE_MAX_ATTEMPTS {
        let url = urls[(attempt - 1) % urls.len()].clone();

        match download_to_file_resume_concurrent(
            &url, local_path, expected_size, stage, file_label,
            files_done, files_total, attempt as u32, app, stage_overall_percent,
            shared_downloaded.clone(), total_all_bytes,
        ).await {
            Ok((size, sha)) => {
                if let Some(exp) = expected_sha256 {
                    if let Some(ref actual) = sha {
                        if actual.to_lowercase() != exp.to_lowercase() {
                            let _ = fs::remove_file(local_path);
                            return DownloadOutcome::Err(format!(
                                "sha256 不匹配: expected={} actual={}", exp, actual
                            ));
                        }
                    }
                }
                return DownloadOutcome::Ok { size, sha256: sha };
            }
            Err(e) => {
                eprintln!("[Downloader] {} 第 {}/{} 次失败: {}", file_label, attempt, FILE_MAX_ATTEMPTS, e);
                last_err = Some(e.clone());
                if attempt == FILE_MAX_ATTEMPTS {
                    return DownloadOutcome::Err(last_err.unwrap_or_else(|| "未知错误".into()));
                }
            }
        }
    }
    DownloadOutcome::Err(last_err.unwrap_or_else(|| "未知错误".into()))
}

/// 并发版：带断点续传的文件下载，通过 shared_downloaded 原子计数器共享进度
#[allow(clippy::too_many_arguments)]
async fn download_to_file_resume_concurrent(
    url: &str,
    local_path: &Path,
    expected_size: Option<u64>,
    stage: &str,
    file_label: &str,
    files_done: u32,
    files_total: u32,
    attempt: u32,
    app: &AppHandle,
    stage_overall_percent: f32,
    shared_downloaded: Arc<AtomicU64>,
    total_all_bytes: u64,
) -> Result<(u64, Option<String>), String> {
    if let Some(parent) = local_path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("创建目录失败: {}", e))?;
    }

    let existing_bytes: u64 = fs::metadata(local_path).map(|m| m.len()).unwrap_or(0);

    let client = reqwest::Client::builder()
        .user_agent("jjxf_launcher/0.1.0")
        .connect_timeout(CONNECT_TIMEOUT)
        .build()
        .map_err(|e| format!("构建 HTTP 客户端失败: {}", e))?;

    let mut request = client.get(url);
    if existing_bytes > 0 {
        if let Some(total) = expected_size {
            if existing_bytes >= total {
                let hash = compute_file_sha256(local_path).ok();
                return Ok((existing_bytes, hash));
            }
        }
        request = request.header("Range", format!("bytes={}-", existing_bytes));
    }

    let response = request.send().await.map_err(|e| format!("请求失败: {}", e))?;

    let status = response.status();
    let is_partial = status == 206;
    let is_full = status == 200;
    if !is_partial && !is_full {
        return Err(format!("HTTP 状态码: {} (url={})", status, url));
    }

    let total: Option<u64> = response.content_length().map(|cl| {
        if is_partial { existing_bytes + cl } else { cl }
    }).or(expected_size);

    let effective_total_bytes = if total_all_bytes > 0 { total_all_bytes } else { total.unwrap_or(0) };
    let start_offset = if is_full { 0 } else { existing_bytes };

    let mut file = if start_offset == 0 {
        fs::File::create(local_path).map_err(|e| format!("创建文件失败: {}", e))?
    } else {
        fs::OpenOptions::new()
            .append(true)
            .open(local_path)
            .map_err(|e| format!("打开文件失败: {}", e))?
    };

    let mut hasher = Sha256::new();
    let mut file_downloaded: u64 = start_offset;
    let mut stream = response.bytes_stream();

    if start_offset > 0 {
        let mut existing_file = fs::File::open(local_path).map_err(|e| format!("读取已有文件失败: {}", e))?;
        let mut buf = [0u8; 64 * 1024];
        loop {
            let n = existing_file.read(&mut buf).map_err(|e| format!("读取已有文件失败: {}", e))?;
            if n == 0 { break; }
            hasher.update(&buf[..n]);
        }
    }

    // 发初始进度
    {
        let total_dl = shared_downloaded.load(Ordering::Relaxed);
        let file_pct = total.map(|t| if t == 0 { 100.0 } else { (start_offset as f32 / t as f32) * 100.0 });
        let _ = emit_download_progress(app, DownloadProgressPayload {
            stage: stage.to_string(), current_file: file_label.to_string(),
            files_done, files_total, file_downloaded: start_offset, file_total: total,
            file_percent: file_pct,
            stage_percent: 0.0, overall_percent: stage_overall_percent,
            attempt, url: Some(url.to_string()),
            total_downloaded: total_dl, total_bytes: effective_total_bytes,
            speed: None,
        });
    }

    let throttle = std::time::Duration::from_millis(500);
    let mut last_emit = std::time::Instant::now();

    loop {
        let next = match tokio::time::timeout(READ_CHUNK_TIMEOUT, stream.next()).await {
            Ok(Some(item)) => item,
            Ok(None) => break,
            Err(_) => return Err(format!("读取响应超时（>{}s）: {}", READ_CHUNK_TIMEOUT.as_secs(), url)),
        };
        let chunk = next.map_err(|e| format!("读取 chunk 失败: {}", e))?;
        let n = chunk.len();
        if n == 0 { break; }
        file.write_all(&chunk).map_err(|e| format!("写文件失败: {}", e))?;
        hasher.update(&chunk);
        file_downloaded += n as u64;
        // 更新共享计数器
        shared_downloaded.fetch_add(n as u64, Ordering::Relaxed);

        let is_done = total.map(|t| file_downloaded >= t).unwrap_or(false);
        if last_emit.elapsed() >= throttle || is_done {
            last_emit = std::time::Instant::now();
            let total_dl = shared_downloaded.load(Ordering::Relaxed);
            let file_pct = total.map(|t| if t == 0 { 100.0 } else { (file_downloaded as f32 / t as f32) * 100.0 });
            let _ = emit_download_progress(app, DownloadProgressPayload {
                stage: stage.to_string(), current_file: file_label.to_string(),
                files_done, files_total, file_downloaded, file_total: total,
                file_percent: file_pct,
                stage_percent: 0.0, overall_percent: stage_overall_percent,
                attempt, url: Some(url.to_string()),
                total_downloaded: total_dl, total_bytes: effective_total_bytes,
            speed: None,
            });
        }
    }

    file.flush().map_err(|e| format!("flush 文件失败: {}", e))?;
    let hash = hex::encode(hasher.finalize());
    Ok((file_downloaded, Some(hash)))
}

/// ============== IDM 风格多线程分片下载 ==============
///
/// 核心设计：
///   - 启动 N 个工作线程从共享任务队列中拉取分片
///   - 队列初始填充 K 个分片（K > N，保证某个线程抽完时其他线程还有活干）
///   - 某个分片拉取失败 → 不是简单退出，而是增加其失败计数后重新放回队列
///   - 连续失败达到 CHUNK_MAX_CONSECUTIVE_FAILS → 才认为该分片彻底失败
///   - 工作线程随时拉取到任何一个未完成分片，保证所有线程都在干活

/// 分片任务描述
#[derive(Debug, Clone)]
struct ChunkRange {
    index: u32,
    start: u64,
    end: u64, // 包含
}

/// 分片状态
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ChunkStatus {
    Pending,    // 待下载
    Downloading,// 下载中
    Done,       // 完成
    Failed,     // 连续失败超过阈值，彻底失败
}

/// 分片任务
#[derive(Debug, Clone)]
struct ChunkTask {
    index: u32,
    start: u64,
    end: u64,
    status: ChunkStatus,
    fail_count: u32,
}

/// 计算分片计划：IDM 风格
/// - 默认分为 FILE_CHUNK_CONCURRENCY * 2 块（保证工作线程不会饿）
/// - 但每块不小于 CHUNK_MIN_SIZE
fn plan_chunks(total_size: u64, concurrency: usize) -> Vec<ChunkRange> {
    let mut ranges = Vec::new();
    if total_size == 0 {
        return ranges;
    }
    // 分片数 = concurrency * 2，让任务总数超过线程数保证充分调度
    let n = (concurrency * 2).max(1) as u64;
    let mut chunk_size = total_size / n;
    if chunk_size < CHUNK_MIN_SIZE {
        chunk_size = CHUNK_MIN_SIZE.max(1);
    }
    let mut start: u64 = 0;
    let mut idx: u32 = 0;
    while start < total_size {
        let end = (start + chunk_size - 1).min(total_size - 1);
        ranges.push(ChunkRange { index: idx, start, end });
        idx += 1;
        start = end + 1;
    }
    ranges
}

/// IDM 风格下载：多个工作线程从共享队列中拉取分片
/// - 任务被加锁索引访问（不是一锁护全表）
/// - 失败后重试，趋近 CHUNK_MAX_CONSECUTIVE_FAILS 才放弃该分片
#[allow(clippy::too_many_arguments)]
async fn download_file_with_chunks(
    primary_url: &str,
    local_path: &Path,
    expected_size: u64,
    shared_downloaded: Arc<AtomicU64>,
    stats: Arc<ThreadStats>,
) -> Result<(u64, Option<String>), String> {
    if let Some(parent) = local_path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("创建目录失败: {}", e))?;
    }
    let temp_path = local_path.with_extension(
        local_path
            .extension()
            .map(|e| format!("{}.part", e.to_string_lossy()))
            .unwrap_or_else(|| "part".into()),
    );

    // 先删除可能存在的临时文件
    let _ = fs::remove_file(&temp_path);

    // 预创建并预分配临时文件
    {
        let f = fs::File::create(&temp_path).map_err(|e| format!("创建临时文件失败: {}", e))?;
        f.set_len(expected_size)
            .map_err(|e| format!("预分配文件失败: {}", e))?;
    }

    let chunks = plan_chunks(expected_size, FILE_CHUNK_CONCURRENCY);
    stats.reset(chunks.len() as u32);
    if chunks.is_empty() {
        // 空文件处理
        let _ = fs::remove_file(&temp_path);
        fs::File::create(local_path).map_err(|e| format!("创建文件失败: {}", e))?;
        return Ok((0, Some(compute_file_sha256(local_path).unwrap_or_default())));
    }

    // 共享任务队列（受 Mutex 保护）
    let tasks: Arc<tokio::sync::Mutex<Vec<ChunkTask>>> = Arc::new(tokio::sync::Mutex::new(
        chunks
            .iter()
            .map(|c| ChunkTask {
                index: c.index,
                start: c.start,
                end: c.end,
                status: ChunkStatus::Pending,
                fail_count: 0,
            })
            .collect(),
    ));
    // 任务状态变更通知：工人完成或新增分片时通知
    let notify = Arc::new(tokio::sync::Notify::new());
    // 是否全部完成
    let all_done = Arc::new(std::sync::atomic::AtomicBool::new(false));

    // 创建 HTTP 客户端池
    let client = reqwest::Client::builder()
        .user_agent("jjxf_launcher/0.1.0")
        .connect_timeout(CONNECT_TIMEOUT)
        .build()
        .map_err(|e| format!("构建 HTTP 客户端失败: {}", e))?;

    // 启动 N 个工作线程
    let mut workers = Vec::new();
    for worker_id in 0..FILE_CHUNK_CONCURRENCY {
        let tasks = tasks.clone();
        let notify = notify.clone();
        let all_done = all_done.clone();
        let temp_path = temp_path.clone();
        let url = primary_url.to_string();
        let client = client.clone();
        let shared_downloaded = shared_downloaded.clone();
        let stats = stats.clone();
        let worker = tokio::spawn(async move {
            loop {
                if all_done.load(Ordering::Relaxed) {
                    break;
                }

                // 从队列中取一个 Pending 的任务
                let task_opt = {
                    let mut guard = tasks.lock().await;
                    let pos = guard.iter().position(|t| t.status == ChunkStatus::Pending);
                    if let Some(p) = pos {
                        guard[p].status = ChunkStatus::Downloading;
                        Some(guard[p].clone())
                    } else {
                        // 没有可拉取的任务：检查是否真的全部完成
                        let any_active = guard
                            .iter()
                            .any(|t| matches!(t.status, ChunkStatus::Downloading));
                        if !any_active {
                            all_done.store(true, Ordering::Relaxed);
                            notify.notify_waiters();
                            break;
                        }
                        None
                    }
                };

                let task = match task_opt {
                    Some(t) => t,
                    None => {
                        // 没有 Pending 且还有人在跑，等一下
                        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                        continue;
                    }
                };

                // 拉取该分片
                let res = pull_chunk(
                    worker_id,
                    &client,
                    &url,
                    &temp_path,
                    task.start,
                    task.end,
                    shared_downloaded.clone(),
                    stats.clone(),
                )
                .await;

                // 根据结果更新任务状态
                {
                    let mut guard = tasks.lock().await;
                    if let Some(t) = guard.iter_mut().find(|t| t.index == task.index) {
                        match res {
                            Ok(()) => {
                                t.status = ChunkStatus::Done;
                            }
                            Err(e) => {
                                t.fail_count += 1;
                                stats.inc_failed();
                                eprintln!(
                                    "[Downloader] worker={} chunk={} 第 {}/{} 次失败: {}",
                                    worker_id, task.index, t.fail_count, CHUNK_MAX_CONSECUTIVE_FAILS, e
                                );
                                if t.fail_count >= CHUNK_MAX_CONSECUTIVE_FAILS {
                                    t.status = ChunkStatus::Failed;
                                } else {
                                    // 退避：200ms, 400ms, 800ms, 1.6s
                                    let backoff_ms = CHUNK_RETRY_BASE_MS
                                        * (1u64 << (t.fail_count - 1).min(10));
                                    tokio::time::sleep(std::time::Duration::from_millis(
                                        backoff_ms,
                                    ))
                                    .await;
                                    t.status = ChunkStatus::Pending; // 重新入队
                                }
                            }
                        }
                    }
                }

                // 唤醒可能在等待的其他人
                notify.notify_one();
            }
        });
        workers.push(worker);
    }

    // 等待所有工人退出
    for w in workers {
        let _ = w.await;
    }

    // 验证：检查是否所有分片都 Done
    {
        let guard = tasks.lock().await;
        let failed: Vec<u32> = guard
            .iter()
            .filter(|t| t.status == ChunkStatus::Failed)
            .map(|t| t.index)
            .collect();
        if !failed.is_empty() {
            // 删除临时文件
            let _ = fs::remove_file(&temp_path);
            return Err(format!(
                "{} 个分片连续失败超阈值：{:?}",
                failed.len(),
                failed
            ));
        }
    }

    // sha256 校验
    let hash = compute_file_sha256(&temp_path).ok();

    // 重命名为正式文件
    fs::rename(&temp_path, local_path).map_err(|e| format!("重命名临时文件失败: {}", e))?;

    Ok((expected_size, hash))
}

/// 拉取一个分片：仅下载一次（重试由工作线程外层控制）
/// stats 在 request/receive 各阶段递增递减
/// 失败时自动回退 shared_downloaded 和 stats.last_bytes 中已累加的字节，
/// 确保外层重试不会导致已下载超出总大小。
async fn pull_chunk(
    worker_id: usize,
    client: &reqwest::Client,
    url: &str,
    temp_path: &Path,
    start: u64,
    end: u64,
    shared_downloaded: Arc<AtomicU64>,
    stats: Arc<ThreadStats>,
) -> Result<(), String> {
    stats.inc_requesting();
    let request = client.get(url).header("Range", format!("bytes={}-{}", start, end));
    let send_result = request.send().await;
    stats.dec_requesting();
    let response = send_result.map_err(|e| format!("分片请求失败: {}", e))?;
    let status = response.status();
    if status != 206 && status != 200 {
        return Err(format!("分片 HTTP 状态码: {}", status));
    }

    // 打开文件并定位到 start
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create(true)
        .open(temp_path)
        .map_err(|e| format!("打开分片文件失败: {}", e))?;
    file.seek(std::io::SeekFrom::Start(start))
        .map_err(|e| format!("seek 失败: {}", e))?;

    let mut stream = response.bytes_stream();
    let expected_len = end - start + 1;
    let mut received: u64 = 0;
    let mut receiving = false;
    loop {
        let next = match tokio::time::timeout(READ_CHUNK_TIMEOUT, stream.next()).await {
            Ok(Some(item)) => item,
            Ok(None) => break,
            Err(_) => {
                // 超时：回退已累加的字节
                let err_msg = format!(
                    "分片读取超时 (worker={}, bytes={}-{})",
                    worker_id, start, end
                );
                rollback_chunk_bytes(&shared_downloaded, &stats, received, receiving);
                return Err(err_msg);
            }
        };
        let chunk_data = match next {
            Ok(data) => data,
            Err(e) => {
                let err_msg = format!("分片读取 chunk 失败: {}", e);
                rollback_chunk_bytes(&shared_downloaded, &stats, received, receiving);
                return Err(err_msg);
            }
        };
        let n = chunk_data.len();
        if n == 0 { break; }
        if !receiving {
            receiving = true;
            stats.inc_receiving();
        }
        if let Err(e) = file.write_all(&chunk_data) {
            let err_msg = format!("分片写文件失败: {}", e);
            rollback_chunk_bytes(&shared_downloaded, &stats, received, receiving);
            return Err(err_msg);
        }
        received += n as u64;
        shared_downloaded.fetch_add(n as u64, Ordering::Relaxed);
        // 同步累加到 stats.last_bytes，让 monitor 能读到当前进度
        stats.last_bytes.fetch_add(n as u64, Ordering::Relaxed);
    }

    if receiving {
        stats.dec_receiving();
    }

    if received != expected_len {
        let err_msg = format!(
            "分片长度不符: 期望 {} 实际 {}",
            expected_len, received
        );
        rollback_chunk_bytes(&shared_downloaded, &stats, received, receiving);
        return Err(err_msg);
    }
    Ok(())
}

/// 分片失败时回退 shared_downloaded 和 stats.last_bytes 中本分片已累加的字节
fn rollback_chunk_bytes(
    shared_downloaded: &AtomicU64,
    stats: &ThreadStats,
    received: u64,
    was_receiving: bool,
) {
    if received > 0 {
        shared_downloaded.fetch_sub(received, Ordering::Relaxed);
        stats.last_bytes.fetch_sub(received, Ordering::Relaxed);
    }
    if was_receiving {
        stats.dec_receiving();
    }
}



/// 判断是否使用多线程分片
fn should_use_chunks(expected_size: u64) -> bool {
    expected_size >= CHUNK_MIN_FILE_SIZE
}

/// 分片下载包装：含重试 + 主备域名轮换 + sha256 校验 + 进度节流发送。
/// 调用前需确保 expected_size 已知（用于预分配临时文件）。
#[allow(clippy::too_many_arguments)]
async fn download_file_with_retry_chunks(
    app: &AppHandle,
    primary_url: &str,
    local_path: &Path,
    expected_size: u64,
    expected_sha256: Option<&str>,
    stage: &str,
    file_label: &str,
    files_done: u32,
    files_total: u32,
    stage_overall_percent: f32,
    shared_downloaded: Arc<AtomicU64>,
    total_all_bytes: u64,
) -> DownloadOutcome {
    let urls: Vec<String> = build_urls(primary_url);
    let mut last_err: Option<String> = None;
    let effective_total_bytes = if total_all_bytes > 0 { total_all_bytes } else { expected_size };
    let throttle = std::time::Duration::from_millis(500);
    let stats = Arc::new(ThreadStats::new());

    for attempt in 1..=FILE_MAX_ATTEMPTS {
        let url = urls[(attempt - 1) % urls.len()].clone();

        // 记录本次尝试前的 shared_downloaded 快照，失败时恢复到该值
        let snapshot_before = shared_downloaded.load(Ordering::Relaxed);

        // 发初始进度
        let total_dl = shared_downloaded.load(Ordering::Relaxed);
        let _ = emit_download_progress(app, DownloadProgressPayload {
            stage: stage.to_string(), current_file: file_label.to_string(),
            files_done, files_total, file_downloaded: 0, file_total: Some(expected_size),
            file_percent: Some(0.0),
            stage_percent: 0.0, overall_percent: stage_overall_percent,
            attempt: attempt as u32, url: Some(url.clone()),
            total_downloaded: total_dl, total_bytes: effective_total_bytes,
            speed: None,
        });

        // 在后台启动一个 500ms 节流的进度推送任务
        let (stop_tx, mut stop_rx) = tokio::sync::oneshot::channel::<()>();
        let app_clone = app.clone();
        let stage_s = stage.to_string();
        let label_s = file_label.to_string();
        let url_s = url.clone();
        let sd_clone = shared_downloaded.clone();
        let attempt_u32 = attempt as u32;
        let pumper = tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = tokio::time::sleep(throttle) => {
                        let total_dl = sd_clone.load(Ordering::Relaxed);
                        let _ = emit_download_progress(&app_clone, DownloadProgressPayload {
                            stage: stage_s.clone(), current_file: label_s.clone(),
                            files_done, files_total, file_downloaded: 0, file_total: Some(expected_size),
                            file_percent: None,
                            stage_percent: 0.0, overall_percent: stage_overall_percent,
                            attempt: attempt_u32, url: Some(url_s.clone()),
                            total_downloaded: total_dl, total_bytes: effective_total_bytes,
            speed: None,
                        });
                    }
                    _ = &mut stop_rx => { break; }
                }
            }
        });

        // 启动线程状态监控（每 1 秒打印后端控制台）
        let monitor_stop = spawn_stats_monitor(stats.clone(), file_label.to_string());

        let result = download_file_with_chunks(
            &url, local_path, expected_size, shared_downloaded.clone(), stats.clone()
        ).await;

        // 停止监控和进度推送
        stats.finish();
        let _ = monitor_stop.send(());
        let _ = stop_tx.send(());
        let _ = pumper.await;

        match result {
            Ok((size, _sha)) => {
                // 下载阶段不校验 sha256（留到资源校验阶段统一跑），
                // 这里只检查文件大小是否正确
                if size != expected_size {
                    // 失败：恢复 shared_downloaded 到本次尝试前的值
                    shared_downloaded.store(snapshot_before, Ordering::Relaxed);
                    let _ = fs::remove_file(local_path);
                    eprintln!(
                        "[Downloader] {} 大小不符: expected={} actual={}",
                        file_label, expected_size, size
                    );
                    return DownloadOutcome::Err(format!(
                        "下载大小不符: expected={} actual={}",
                        expected_size, size
                    ));
                }
                // 最后一次 100% 进度
                let total_dl = shared_downloaded.load(Ordering::Relaxed);
                let _ = emit_download_progress(app, DownloadProgressPayload {
                    stage: stage.to_string(), current_file: file_label.to_string(),
                    files_done, files_total, file_downloaded: size, file_total: Some(size),
                    file_percent: Some(100.0),
                    stage_percent: 0.0, overall_percent: stage_overall_percent,
                    attempt: attempt as u32, url: None,
                    total_downloaded: total_dl, total_bytes: effective_total_bytes,
            speed: None,
                });
                return DownloadOutcome::Ok { size, sha256: _sha };
            }

            Err(e) => {
                // 恢复 shared_downloaded 到本次尝试前的值
                shared_downloaded.store(snapshot_before, Ordering::Relaxed);
                eprintln!("[Downloader] {} 第 {}/{} 次失败: {}", file_label, attempt, FILE_MAX_ATTEMPTS, e);
                stats.inc_failed();
                last_err = Some(e.clone());
                if attempt == FILE_MAX_ATTEMPTS {
                    return DownloadOutcome::Err(last_err.unwrap_or_else(|| "未知错误".into()));
                }
            }
        }
    }
    DownloadOutcome::Err(last_err.unwrap_or_else(|| "未知错误".into()))
}


/// 主域名 -> 备用域名 URL 列表
fn build_urls(primary_path: &str) -> Vec<String> {
    if let Some(stripped) = primary_path.strip_prefix(PRIMARY_HOST) {
        vec![
            primary_path.to_string(),
            format!("{}{}", BACKUP_HOST, stripped),
        ]
    } else {
        vec![primary_path.to_string()]
    }
}

// ============== 单独提供：只校验文件是否完整 ==============

#[tauri::command]
pub async fn verify_local_files(app: AppHandle) -> Result<DownloadDonePayload, String> {
    verify_local_files_internal(&app).await
}

async fn verify_local_files_internal(app: &AppHandle) -> Result<DownloadDonePayload, String> {
    let local_manifest_path = game_dir()?.join("manifest.json");
    let content = fs::read_to_string(&local_manifest_path)
        .map_err(|e| format!("读取本地 manifest.json 失败: {}", e))?;
    let manifest: ManifestFile = serde_json::from_str(&content)
        .map_err(|e| format!("解析本地 manifest.json 失败: {}", e))?;

    let total_files = manifest.packs.len() + 1;
    let mut done: u32 = 0;
    let mut failed: Vec<String> = Vec::new();
    let packs_total: u64 = manifest.packs.values().map(|p| p.size).sum();
    let exe_path = game_exe_path()?;
    let exe_size: u64 = fs::metadata(&exe_path).map(|m| m.len()).unwrap_or(0);
    let total_bytes: u64 = exe_size.saturating_add(packs_total);

    // 发初始进度
    let _ = emit_download_progress(app, DownloadProgressPayload {
        stage: "verify".into(), current_file: "game.exe".into(),
        files_done: 0, files_total: total_files as u32,
        file_downloaded: 0, file_total: Some(0), file_percent: Some(0.0),
        stage_percent: 0.0, overall_percent: 0.0,
        attempt: 1, url: None,
        total_downloaded: 0, total_bytes, speed: None,
    });

    // exe — 分块 SHA256 + 字节级进度
    if exe_path.exists() {
        let hash = compute_file_sha256_with_progress(
            app, &exe_path, "game.exe", exe_size, 0, total_bytes,
        );
        let _ = verify_state::upsert_exe(FileVerifyRecord {
            sha256: hash.clone().unwrap_or_default(), size: exe_size, status: "ok".into(),
        });
        emit_download_file_done(app, DownloadFileDonePayload {
            stage: "verify".into(), file: "game.exe".into(), ok: true,
            verify_status: "ok".into(), sha256: hash, size: exe_size,
        });
        done += 1;
        let _ = emit_download_progress(app, DownloadProgressPayload {
            stage: "verify".into(), current_file: "game.exe".into(),
            files_done: done, files_total: total_files as u32,
            file_downloaded: exe_size, file_total: Some(exe_size), file_percent: Some(100.0),
            stage_percent: 0.0, overall_percent: 0.0,
            attempt: 1, url: None,
            total_downloaded: exe_size, total_bytes, speed: None,
        });
    } else {
        let _ = verify_state::upsert_exe(FileVerifyRecord {
            sha256: String::new(), size: 0, status: "missing".into(),
        });
        failed.push("game.exe".into());
        emit_download_file_done(app, DownloadFileDonePayload {
            stage: "verify".into(), file: "game.exe".into(), ok: false,
            verify_status: "missing".into(), sha256: None, size: 0,
        });
        done += 1;
    }
    tokio::task::yield_now().await;

    // packs — 复用 verify_pack_with_progress 得分块 SHA256 + 字节级进度
    // verified_bytes 从 exe 大小开始，total_bytes 已包含 exe，保证进度条连续不跳变
    let mut verified_bytes: u64 = exe_size;
    for (name, info) in manifest.packs.iter() {
        let path = pack_file_path(name).unwrap();
        let file_label = format!("{}{}", name, PACK_FILE_EXT);
        let p = PackEntry {
            name: name.clone(),
            url: String::new(),
            local_path: path.clone(),
            expected_sha256: info.sha256.to_lowercase(),
            expected_size: info.size,
        };
        if !path.exists() {
            let _ = verify_state::upsert_pack(name, FileVerifyRecord {
                sha256: info.sha256.clone(), size: info.size, status: "missing".into(),
            });
            failed.push(file_label.clone());
            emit_download_file_done(app, DownloadFileDonePayload {
                stage: "verify".into(), file: file_label.clone(), ok: false,
                verify_status: "missing".into(), sha256: None, size: 0,
            });
            done += 1;
            verified_bytes = verified_bytes.saturating_add(info.size);
            let _ = emit_download_progress(app, DownloadProgressPayload {
                stage: "verify".into(), current_file: file_label,
                files_done: done, files_total: total_files as u32,
                file_downloaded: 0, file_total: Some(0), file_percent: Some(100.0),
                stage_percent: 0.0, overall_percent: 0.0,
                attempt: 1, url: None,
                total_downloaded: verified_bytes, total_bytes, speed: None,
            });
        } else {
            let (ok, actual_hash) = verify_pack_with_progress(
                app, &p, &file_label, info.size, &mut verified_bytes, total_bytes,
            );
            let status = if ok { "ok" } else { "mismatch" };
            let _ = verify_state::upsert_pack(name, FileVerifyRecord {
                sha256: actual_hash.clone().unwrap_or_default(), size: info.size, status: status.into(),
            });
            if ok {
                emit_download_file_done(app, DownloadFileDonePayload {
                    stage: "verify".into(), file: file_label, ok: true,
                    verify_status: "ok".into(), sha256: actual_hash, size: info.size,
                });
            } else {
                failed.push(file_label.clone());
                emit_download_file_done(app, DownloadFileDonePayload {
                    stage: "verify".into(), file: file_label, ok: false,
                    verify_status: "mismatch".into(), sha256: actual_hash, size: info.size,
                });
            }
            done += 1;
        }
        tokio::task::yield_now().await;
    }

    if let Some(v) = manifest.version.as_ref() {
        let packs_path = manifest
            .path
            .as_deref()
            .unwrap_or(DEFAULT_PACKS_PATH);
        let _ = verify_state::set_manifest_version_and_path(v, packs_path);
    }

    Ok(DownloadDonePayload {
        ok: failed.is_empty(),
        message: if failed.is_empty() { "校验通过".into() } else { format!("有 {} 个文件需要重新下载", failed.len()) },
        failed_files: failed,
    })
}

fn percent_of_u64_local(a: u64, total: u64) -> f32 {
    if total == 0 { 100.0 } else { (a as f32 / total as f32) * 100.0 }
}

/// 资源校验阶段：读 pck 算 sha256，同时发送字节级进度
/// 返回 (是否与预期 sha256 匹配, 实际 sha256)
fn verify_pack_with_progress(
    app: &AppHandle,
    p: &PackEntry,
    file_label: &str,
    file_total: u64,
    verified_bytes: &mut u64,
    total_verify_bytes: u64,
) -> (bool, Option<String>) {
    let mut file = match fs::File::open(&p.local_path) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("[Verify] 打开 {} 失败: {}", file_label, e);
            return (false, None);
        }
    };
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 128 * 1024];
    let mut last_emit = std::time::Instant::now();
    let throttle = std::time::Duration::from_millis(300);
    loop {
        let n = match file.read(&mut buf) {
            Ok(n) => n,
            Err(e) => {
                eprintln!("[Verify] 读 {} 失败: {}", file_label, e);
                return (false, None);
            }
        };
        if n == 0 { break; }
        hasher.update(&buf[..n]);
        *verified_bytes += n as u64;
        // 每 300ms 发一次进度
        if last_emit.elapsed() >= throttle {
            last_emit = std::time::Instant::now();
            let _ = emit_download_progress(app, DownloadProgressPayload {
                stage: "verify".into(),
                current_file: file_label.to_string(),
                files_done: 0, files_total: 0,
                file_downloaded: *verified_bytes as u64, file_total: Some(file_total),
                file_percent: Some(if file_total > 0 { (*verified_bytes as f32 / file_total as f32) * 100.0 } else { 0.0 }),
                stage_percent: 0.0, overall_percent: 0.0,
                attempt: 1, url: None,
                total_downloaded: *verified_bytes as u64, total_bytes: total_verify_bytes,
            speed: None,
            });
        }
    }
    let actual = hex::encode(hasher.finalize());
    let ok = actual.to_lowercase() == p.expected_sha256.to_lowercase();
    (ok, Some(actual))
}


fn compute_file_sha256(path: &Path) -> Result<String, String> {
    let mut file = fs::File::open(path).map_err(|e| format!("打开文件失败: {}", e))?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = file.read(&mut buf).map_err(|e| format!("读取文件失败: {}", e))?;
        if n == 0 { break; }
        hasher.update(&buf[..n]);
    }
    Ok(hex::encode(hasher.finalize()))
}

/// 分块 SHA256 计算 + 字节级进度发射（供手动校验使用）
/// 与 verify_pack_with_progress 类似但直接读本地文件并逐文件上报进度
fn compute_file_sha256_with_progress(
    app: &AppHandle,
    path: &Path,
    file_label: &str,
    file_total: u64,
    verified_bytes_before: u64,
    total_verify_bytes: u64,
) -> Option<String> {
    let mut file = match fs::File::open(path) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("[Verify] 打开 {} 失败: {}", file_label, e);
            return None;
        }
    };
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 128 * 1024];
    let mut read = 0u64;
    let mut last_emit = std::time::Instant::now();
    let throttle = std::time::Duration::from_millis(300);
    loop {
        let n = match file.read(&mut buf) {
            Ok(n) => n,
            Err(e) => {
                eprintln!("[Verify] 读 {} 失败: {}", file_label, e);
                return None;
            }
        };
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
        read += n as u64;
        if last_emit.elapsed() >= throttle {
            last_emit = std::time::Instant::now();
            let total_dl = verified_bytes_before + read;
            let _ = emit_download_progress(app, DownloadProgressPayload {
                stage: "verify".into(),
                current_file: file_label.to_string(),
                files_done: 0,
                files_total: 0,
                file_downloaded: read,
                file_total: Some(file_total),
                file_percent: Some(if file_total > 0 {
                    (read as f32 / file_total as f32) * 100.0
                } else {
                    0.0
                }),
                stage_percent: 0.0,
                overall_percent: 0.0,
                attempt: 1,
                url: None,
                total_downloaded: total_dl,
                total_bytes: total_verify_bytes,
            speed: None,
            });
        }
    }
    Some(hex::encode(hasher.finalize()))
}

/// 给前端读取 verify.json 的轻量 command
#[derive(serde::Serialize)]
pub struct VerifyStateInfo {
    pub path: String,
    pub exe_ok: bool,
    pub packs: std::collections::BTreeMap<String, String>,
    pub manifest_version: Option<String>,
    pub exists: bool,
    pub pack_names: Vec<String>,
}

#[tauri::command]
pub fn read_verify_state() -> Result<VerifyStateInfo, String> {
    let state = verify_state::load();
    let path = crate::paths::verify_json_path()?;
    let pack_names = crate::verify_state::read_local_pack_names()?;
    let packs: std::collections::BTreeMap<String, String> = state.packs.iter()
        .map(|(k, v)| (k.clone(), v.status.clone())).collect();
    Ok(VerifyStateInfo {
        path: path.to_string_lossy().to_string(),
        exe_ok: verify_state::is_exe_ok(&state), packs,
        manifest_version: state.manifest_version.clone(),
        exists: path.exists(), pack_names,
    })
}

/// 检查是否需要更新：比较服务端 manifest 与本地 verify.json 的 sha256/path
/// 返回需要更新的文件列表（即使 version 相同，sha256 或 path 不同也需要更新）
#[derive(serde::Serialize)]
pub struct UpdateCheckResult {
    pub needs_update: bool,
    /// 需要更新的文件列表："game.exe" 或 "Arts.pck" 等
    pub outdated_files: Vec<String>,
    /// 服务端版本号
    pub server_version: Option<String>,
    /// 本地版本号
    pub local_version: Option<String>,
    /// manifest path 是否变更（变更时需要重新下载所有文件）
    pub path_changed: bool,
}

#[tauri::command]
pub fn check_update_needed(server_manifest_content: String) -> Result<UpdateCheckResult, String> {
    let manifest: ManifestFile = serde_json::from_str(&server_manifest_content)
        .map_err(|e| format!("解析服务端 manifest 失败: {}", e))?;

    let verify = verify_state::load();
    let packs_path = manifest.path.as_deref().unwrap_or(DEFAULT_PACKS_PATH);

    let path_changed = !verify_state::is_manifest_path_match(&verify, packs_path);
    let mut outdated_files: Vec<String> = Vec::new();

    // 检查 exe
    if let Some(ref exe_info) = manifest.exe {
        if !verify_state::is_exe_sha256_match(&verify, &exe_info.sha256) || path_changed {
            outdated_files.push("game.exe".to_string());
        }
    } else if !verify_state::is_exe_ok(&verify) {
        outdated_files.push("game.exe".to_string());
    }

    // 检查 packs
    for (name, info) in &manifest.packs {
        let expected_sha256 = info.sha256.to_lowercase();
        if path_changed || !verify_state::is_pack_sha256_match(&verify, name, &expected_sha256) {
            outdated_files.push(format!("{}{}", name, PACK_FILE_EXT));
        }
    }

    // 如果 manifest path 变更，也检查之前 verify.json 中的 packs 是否存在但 manifest 中没有的
    // （这种情况一般不会发生，但如果发生，path_changed 已经处理了重下所有文件）

    Ok(UpdateCheckResult {
        needs_update: !outdated_files.is_empty(),
        outdated_files,
        server_version: manifest.version.clone(),
        local_version: verify.manifest_version.clone(),
        path_changed,
    })
}

/// 清理 verify.json
#[tauri::command]
pub fn clear_verify_state() -> Result<(), String> {
    let path = crate::paths::verify_json_path()?;
    if path.exists() {
        fs::remove_file(&path).map_err(|e| format!("删除 verify.json 失败: {}", e))?;
    }
    Ok(())
}