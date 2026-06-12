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
/// 单个文件下载失败时的最大重试次数（与主备域名叠加）
pub const FILE_MAX_ATTEMPTS: usize = 3;

/// 服务端域名配置：主用 oss.jjxf.cc，备用 update.jjxf.cc
pub const PRIMARY_HOST: &str = "https://oss.jjxf.cc";
pub const BACKUP_HOST: &str = "https://update.jjxf.cc";
/// manifest.json 现在直接位于服务器根目录下
pub const MANIFEST_PATH: &str = "/manifest.json";
/// 最多重试次数（包含主用域名 + 备用域名，最多 3 次整体尝试）
pub const MAX_ATTEMPTS: usize = 3;

// ============== 下载相关路径配置 ==============
// 路径常量说明：
//   - manifest 中新增 `path` 字段（例如 "0.8.2.9_6_13"），所有资源文件都放在该子目录下
//   - exe:  {host}/{manifest.path}/game.exe
//   - pack: {host}/{manifest.path}/{pack_name}.pck
//   若 manifest 中缺少 path 字段，则回退使用 "True_Pcks" 作为默认路径前缀

/// manifest 中缺少 path 字段时的默认回退路径前缀
pub const DEFAULT_PACKS_PATH: &str = "True_Pcks";
/// 资源包文件后缀
pub const PACK_FILE_EXT: &str = ".pck";

/// 本地游戏主程序文件名（落在 game_dir 下）
pub const GAME_EXE_NAME: &str = "game.exe";
/// 本地资源包目录（落在 game_dir/hot_update 下）
pub const HOT_UPDATE_DIR: &str = "hot_update";
/// 本地 manifest.json 文件名
#[allow(dead_code)]
pub const LOCAL_MANIFEST_NAME: &str = "manifest.json"; // 供外部模块按需使用
/// 本地校验记录文件名
pub const VERIFY_RECORD_NAME: &str = "verify.json";

/// 进度事件名（前端用 listen("download:progress", ...) 监听）
pub const EVT_PROGRESS: &str = "manifest:progress";
pub const EVT_DONE: &str = "manifest:done";
pub const EVT_ERROR: &str = "manifest:error";

/// 下载阶段事件名（用于新下载流程，前端 listen("download:progress") 监听）
pub const EVT_DOWNLOAD_PROGRESS: &str = "download:progress";
#[allow(dead_code)]
pub const EVT_DOWNLOAD_DONE: &str = "download:done";
pub const EVT_DOWNLOAD_ERROR: &str = "download:error";

/// 单文件多线程分片下载的并发线程数（IDM 风格）
pub const FILE_CHUNK_CONCURRENCY: usize = 36;
/// 启用多线程分片的最小文件大小（小于此值走单线程）
pub const CHUNK_MIN_FILE_SIZE: u64 = 512 * 1024; // 512KB
/// 单个分片的最小字节数（IDM 风格：根据文件总大小动态算）
pub const CHUNK_MIN_SIZE: u64 = 256 * 1024; // 256KB
/// 单个分片连续失败达到此次数后才认为该分片彻底失败
pub const CHUNK_MAX_CONSECUTIVE_FAILS: u32 = 5;
/// 一次下载中重试的退避初始延迟（后续翻倍）
pub const CHUNK_RETRY_BASE_MS: u64 = 200;
/// pck 文件并行下载数（同时下载多个 .pck 文件）
pub const PACKS_PARALLEL_DOWNLOADS: usize = 2;
