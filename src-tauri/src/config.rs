// ============== 全局常量配置 ==============

use std::time::Duration;

/// 单次请求的连接超时
pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(8);
/// 整体请求（包含建连 + 等待响应头）的超时
pub const REQUEST_TIMEOUT: Duration = Duration::from_secs(15);
/// 单个 chunk 读取之间的最大间隔（防止服务器挂起不返回数据）
pub const READ_CHUNK_TIMEOUT: Duration = Duration::from_secs(20);
/// 整体下载的最大允许耗时（兜底）
pub const OVERALL_TIMEOUT: Duration = Duration::from_secs(60);

/// 服务端域名配置：主用 oss.jjxf.cc，备用 update.jjxf.cc
pub const PRIMARY_HOST: &str = "https://oss.jjxf.cc";
pub const BACKUP_HOST: &str = "https://update.jjxf.cc";
/// manifest.json 相对路径
pub const MANIFEST_PATH: &str = "/True_Pcks/manifest.json";
/// 最多重试次数（包含主用域名 + 备用域名，最多 3 次整体尝试）
pub const MAX_ATTEMPTS: usize = 3;

/// 进度事件名（前端用 listen("manifest:progress", ...) 监听）
pub const EVT_PROGRESS: &str = "manifest:progress";
pub const EVT_DONE: &str = "manifest:done";
pub const EVT_ERROR: &str = "manifest:error";
