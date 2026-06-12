// ============== 游戏进程管理 ==============
// 负责启动 game.exe、检测进程是否运行、终止进程

use std::process::Command;
use std::sync::Mutex;

use serde::Serialize;

use crate::paths::game_exe_path;

/// 记录当前游戏的 PID
static GAME_PID: Mutex<Option<u32>> = Mutex::new(None);

/// 启动游戏
#[tauri::command]
pub fn launch_game() -> Result<String, String> {
    let exe_path = game_exe_path()
        .map_err(|e| format!("无法获取 game.exe 路径: {}", e))?;

    if !exe_path.exists() {
        return Err(format!("game.exe 不存在: {}", exe_path.display()));
    }

    // 如果已有游戏实例，先杀掉
    let _ = kill_game();

    let child = Command::new(&exe_path)
        .spawn()
        .map_err(|e| format!("启动 game.exe 失败: {}", e))?;

    let pid = child.id();
    let mut guard = GAME_PID.lock().unwrap();
    *guard = Some(pid);

    Ok(format!("已启动 game.exe (PID: {})", pid))
}

/// 终止游戏
#[tauri::command]
pub fn kill_game() -> Result<String, String> {
    let pid = {
        let mut guard = GAME_PID.lock().unwrap();
        let p = *guard;
        *guard = None;
        p
    };

    if let Some(pid) = pid {
        // 尝试优雅终止，失败则强制 kill
        kill_pid(pid);
        Ok(format!("已终止 game.exe (PID: {})", pid))
    } else {
        // 没有记录的 PID，尝试通过进程名查找并终止
        kill_by_name("game.exe");
        Ok("已终止 game.exe (by name)".to_string())
    }
}

/// 检查 game.exe 进程是否正在运行
/// 返回 JSON 给前端：{ running: bool }
#[derive(Serialize)]
pub struct ProcessStatus {
    pub running: bool,
}

#[tauri::command]
pub fn check_game_running() -> Result<ProcessStatus, String> {
    // 先通过记录的 PID 检查
    {
        let guard = GAME_PID.lock().unwrap();
        if let Some(pid) = *guard {
            if is_pid_alive(pid) {
                return Ok(ProcessStatus { running: true });
            } else {
                // PID 已失效，清除记录
                drop(guard);
                let mut guard = GAME_PID.lock().unwrap();
                *guard = None;
            }
        }
    }

    // 通过进程名查找（兜底）
    let running = find_process_by_name("game.exe").is_some();
    Ok(ProcessStatus { running })
}

// ============== 平台相关 ==============

#[cfg(target_os = "linux")]
fn kill_pid(pid: u32) {
    let _ = Command::new("kill")
        .arg(pid.to_string())
        .spawn();
}

#[cfg(target_os = "windows")]
fn kill_pid(pid: u32) {
    let _ = Command::new("taskkill")
        .args(["/F", "/PID", &pid.to_string()])
        .spawn();
}

#[cfg(target_os = "macos")]
fn kill_pid(pid: u32) {
    let _ = Command::new("kill")
        .arg(pid.to_string())
        .spawn();
}

#[cfg(target_os = "linux")]
fn kill_by_name(name: &str) {
    let _ = Command::new("pkill")
        .arg("-f")
        .arg(name)
        .spawn();
}

#[cfg(target_os = "windows")]
fn kill_by_name(name: &str) {
    let _ = Command::new("taskkill")
        .args(["/F", "/IM", name])
        .spawn();
}

#[cfg(target_os = "macos")]
fn kill_by_name(name: &str) {
    let _ = Command::new("pkill")
        .arg("-f")
        .arg(name)
        .spawn();
}

#[cfg(target_os = "linux")]
fn is_pid_alive(pid: u32) -> bool {
    std::path::Path::new(&format!("/proc/{}", pid)).exists()
}

#[cfg(target_os = "windows")]
fn is_pid_alive(pid: u32) -> bool {
    // Windows 上通过 process API 检查
    use std::os::windows::process::CommandExt;
    let output = std::process::Command::new("tasklist")
        .args(["/FI", &format!("PID eq {}", pid)])
        .creation_flags(0x08000000) // CREATE_NO_WINDOW
        .output();
    match output {
        Ok(out) => {
            let s = String::from_utf8_lossy(&out.stdout);
            s.contains(&pid.to_string())
        }
        Err(_) => false,
    }
}

#[cfg(target_os = "macos")]
fn is_pid_alive(pid: u32) -> bool {
    let output = std::process::Command::new("kill")
        .args(["-0", &pid.to_string()])
        .output();
    output.map(|o| o.status.success()).unwrap_or(false)
}

#[cfg(target_os = "linux")]
fn find_process_by_name(name: &str) -> Option<u32> {
    let output = std::process::Command::new("pgrep")
        .arg("-f")
        .arg(name)
        .output()
        .ok()?;
    let s = String::from_utf8(output.stdout).ok()?;
    s.lines().next()?.trim().parse().ok()
}

#[cfg(target_os = "windows")]
fn find_process_by_name(name: &str) -> Option<u32> {
    use std::os::windows::process::CommandExt;
    let output = std::process::Command::new("tasklist")
        .args(["/FI", &format!("IMAGENAME eq {}", name)])
        .creation_flags(0x08000000)
        .output()
        .ok()?;
    let s = String::from_utf8_lossy(&output.stdout);
    // tasklist 输出格式: game.exe  1234 Console ...
    let lines: Vec<&str> = s.lines().collect();
    if lines.len() < 4 {
        return None;
    }
    for line in &lines[3..] {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() >= 2 && parts[0].to_lowercase().contains(name) {
            return parts[1].parse().ok();
        }
    }
    None
}

#[cfg(target_os = "macos")]
fn find_process_by_name(name: &str) -> Option<u32> {
    let output = std::process::Command::new("pgrep")
        .arg("-f")
        .arg(name)
        .output()
        .ok()?;
    let s = String::from_utf8(output.stdout).ok()?;
    s.lines().next()?.trim().parse().ok()
}