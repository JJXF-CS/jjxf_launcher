// ============== 路径解析 ==============

use std::path::PathBuf;

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
