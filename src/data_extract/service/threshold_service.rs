//! 阈值配置路径与持久化。

use std::fs;
use std::path::PathBuf;

use crate::data_extract::domain::ThresholdProfile;
use crate::data_extract::error::{DataExtractError, DataExtractResult};

pub fn thresholds_path() -> PathBuf {
    #[cfg(target_os = "windows")]
    {
        let base = std::env::var("APPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("."));
        base.join("imgforge").join("imatest_thresholds.json")
    }
    #[cfg(not(target_os = "windows"))]
    {
        std::env::var("HOME")
            .map(|h| {
                PathBuf::from(h)
                    .join(".imgforge")
                    .join("imatest_thresholds.json")
            })
            .unwrap_or_else(|_| PathBuf::from("imatest_thresholds.json"))
    }
}

pub struct ThresholdService;

impl ThresholdService {
    pub fn load() -> DataExtractResult<ThresholdProfile> {
        let path = thresholds_path();
        if !path.exists() {
            return Ok(ThresholdProfile::default_rules());
        }
        let text = fs::read_to_string(&path)?;
        serde_json::from_str(&text).map_err(DataExtractError::Json)
    }

    pub fn save(profile: &ThresholdProfile) -> DataExtractResult<()> {
        let path = thresholds_path();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_string_pretty(profile)?;
        fs::write(path, json)?;
        Ok(())
    }

    pub fn load_or_default() -> ThresholdProfile {
        Self::load().unwrap_or_else(|_| ThresholdProfile::default_rules())
    }
}
