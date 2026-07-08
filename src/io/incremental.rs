//! 增量处理：基于文件哈希与修改时间跳过已处理文件（feature: incremental）。

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::core::error::{AppError, AppResult};
use crate::scheduler::task::ConversionTask;

#[cfg(feature = "incremental")]
use sha2::{Digest, Sha256};

/// 增量处理记录。
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct IncrementalState {
    pub entries: HashMap<String, IncrementalEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IncrementalEntry {
    pub hash: String,
    pub mtime_secs: u64,
    pub output_path: String,
}

/// 增量处理管理器。
pub struct IncrementalProcessor {
    state_path: PathBuf,
    state: IncrementalState,
    enabled: bool,
}

/// 增量任务过滤结果。
pub struct TaskFilterResult {
    pub tasks: Vec<ConversionTask>,
    pub skipped: usize,
}

impl IncrementalProcessor {
    /// 加载增量状态；`enabled` 为 false 时不进行过滤与持久化。
    pub fn load(state_path: PathBuf, enabled: bool) -> AppResult<Self> {
        if !enabled {
            return Ok(Self {
                state_path,
                state: IncrementalState::default(),
                enabled: false,
            });
        }

        let state = if state_path.exists() {
            let content = std::fs::read_to_string(&state_path)
                .map_err(|e| AppError::Incremental(e.to_string()))?;
            toml::from_str(&content).map_err(|e| AppError::Incremental(e.to_string()))?
        } else {
            IncrementalState::default()
        };
        Ok(Self {
            state_path,
            state,
            enabled: true,
        })
    }

    pub fn filter_tasks(&self, tasks: Vec<ConversionTask>) -> AppResult<TaskFilterResult> {
        if !self.enabled {
            return Ok(TaskFilterResult { tasks, skipped: 0 });
        }

        #[cfg(not(feature = "incremental"))]
        {
            return Ok(TaskFilterResult { tasks, skipped: 0 });
        }

        #[cfg(feature = "incremental")]
        {
            let total = tasks.len();
            let mut filtered = Vec::new();
            for task in tasks {
                if self.should_process(&task)? {
                    filtered.push(task);
                }
            }
            Ok(TaskFilterResult {
                skipped: total.saturating_sub(filtered.len()),
                tasks: filtered,
            })
        }
    }

    #[cfg(feature = "incremental")]
    fn should_process(&self, task: &ConversionTask) -> AppResult<bool> {
        let hash = compute_file_hash(&task.input_path)?;
        let mtime = file_mtime_secs(&task.input_path)?;
        let key = task.input_path.to_string_lossy().to_string();

        if let Some(entry) = self.state.entries.get(&key) {
            if entry.hash == hash
                && entry.mtime_secs == mtime
                && Path::new(&entry.output_path).exists()
            {
                return Ok(false);
            }
        }
        Ok(true)
    }

    pub fn record_success(&mut self, task: &ConversionTask) -> AppResult<()> {
        if !self.enabled {
            return Ok(());
        }

        #[cfg(feature = "incremental")]
        {
            let hash = compute_file_hash(&task.input_path)?;
            let mtime = file_mtime_secs(&task.input_path)?;
            let key = task.input_path.to_string_lossy().to_string();
            self.state.entries.insert(
                key,
                IncrementalEntry {
                    hash,
                    mtime_secs: mtime,
                    output_path: task.output_path.to_string_lossy().to_string(),
                },
            );
        }
        let _ = task;
        Ok(())
    }

    pub fn save(&self) -> AppResult<()> {
        if !self.enabled {
            return Ok(());
        }

        #[cfg(feature = "incremental")]
        {
            let content = toml::to_string_pretty(&self.state)
                .map_err(|e| AppError::Incremental(e.to_string()))?;
            if let Some(parent) = self.state_path.parent() {
                std::fs::create_dir_all(parent)
                    .map_err(|e| AppError::Incremental(e.to_string()))?;
            }
            std::fs::write(&self.state_path, content)
                .map_err(|e| AppError::Incremental(e.to_string()))?;
        }
        let _ = self;
        Ok(())
    }
}

#[cfg(feature = "incremental")]
fn compute_file_hash(path: &Path) -> AppResult<String> {
    use std::io::Read;
    let mut file = std::fs::File::open(path).map_err(|e| AppError::Incremental(e.to_string()))?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 8192];
    loop {
        let n = file
            .read(&mut buffer)
            .map_err(|e| AppError::Incremental(e.to_string()))?;
        if n == 0 {
            break;
        }
        hasher.update(&buffer[..n]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

#[cfg(feature = "incremental")]
fn file_mtime_secs(path: &Path) -> AppResult<u64> {
    let meta = std::fs::metadata(path).map_err(|e| AppError::Incremental(e.to_string()))?;
    let mtime = meta
        .modified()
        .map_err(|e| AppError::Incremental(e.to_string()))?;
    Ok(mtime
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs())
}
