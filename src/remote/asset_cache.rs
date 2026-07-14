//! 远程资产本地文件缓存：按 asset_id 下载到磁盘，供路径式纹理/播放管线复用。

use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use crate::remote::client::try_build_http_client;
use crate::remote::config::RemoteConfig;
use crate::remote::error::{RemoteError, RemoteResult};
use crate::remote::services::RemoteAssetService;
use crate::remote::types::RemoteAssetRef;

/// 远程资产 → 本地缓存文件。
#[derive(Debug, Clone)]
pub struct RemoteAssetCache {
    root: PathBuf,
}

impl RemoteAssetCache {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    /// 默认目录：`~/.imgforge/remote_cache/assets`。
    pub fn default_root() -> PathBuf {
        if let Some(home) = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")) {
            return Path::new(&home)
                .join(".imgforge")
                .join("remote_cache")
                .join("assets");
        }
        PathBuf::from(".imgforge")
            .join("remote_cache")
            .join("assets")
    }

    pub fn from_config(cfg: &RemoteConfig) -> Self {
        let root = cfg
            .cache_path
            .as_ref()
            .and_then(|p| p.parent().map(|d| d.join("assets")))
            .unwrap_or_else(Self::default_root);
        Self::new(root)
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn local_path_for(&self, asset: &RemoteAssetRef) -> PathBuf {
        let safe_name = sanitize_filename(&asset.name);
        self.root.join(&asset.id).join(safe_name)
    }

    /// 若本地已有且 checksum 匹配则直接返回；否则下载。
    pub fn ensure_local(
        &self,
        cfg: &RemoteConfig,
        asset: &RemoteAssetRef,
    ) -> RemoteResult<PathBuf> {
        let dest = self.local_path_for(asset);
        if dest.exists() {
            if let Some(expected) = asset.checksum.as_deref() {
                if checksum_matches(&dest, expected)? {
                    return Ok(dest);
                }
            } else {
                return Ok(dest);
            }
        }
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| RemoteError::Cache(format!("mkdir {}: {e}", parent.display())))?;
        }
        let client = try_build_http_client(cfg)?;
        let service = RemoteAssetService::new(cfg.clone(), client);
        service.download_to(&asset.id, &dest)?;
        if let Some(expected) = asset.checksum.as_deref() {
            if !checksum_matches(&dest, expected)? {
                let _ = std::fs::remove_file(&dest);
                return Err(RemoteError::Cache(format!(
                    "checksum mismatch for asset {}",
                    asset.id
                )));
            }
        }
        Ok(dest)
    }

    /// 已缓存则返回路径，否则 None（不触发网络）。
    pub fn peek_local(&self, asset: &RemoteAssetRef) -> Option<PathBuf> {
        let dest = self.local_path_for(asset);
        dest.exists().then_some(dest)
    }
}

fn sanitize_filename(name: &str) -> String {
    let base = Path::new(name)
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "asset.bin".into());
    let cleaned: String = base
        .chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            other => other,
        })
        .collect();
    if cleaned.is_empty() {
        "asset.bin".into()
    } else {
        cleaned
    }
}

fn checksum_matches(path: &Path, expected: &str) -> RemoteResult<bool> {
    let bytes = std::fs::read(path)
        .map_err(|e| RemoteError::Cache(format!("read {}: {e}", path.display())))?;
    let hex = format!("sha256:{:x}", Sha256::digest(&bytes));
    let expected = expected.trim();
    Ok(hex.eq_ignore_ascii_case(expected)
        || hex
            .strip_prefix("sha256:")
            .map(|h| expected.eq_ignore_ascii_case(h))
            .unwrap_or(false)
        || expected
            .strip_prefix("sha256:")
            .map(|h| {
                hex.strip_prefix("sha256:")
                    .unwrap_or(&hex)
                    .eq_ignore_ascii_case(h)
            })
            .unwrap_or(false))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_strips_path_separators() {
        assert_eq!(sanitize_filename("a/b\\c.png"), "c.png");
    }

    #[test]
    fn peek_missing_returns_none() {
        let dir = tempfile::tempdir().unwrap();
        let cache = RemoteAssetCache::new(dir.path());
        let asset = RemoteAssetRef {
            id: "a1".into(),
            name: "x.png".into(),
            mime: None,
            size: None,
            checksum: None,
            download_url: None,
        };
        assert!(cache.peek_local(&asset).is_none());
    }
}
