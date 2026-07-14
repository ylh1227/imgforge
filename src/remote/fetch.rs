//! 轻量后台拉取：spawn 线程执行闭包，UI 每帧 poll。

use std::sync::{Arc, Mutex};
use std::thread;

use crate::remote::error::RemoteError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DataSource {
    Local,
    Remote,
}

impl DataSource {
    pub fn label(self) -> &'static str {
        match self {
            Self::Local => "本地",
            Self::Remote => "远程",
        }
    }

    pub fn from_remote_enabled(enabled: bool) -> Self {
        if enabled {
            Self::Remote
        } else {
            Self::Local
        }
    }
}

enum FetchState<T> {
    Pending,
    Ready(Result<T, String>),
    Taken,
}

/// 一次性后台任务句柄。
pub struct RemoteFetch<T> {
    inner: Arc<Mutex<FetchState<T>>>,
}

impl<T: Send + 'static> RemoteFetch<T> {
    pub fn spawn(f: impl FnOnce() -> Result<T, RemoteError> + Send + 'static) -> Self {
        let inner = Arc::new(Mutex::new(FetchState::Pending));
        let slot = Arc::clone(&inner);
        thread::Builder::new()
            .name("imgforge-remote-fetch".into())
            .spawn(move || {
                let result = f().map_err(|e| e.to_string());
                if let Ok(mut g) = slot.lock() {
                    *g = FetchState::Ready(result);
                }
            })
            .expect("spawn remote fetch");
        Self { inner }
    }

    pub fn is_pending(&self) -> bool {
        self.inner
            .lock()
            .map(|g| matches!(&*g, FetchState::Pending))
            .unwrap_or(false)
    }

    /// 若完成则取出结果（只可取一次）；未完成返回 None。
    pub fn poll(&self) -> Option<Result<T, String>> {
        let mut g = self.inner.lock().ok()?;
        match std::mem::replace(&mut *g, FetchState::Taken) {
            FetchState::Ready(r) => Some(r),
            FetchState::Pending => {
                *g = FetchState::Pending;
                None
            }
            FetchState::Taken => None,
        }
    }
}
