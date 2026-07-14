//! 服务端运行配置（远程优先：Postgres / Redis / S3-MinIO）。

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::remote::worker_policy::WorkerReliabilityPolicy;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    /// 监听地址，如 `0.0.0.0:8787`。
    pub bind: String,
    /// 可选 Bearer token；为空则不校验（仅开发）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auth_token: Option<String>,
    /// 对外 API base（不含尾斜杠）。
    pub public_base: String,
    /// 本地工作/缓存目录（Worker 临时文件、SQLite 回退）。
    pub data_dir: PathBuf,
    /// Postgres 连接串；未设置时回退 SQLite（仅测试/单机开发）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub database_url: Option<String>,
    /// Redis URL；未设置时回退内存队列。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub redis_url: Option<String>,
    /// S3/MinIO 配置；未设置时回退本地磁盘对象存储。
    #[serde(default)]
    pub s3: S3Config,
    /// Worker 可靠性策略。
    #[serde(default)]
    pub worker: WorkerReliabilityPolicy,
    /// API 进程内是否跑内联 Worker。
    #[serde(default = "default_true")]
    pub inline_worker: bool,
    /// 默认 workspace（无 token 映射时）。
    #[serde(default = "default_workspace")]
    pub default_workspace: String,
    /// 单次上传大小上限（字节）。
    #[serde(default = "default_max_upload")]
    pub max_upload_bytes: u64,
    /// 轻量 API 令牌桶限流（每 token/IP 每分钟请求数）。
    #[serde(default = "default_rate_limit_per_minute")]
    pub rate_limit_per_minute: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct S3Config {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub endpoint: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub region: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bucket: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub access_key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub secret_key: Option<String>,
    #[serde(default)]
    pub path_style: bool,
}

fn default_true() -> bool {
    true
}

fn default_workspace() -> String {
    "default".into()
}

fn default_max_upload() -> u64 {
    512 * 1024 * 1024
}

fn default_rate_limit_per_minute() -> u32 {
    120
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            bind: "0.0.0.0:8787".into(),
            auth_token: None,
            public_base: "http://127.0.0.1:8787".into(),
            data_dir: default_data_dir(),
            database_url: None,
            redis_url: None,
            s3: S3Config::default(),
            worker: WorkerReliabilityPolicy::default(),
            inline_worker: true,
            default_workspace: default_workspace(),
            max_upload_bytes: default_max_upload(),
            rate_limit_per_minute: default_rate_limit_per_minute(),
        }
    }
}

impl ServerConfig {
    pub fn objects_dir(&self) -> PathBuf {
        self.data_dir.join("objects")
    }

    pub fn uploads_dir(&self) -> PathBuf {
        self.data_dir.join("uploads")
    }

    pub fn work_dir(&self) -> PathBuf {
        self.data_dir.join("work")
    }

    pub fn outputs_dir(&self) -> PathBuf {
        self.data_dir.join("outputs")
    }

    pub fn sqlite_path(&self) -> PathBuf {
        self.data_dir.join("meta.sqlite")
    }

    pub fn uses_postgres(&self) -> bool {
        self.database_url
            .as_ref()
            .map(|u| u.starts_with("postgres"))
            .unwrap_or(false)
    }

    pub fn uses_redis(&self) -> bool {
        self.redis_url
            .as_ref()
            .map(|u| !u.trim().is_empty())
            .unwrap_or(false)
    }

    pub fn uses_s3(&self) -> bool {
        self.s3
            .bucket
            .as_ref()
            .map(|b| !b.is_empty())
            .unwrap_or(false)
            && self
                .s3
                .endpoint
                .as_ref()
                .map(|e| !e.is_empty())
                .unwrap_or(false)
    }

    pub fn object_store_public_base(&self) -> String {
        format!("{}/v1/artifacts", self.public_base.trim_end_matches('/'))
    }

    pub fn ensure_dirs(&self) -> std::io::Result<()> {
        std::fs::create_dir_all(&self.data_dir)?;
        std::fs::create_dir_all(self.objects_dir())?;
        std::fs::create_dir_all(self.uploads_dir())?;
        std::fs::create_dir_all(self.work_dir())?;
        std::fs::create_dir_all(self.outputs_dir())?;
        Ok(())
    }

    pub fn from_env() -> Self {
        let mut config = Self::default();
        if let Ok(bind) = std::env::var("IMGFORGE_SERVER_BIND") {
            config.bind = bind;
        }
        if let Ok(token) = std::env::var("IMGFORGE_SERVER_TOKEN") {
            if !token.trim().is_empty() {
                config.auth_token = Some(token);
            }
        }
        if let Ok(base) =
            std::env::var("IMGFORGE_PUBLIC_BASE").or_else(|_| std::env::var("IMGFORGE_OBJECT_BASE"))
        {
            let trimmed = base.trim_end_matches('/').to_string();
            config.public_base = trimmed
                .strip_suffix("/objects")
                .or_else(|| trimmed.strip_suffix("/v1/artifacts"))
                .unwrap_or(&trimmed)
                .to_string();
        }
        if let Ok(dir) = std::env::var("IMGFORGE_SERVER_DATA_DIR") {
            config.data_dir = PathBuf::from(dir);
        }
        if let Ok(url) =
            std::env::var("IMGFORGE_DATABASE_URL").or_else(|_| std::env::var("DATABASE_URL"))
        {
            if !url.trim().is_empty() {
                config.database_url = Some(url);
            }
        }
        if let Ok(url) = std::env::var("IMGFORGE_REDIS_URL").or_else(|_| std::env::var("REDIS_URL"))
        {
            if !url.trim().is_empty() {
                config.redis_url = Some(url);
            }
        }
        if let Ok(v) = std::env::var("IMGFORGE_INLINE_WORKER") {
            config.inline_worker = !matches!(v.as_str(), "0" | "false" | "FALSE" | "no");
        }
        if let Ok(ws) = std::env::var("IMGFORGE_DEFAULT_WORKSPACE") {
            config.default_workspace = ws;
        }
        if let Ok(n) = std::env::var("IMGFORGE_MAX_UPLOAD_BYTES") {
            if let Ok(v) = n.parse() {
                config.max_upload_bytes = v;
            }
        }
        if let Ok(n) = std::env::var("IMGFORGE_RATE_LIMIT_PER_MINUTE") {
            if let Ok(v) = n.parse() {
                config.rate_limit_per_minute = v;
            }
        }
        // S3 / MinIO
        if let Ok(ep) = std::env::var("IMGFORGE_S3_ENDPOINT") {
            config.s3.endpoint = Some(ep);
        }
        if let Ok(region) = std::env::var("IMGFORGE_S3_REGION") {
            config.s3.region = Some(region);
        }
        if let Ok(bucket) = std::env::var("IMGFORGE_S3_BUCKET") {
            config.s3.bucket = Some(bucket);
        }
        if let Ok(ak) =
            std::env::var("IMGFORGE_S3_ACCESS_KEY").or_else(|_| std::env::var("AWS_ACCESS_KEY_ID"))
        {
            config.s3.access_key = Some(ak);
        }
        if let Ok(sk) = std::env::var("IMGFORGE_S3_SECRET_KEY")
            .or_else(|_| std::env::var("AWS_SECRET_ACCESS_KEY"))
        {
            config.s3.secret_key = Some(sk);
        }
        if let Ok(v) = std::env::var("IMGFORGE_S3_PATH_STYLE") {
            config.s3.path_style = matches!(v.as_str(), "1" | "true" | "TRUE" | "yes");
        }
        config
    }

    pub fn backend_summary(&self) -> String {
        format!(
            "meta={} queue={} objects={}",
            if self.uses_postgres() {
                "postgres"
            } else {
                "sqlite-fallback"
            },
            if self.uses_redis() {
                "redis-streams"
            } else {
                "memory-fallback"
            },
            if self.uses_s3() {
                "s3"
            } else {
                "disk-fallback"
            }
        )
    }
}

fn default_data_dir() -> PathBuf {
    if let Some(home) = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")) {
        return Path::new(&home).join(".imgforge").join("server");
    }
    PathBuf::from(".imgforge-server")
}
