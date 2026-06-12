// ============== manifest 相关 (本地读取 / 保存 / 卸载 / 解析) ==============

use std::fs;

use crate::paths::game_dir;

/// 读取本地 manifest.json 时返回给前端的结构
#[derive(serde::Serialize)]
pub struct LocalManifestInfo {
    pub exists: bool,
    pub version: Option<String>,
    pub content: Option<String>,
    pub path: String,
}

/// 读取本地 manifest.json 的版本号与原始内容
#[tauri::command]
pub fn read_local_manifest() -> Result<LocalManifestInfo, String> {
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

/// 从 manifest.json 文本中解析 version 字段
pub fn parse_version(content: &str) -> Option<String> {
    let value: serde_json::Value = serde_json::from_str(content).ok()?;
    value
        .get("version")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
}

/// 解析服务端 manifest.json 的版本号
#[tauri::command]
pub fn parse_manifest_version(content: String) -> Option<String> {
    parse_version(&content)
}

/// 将服务端 manifest.json 内容保存到 <app_root>/game/manifest.json
#[tauri::command]
pub fn save_manifest(content: String) -> Result<String, String> {
    let dir = game_dir()?;
    fs::create_dir_all(&dir).map_err(|e| format!("创建 game 目录失败: {}", e))?;
    let path = dir.join("manifest.json");
    fs::write(&path, content).map_err(|e| format!("写入 manifest.json 失败: {}", e))?;
    Ok(path.to_string_lossy().to_string())
}

/// 卸载游戏：清空整个 game 目录
/// 返回是否真的执行了清理（即 game 目录存在过）
#[tauri::command]
pub fn delete_manifest() -> Result<bool, String> {
    let dir = game_dir()?;
    if !dir.exists() {
        return Ok(false);
    }
    fs::remove_dir_all(&dir).map_err(|e| format!("清空 game 目录失败: {}", e))?;
    Ok(true)
}
