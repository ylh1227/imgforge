//! ImgForge 服务端入口（feature = `server`）。
//!
//! 远程优先版：配置 Postgres / Redis / S3 时使用完整远程栈，未配置时回退
//! SQLite 元数据 + 磁盘对象存储 + 内存队列。
//!
//! ```text
//! cargo run --bin imgforge-server --features server
//! ```
//!
//! 环境变量：
//! - `IMGFORGE_SERVER_BIND`（默认 `127.0.0.1:8787`）
//! - `IMGFORGE_SERVER_TOKEN`（可选 Bearer）
//! - `IMGFORGE_PUBLIC_BASE`（对外 API base，默认 `http://127.0.0.1:8787`）
//! - `IMGFORGE_SERVER_DATA_DIR`（默认 `~/.imgforge/server`）
//! - `DATABASE_URL` / `IMGFORGE_DATABASE_URL`（Postgres 元数据）
//! - `REDIS_URL` / `IMGFORGE_REDIS_URL`（Redis Streams 队列）
//! - `IMGFORGE_S3_ENDPOINT`、`IMGFORGE_S3_REGION`、`IMGFORGE_S3_BUCKET`
//! - `IMGFORGE_S3_ACCESS_KEY` / `AWS_ACCESS_KEY_ID`
//! - `IMGFORGE_S3_SECRET_KEY` / `AWS_SECRET_ACCESS_KEY`
//! - `IMGFORGE_S3_PATH_STYLE`（MinIO 常用）
//! - `IMGFORGE_INLINE_WORKER`（默认开）

use std::net::SocketAddr;

use imgforge::server::{api, worker::InlineWorker, AppState, ServerConfig};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let config = ServerConfig::from_env();
    let bind = config.bind.clone();
    let inline = config.inline_worker;
    let data_dir = config.data_dir.display().to_string();
    let backend_summary = config.backend_summary();

    let state = AppState::from_config(config)?;
    let _worker = if inline {
        Some(InlineWorker::spawn(state.clone()))
    } else {
        None
    };

    let app = api::app(state);
    let addr: SocketAddr = bind.parse()?;
    tracing::info!(
        %addr,
        data_dir = %data_dir,
        backend = %backend_summary,
        "imgforge-server listening"
    );
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}
