// ============== 启动器后端入口 ==============
// 这里是 Tauri 启动器后端的统一入口，只负责：
//   1) 把各功能模块声明出来
//   2) 注册 Tauri command handlers
//   3) 启动 Tauri Builder
// 任何具体实现都不放在本文件中。
//
// 模块结构：
//   - config     : 全局常量 (超时时间 / 域名 / 事件名 / 路径配置)
//   - paths      : 路径解析 (app 根目录 / game 目录 / hot_update / verify.json)
//   - events     : 进度事件 payload 与 emit 函数
//   - manifest   : 本地 manifest 读取 / 保存 / 卸载 / 版本解析
//   - network    : HTTP 客户端 / 流式下载 / 主备域名重试拉取
//   - verify_state : verify.json 的读写 + 单文件校验状态
//   - downloader : 游戏下载主流程 (exe + pck + 校验 + 写 verify.json)

mod config;
mod downloader;
mod events;
mod manifest;
mod network;
mod paths;
mod verify_state;

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
        .plugin(
            tauri_plugin_log::Builder::new()
                // 记录所有 log（包含下载器 [Downloader] 状态行）
                .level(log::LevelFilter::Info)
                .build(),
        )

        .invoke_handler(tauri::generate_handler![

            greet,
            get_working_dir,
            manifest::read_local_manifest,
            network::fetch_manifest,
            network::fetch_manifest_with_fallback,
            manifest::parse_manifest_version,
            manifest::save_manifest,
            manifest::delete_manifest,
            downloader::start_download,
            downloader::verify_local_files,
            downloader::read_verify_state,
            downloader::clear_verify_state,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
