// ============== verify.json 状态管理 ==============
// 每次下载/校验完一个文件，都把结果写入 <game>/verify.json。
// 下次启动器打开时只要读这个文件就能：
//   1) 知道哪些文件已经下载好了
//   2) 知道哪些文件上次校验失败需要重下
//   3) 决定按钮文案是「下载/继续/更新/开始游戏」

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::paths::verify_json_path;

/// 单个文件的校验记录
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FileVerifyRecord {
    /// 服务端给出的 sha256
    pub sha256: String,
    /// 文件字节数
    pub size: u64,
    /// 校验状态：
    ///   "ok"       - 下载成功且 sha256 匹配
    ///   "mismatch" - 文件存在但 sha256 不匹配（需重下）
    ///   "missing"  - 文件丢失（需重下）
    ///   "pending"  - 下载未完成
    pub status: String,
}

/// 整个 verify.json 的结构
/// 字段命名上使用稳定 key，方便外部脚本/调试查阅
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct VerifyState {
    /// 服务端 manifest 版本号
    #[serde(default)]
    pub manifest_version: Option<String>,
    /// 服务端 manifest 中的 path 字段（资源所在子目录，如 "0.8.2.9_6_13"）
    #[serde(default)]
    pub manifest_path: Option<String>,
    /// 资源包相对路径 -> 记录（"Arts" / "Json" / ...）
    #[serde(default)]
    pub packs: BTreeMap<String, FileVerifyRecord>,
    /// 主程序记录
    #[serde(default)]
    pub exe: Option<FileVerifyRecord>,
    /// 上次写入时间（RFC3339 字符串）
    #[serde(default)]
    pub updated_at: Option<String>,
}

/// 读取 verify.json（不存在或损坏时返回空状态）
pub fn load() -> VerifyState {
    let path = match verify_json_path() {
        Ok(p) => p,
        Err(_) => return VerifyState::default(),
    };
    if !path.exists() {
        return VerifyState::default();
    }
    match fs::read_to_string(&path) {
        Ok(s) => serde_json::from_str(&s).unwrap_or_default(),
        Err(_) => VerifyState::default(),
    }
}

/// 把整个 VerifyState 写回 verify.json
pub fn save(state: &VerifyState) -> Result<(), String> {
    let path = verify_json_path()?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("创建 game 目录失败: {}", e))?;
    }
    let json = serde_json::to_string_pretty(state)
        .map_err(|e| format!("序列化 verify.json 失败: {}", e))?;
    fs::write(&path, json).map_err(|e| format!("写入 verify.json 失败: {}", e))?;
    Ok(())
}

/// 更新单个 pack 记录并立即落盘
pub fn upsert_pack(name: &str, record: FileVerifyRecord) -> Result<(), String> {
    let mut state = load();
    state.packs.insert(name.to_string(), record);
    state.updated_at = Some(current_rfc3339());
    save(&state)
}

/// 更新 exe 记录并立即落盘
pub fn upsert_exe(record: FileVerifyRecord) -> Result<(), String> {
    let mut state = load();
    state.exe = Some(record);
    state.updated_at = Some(current_rfc3339());
    save(&state)
}

/// 同时记录 manifest_version 和 path（一般只在确定要写其他字段时一起写）
pub fn set_manifest_version_and_path(version: &str, path: &str) -> Result<(), String> {
    let mut state = load();
    state.manifest_version = Some(version.to_string());
    state.manifest_path = Some(path.to_string());
    state.updated_at = Some(current_rfc3339());
    save(&state)
}

/// 同时记录 manifest_version（一般只在确定要写其他字段时一起写）
pub fn set_manifest_version(version: &str) -> Result<(), String> {
    let mut state = load();
    state.manifest_version = Some(version.to_string());
    state.updated_at = Some(current_rfc3339());
    save(&state)
}

/// 当前 pack 是否处于「ok」状态（存在且 sha256 匹配）
pub fn is_pack_ok(state: &VerifyState, name: &str) -> bool {
    state
        .packs
        .get(name)
        .map(|r| r.status == "ok")
        .unwrap_or(false)
}

/// exe 是否 ok（且 sha256 与预期一致）
pub fn is_exe_ok(state: &VerifyState) -> bool {
    state
        .exe
        .as_ref()
        .map(|r| r.status == "ok")
        .unwrap_or(false)
}

/// 检查 exe 的 sha256 是否与给定的预期值一致（用于热更新检测）
pub fn is_exe_sha256_match(state: &VerifyState, expected_sha256: &str) -> bool {
    state
        .exe
        .as_ref()
        .map(|r| r.status == "ok" && r.sha256.to_lowercase() == expected_sha256.to_lowercase())
        .unwrap_or(false)
}

/// 检查 pack 的 sha256 是否与给定的预期值一致
pub fn is_pack_sha256_match(state: &VerifyState, name: &str, expected_sha256: &str) -> bool {
    state
        .packs
        .get(name)
        .map(|r| r.status == "ok" && r.sha256.to_lowercase() == expected_sha256.to_lowercase())
        .unwrap_or(false)
}

/// 检查 manifest path 是否与给定值一致
pub fn is_manifest_path_match(state: &VerifyState, expected_path: &str) -> bool {
    state
        .manifest_path
        .as_ref()
        .map(|p| p == expected_path)
        .unwrap_or(false)
}

/// 是否所有 pack 都已经 ok
#[allow(dead_code)]
pub fn all_packs_ok(state: &VerifyState, pack_names: &[String]) -> bool {
    pack_names.iter().all(|n| is_pack_ok(state, n))
}

/// 是否整个游戏（exe + 所有 pack）都 ok
#[allow(dead_code)]
pub fn is_fully_ok(state: &VerifyState, pack_names: &[String]) -> bool {
    is_exe_ok(state) && all_packs_ok(state, pack_names)
}

fn current_rfc3339() -> String {
    // 不引入 chrono 依赖：直接用 std::time::SystemTime 转 RFC3339 字符串。
    // 失败时退回到一个固定字符串即可（不影响校验逻辑）。
    use std::time::{SystemTime, UNIX_EPOCH};
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    // 简化为秒级 unix 时间戳（前端无需解析也能看懂）
    format!("{}", now)
}

/// 读本地 manifest.json（与 manifest.rs 的 LocalManifestInfo 区分，本函数只返回轻量结构）
pub fn read_local_pack_names() -> Result<Vec<String>, String> {
    let path = crate::paths::game_dir()?.join("manifest.json");
    if !Path::new(&path).exists() {
        return Ok(vec![]);
    }
    let content = fs::read_to_string(&path).map_err(|e| format!("读取 manifest.json 失败: {}", e))?;
    let v: serde_json::Value =
        serde_json::from_str(&content).map_err(|e| format!("解析 manifest.json 失败: {}", e))?;
    let mut names: Vec<String> = Vec::new();
    if let Some(packs) = v.get("packs").and_then(|p| p.as_object()) {
        for (k, _) in packs.iter() {
            names.push(k.clone());
        }
    }
    names.sort();
    Ok(names)
}
