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
/// manifest.json 相对路径
pub const MANIFEST_PATH: &str = "/True_Pcks/manifest.json";
/// 最多重试次数（包含主用域名 + 备用域名，最多 3 次整体尝试）
pub const MAX_ATTEMPTS: usize = 3;

// ============== 下载相关路径配置 ==============
// 下面这些常量是「可被业务方按需修改」的位置：
// 1) 以后游戏可能改名为 launcher.exe / client.exe 之类 → 改 GAME_EXE_NAME
// 2) 资源不一定都放在 True_Pcks/ 下了，可能以后在 CDN 的 /v1/packs/ 或 /hot_update/ 之类
//    → 改 PACKS_URL_PREFIX；也可以根据不同版本分目录存放
// 3) 如果以后 exe 也迁到 hot_update/ 之类的子目录 → 改 EXE_URL_PREFIX
//
// 现阶段保持从 True_Pcks/ 拉取（与 manifest.json 同目录），并加 .pck 后缀。

/// 游戏主程序相对服务器路径（不含域名）。例如 "/True_Pcks/game.exe"
pub const EXE_URL_PATH: &str = "/True_Pcks/game.exe";
/// 资源包 URL 前缀，最终拼成 "{prefix}/{pack_name}.pck"，例如 "https://oss.jjxf.cc/True_Pcks/Arts.pck"
pub const PACKS_URL_PREFIX: &str = "/True_Pcks";
/// 资源包文件后缀
pub const PACK_FILE_EXT: &str = ".pck";

/// 本地游戏主程序文件名（落在 game_dir 下）
pub const GAME_EXE_NAME: &str = "game.exe";
/// 本地资源包目录（落在 game_dir/hot_update 下）
pub const HOT_UPDATE_DIR: &str = "hot_update";
/// 本地 manifest.json 文件名
pub const LOCAL_MANIFEST_NAME: &str = "manifest.json"; // 供外部模块按需使用
/// 本地校验记录文件名
pub const VERIFY_RECORD_NAME: &str = "verify.json";

/// 进度事件名（前端用 listen("download:progress", ...) 监听）
pub const EVT_PROGRESS: &str = "manifest:progress";
pub const EVT_DONE: &str = "manifest:done";
pub const EVT_ERROR: &str = "manifest:error";

/// 下载阶段事件名（用于新下载流程，前端 listen("download:progress") 监听）
pub const EVT_DOWNLOAD_PROGRESS: &str = "download:progress";
pub const EVT_DOWNLOAD_DONE: &str = "download:done";
pub const EVT_DOWNLOAD_ERROR: &str = "download:error";

/// 单文件多线程分片下载的并发线程数（IDM 风格）
pub const FILE_CHUNK_CONCURRENCY: usize = 36;
/// 启用多线程分片的最小文件大小（小于此值走单线程）
pub const CHUNK_MIN_FILE_SIZE: u64 = 512 * 1024; // 1MB
/// 单个分片的最小字节数（IDM 风格：根据文件总大小动态算）
pub const CHUNK_MIN_SIZE: u64 = 256 * 1024; // 256KB
/// 单个分片连续失败达到此次数后才认为该分片彻底失败
pub const CHUNK_MAX_CONSECUTIVE_FAILS: u32 = 5;
/// 一次下载中重试的退避初始延迟（后续翻倍）
pub const CHUNK_RETRY_BASE_MS: u64 = 200;



