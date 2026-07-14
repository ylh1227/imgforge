//! ImgForge remote-first 服务端：控制面 API、元数据存储、可靠队列、对象存储与 Worker。
//!
//! 优先使用 Postgres / Redis Streams / S3-MinIO 组成远程处理栈；未配置时
//! 回退到 SQLite / 内存队列 / 磁盘对象存储，便于本地联调。

pub mod api;
pub mod auth;
pub mod config;
pub mod object_store;
pub mod queue;
pub mod rate_limit;
pub mod state;
pub mod storage;
pub mod worker;

pub use config::ServerConfig;
pub use state::AppState;
