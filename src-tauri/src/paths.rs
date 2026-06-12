// ============== 路径解析 ==============

use std::path::PathBuf;

use crate::config::{GAME_EXE_NAME, HOT_UPDATE_DIR, VERIFY_RECORD_NAME};

/// 获取用于存放 /game 的根目录：
/// - 调试时（debug build）：项目根目录下的 `run` 子目录
/// - 打包运行时（release build）：程序可执行文件所在目录
pub fn app_root_dir() -> Result<PathBuf, String> {
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
pub fn game_dir() -> Result<PathBuf, String> {
    let root = app_root_dir()?;
    Ok(root.join("game"))
}

/// 游戏主程序绝对路径：`<game>/game.exe`
pub fn game_exe_path() -> Result<PathBuf, String> {
    Ok(game_dir()?.join(GAME_EXE_NAME))
}

/// hot_update 目录：`<game>/hot_update`
pub fn hot_update_dir() -> Result<PathBuf, String> {
    Ok(game_dir()?.join(HOT_UPDATE_DIR))
}

/// hot_update 下某个 pck 的绝对路径：`<game>/hot_update/<name>.pck`
pub fn pack_file_path(pack_name: &str) -> Result<PathBuf, String> {
    Ok(hot_update_dir()?.join(format!("{}.pck", pack_name)))
}

/// verify.json 的绝对路径：`<game>/verify.json`
pub fn verify_json_path() -> Result<PathBuf, String> {
    Ok(game_dir()?.join(VERIFY_RECORD_NAME))
}
