//! 对象存储抽象（本地磁盘实现；S3/MinIO 可替换）。

use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use sha2::{Digest, Sha256};

use crate::server::storage::StoreError;

pub type ObjectResult<T> = Result<T, StoreError>;

pub trait ObjectStore: Send + Sync {
    fn put_bytes(&self, key: &str, bytes: Vec<u8>) -> ObjectResult<()>;
    fn get_bytes(&self, key: &str) -> ObjectResult<Vec<u8>>;
    fn delete(&self, key: &str) -> ObjectResult<()>;
    fn exists(&self, key: &str) -> ObjectResult<bool>;
    /// 生成短期上传 URL（真实实现为预签名；本地版返回服务端 PUT 路径）。
    fn presign_put(&self, key: &str, ttl_secs: u64) -> ObjectResult<String>;
    fn presign_get(&self, key: &str, ttl_secs: u64) -> ObjectResult<String>;
    /// 可选：解析本地路径（磁盘实现）。
    fn local_path(&self, key: &str) -> Option<PathBuf> {
        let _ = key;
        None
    }
    /// 从已有文件登记到对象存储（硬链接或复制）。
    fn put_file(&self, key: &str, src: &Path) -> ObjectResult<u64> {
        let bytes = fs::read(src).map_err(|e| StoreError::Internal(e.to_string()))?;
        let len = bytes.len() as u64;
        self.put_bytes(key, bytes)?;
        Ok(len)
    }
}

/// 内存对象存储（测试用）。
pub struct MemoryObjectStore {
    inner: Arc<Mutex<std::collections::HashMap<String, Vec<u8>>>>,
    public_base: String,
}

impl MemoryObjectStore {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(std::collections::HashMap::new())),
            public_base: "memory://objects".into(),
        }
    }

    pub fn with_public_base(public_base: impl Into<String>) -> Self {
        Self {
            inner: Arc::new(Mutex::new(std::collections::HashMap::new())),
            public_base: public_base.into(),
        }
    }

    fn lock(
        &self,
    ) -> ObjectResult<std::sync::MutexGuard<'_, std::collections::HashMap<String, Vec<u8>>>> {
        self.inner
            .lock()
            .map_err(|e| StoreError::Internal(e.to_string()))
    }
}

impl Default for MemoryObjectStore {
    fn default() -> Self {
        Self::new()
    }
}

impl ObjectStore for MemoryObjectStore {
    fn put_bytes(&self, key: &str, bytes: Vec<u8>) -> ObjectResult<()> {
        self.lock()?.insert(key.to_string(), bytes);
        Ok(())
    }

    fn get_bytes(&self, key: &str) -> ObjectResult<Vec<u8>> {
        self.lock()?
            .get(key)
            .cloned()
            .ok_or_else(|| StoreError::NotFound(key.into()))
    }

    fn delete(&self, key: &str) -> ObjectResult<()> {
        self.lock()?.remove(key);
        Ok(())
    }

    fn exists(&self, key: &str) -> ObjectResult<bool> {
        Ok(self.lock()?.contains_key(key))
    }

    fn presign_put(&self, key: &str, _ttl_secs: u64) -> ObjectResult<String> {
        Ok(format!(
            "{}/put/{}",
            self.public_base.trim_end_matches('/'),
            key
        ))
    }

    fn presign_get(&self, key: &str, _ttl_secs: u64) -> ObjectResult<String> {
        Ok(format!(
            "{}/get/{}",
            self.public_base.trim_end_matches('/'),
            key
        ))
    }
}

/// 本地磁盘对象存储：`{root}/{safe_key}`，原子写入。
pub struct DiskObjectStore {
    root: PathBuf,
    public_base: String,
}

impl DiskObjectStore {
    pub fn new(root: impl Into<PathBuf>, public_base: impl Into<String>) -> ObjectResult<Self> {
        let root = root.into();
        fs::create_dir_all(&root).map_err(|e| StoreError::Internal(e.to_string()))?;
        Ok(Self {
            root,
            public_base: public_base.into(),
        })
    }

    fn path_for(&self, key: &str) -> ObjectResult<PathBuf> {
        let safe = sanitize_key(key)?;
        let path = self.root.join(&safe);
        if !path.starts_with(&self.root) {
            return Err(StoreError::Validation("invalid object key".into()));
        }
        Ok(path)
    }
}

impl ObjectStore for DiskObjectStore {
    fn put_bytes(&self, key: &str, bytes: Vec<u8>) -> ObjectResult<()> {
        let path = self.path_for(key)?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|e| StoreError::Internal(e.to_string()))?;
        }
        let tmp = path.with_extension("tmp");
        {
            let mut f = File::create(&tmp).map_err(|e| StoreError::Internal(e.to_string()))?;
            f.write_all(&bytes)
                .map_err(|e| StoreError::Internal(e.to_string()))?;
            f.sync_all()
                .map_err(|e| StoreError::Internal(e.to_string()))?;
        }
        fs::rename(&tmp, &path).map_err(|e| StoreError::Internal(e.to_string()))?;
        Ok(())
    }

    fn get_bytes(&self, key: &str) -> ObjectResult<Vec<u8>> {
        let path = self.path_for(key)?;
        fs::read(&path).map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                StoreError::NotFound(key.into())
            } else {
                StoreError::Internal(e.to_string())
            }
        })
    }

    fn delete(&self, key: &str) -> ObjectResult<()> {
        let path = self.path_for(key)?;
        match fs::remove_file(&path) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(StoreError::Internal(e.to_string())),
        }
    }

    fn exists(&self, key: &str) -> ObjectResult<bool> {
        Ok(self.path_for(key)?.exists())
    }

    fn presign_put(&self, key: &str, _ttl_secs: u64) -> ObjectResult<String> {
        Ok(format!(
            "{}/uploads/{}/bytes",
            self.public_base.trim_end_matches('/'),
            key
        ))
    }

    fn presign_get(&self, key: &str, _ttl_secs: u64) -> ObjectResult<String> {
        Ok(format!(
            "{}/v1/artifacts/{}/content",
            self.public_base.trim_end_matches('/'),
            key
        ))
    }

    fn local_path(&self, key: &str) -> Option<PathBuf> {
        self.path_for(key).ok()
    }

    fn put_file(&self, key: &str, src: &Path) -> ObjectResult<u64> {
        let path = self.path_for(key)?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|e| StoreError::Internal(e.to_string()))?;
        }
        let meta = fs::metadata(src).map_err(|e| StoreError::Internal(e.to_string()))?;
        // 优先硬链接，失败则复制。
        if path.exists() {
            let _ = fs::remove_file(&path);
        }
        match fs::hard_link(src, &path) {
            Ok(()) => Ok(meta.len()),
            Err(_) => {
                fs::copy(src, &path).map_err(|e| StoreError::Internal(e.to_string()))?;
                Ok(meta.len())
            }
        }
    }
}

fn sanitize_key(key: &str) -> ObjectResult<String> {
    let trimmed = key.trim().trim_start_matches('/');
    if trimmed.is_empty() || trimmed.contains("..") {
        return Err(StoreError::Validation("invalid object key".into()));
    }
    Ok(trimmed.replace('\\', "/"))
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("sha256:{:x}", hasher.finalize())
}

pub fn sha256_file(path: &Path) -> ObjectResult<String> {
    let mut file = File::open(path).map_err(|e| StoreError::Internal(e.to_string()))?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = file
            .read(&mut buf)
            .map_err(|e| StoreError::Internal(e.to_string()))?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(format!("sha256:{:x}", hasher.finalize()))
}

#[cfg(feature = "server")]
pub mod s3;
#[cfg(feature = "server")]
pub use s3::S3ObjectStore;

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn disk_put_get_roundtrip() {
        let dir = tempdir().unwrap();
        let store = DiskObjectStore::new(dir.path(), "http://127.0.0.1:8787").unwrap();
        store.put_bytes("a/b.bin", b"hello".to_vec()).unwrap();
        assert_eq!(store.get_bytes("a/b.bin").unwrap(), b"hello");
        assert!(store.exists("a/b.bin").unwrap());
        assert_eq!(
            sha256_hex(b"hello"),
            "sha256:2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
        );
    }
}
