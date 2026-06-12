// ============== 启动器后端入口 ==============
// 这里是 Tauri 启动器后端的统一入口，只负责：
//   1) 把各功能模块声明出来
//   2) 注册 Tauri command handlers
//   3) 启动 Tauri Builder
// 任何具体实现都不放在本文件中。
//
// 模块结构：
//   - config     : 全局常量 (超时时间 / 域名 / 事件名)
//   - paths      : 路径解析 (app 根目录 / game 目录)
//   - events     : 进度事件 payload 与 emit 函数
//   - manifest   : 本地 manifest 读取 / 保存 / 卸载 / 版本解析
//   - network    : HTTP 客户端 / 流式下载 / 主备域名重试拉取

mod config;
mod events;
mod manifest;
mod network;
mod paths;

#[tauri::command]
fn greet(name: &str) -> String {
    format!("Hello, {}! You've been greeted from Rust!", name)
}

/// 返回当前 app root 路径
#[tauri::command]
fn get_working_dir() -> Result<String, String> {
    let dir = paths::app_root_dir()?;
    Ok(dir.to_string_lossy().to_string())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            greet,
            get_working_dir,
            manifest::read_local_manifest,
            network::fetch_manifest,
            network::fetch_manifest_with_fallback,
            manifest::parse_manifest_version,
            manifest::save_manifest,
            manifest::delete_manifest
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
