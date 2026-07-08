//! 本地数据目录与数据库路径。

use std::path::PathBuf;

use crate::review::error::{ReviewError, ReviewResult};

/// 应用数据目录（`~/.imgforge` 或平台等价路径）。
pub fn app_data_dir() -> ReviewResult<PathBuf> {
    #[cfg(target_os = "windows")]
    {
        let base = std::env::var("APPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("."));
        Ok(base.join("imgforge"))
    }
    #[cfg(not(target_os = "windows"))]
    {
        let home =
            std::env::var("HOME").map_err(|_| ReviewError::Message("无法定位用户主目录".into()))?;
        Ok(PathBuf::from(home).join(".imgforge"))
    }
}

/// 与主项目共用的 SQLite 文件路径。
pub fn database_path() -> ReviewResult<PathBuf> {
    Ok(app_data_dir()?.join("imgforge.db"))
}

/// 缩略图缓存目录。
pub fn thumbnail_cache_dir() -> ReviewResult<PathBuf> {
    Ok(app_data_dir()?.join("thumbnails"))
}

/// 快捷键配置持久化路径。
pub fn shortcuts_path() -> ReviewResult<PathBuf> {
    Ok(app_data_dir()?.join("review_shortcuts.json"))
}

/// 评审模块配置路径。
pub fn review_config_path() -> ReviewResult<PathBuf> {
    Ok(app_data_dir()?.join("review_config.json"))
}

/// 预览图缓存目录（1920px）。
pub fn preview_cache_dir() -> ReviewResult<PathBuf> {
    Ok(app_data_dir()?.join("previews"))
}
