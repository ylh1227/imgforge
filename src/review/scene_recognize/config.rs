//! 场景识别配置（API Key 存钥匙串 / 环境变量，不进本文件）。

use serde::{Deserialize, Serialize};

use crate::review::error::ReviewResult;
use crate::review::scene_recognize::secret;
use crate::review::storage::paths::app_data_dir;

pub const VISION_API_KEY_ENV: &str = "IMGFORGE_VISION_API_KEY";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SceneRecognizeConfig {
    #[serde(default)]
    pub enabled: bool,
    /// OpenAI 兼容根地址（默认阿里云百炼 compatible-mode）。
    #[serde(default = "default_base_url")]
    pub base_url: String,
    #[serde(default = "default_model")]
    pub model: String,
    #[serde(default = "default_timeout")]
    pub timeout_secs: u64,
    #[serde(default)]
    pub auto_on_import: bool,
    /// 未匹配时是否加 `未识别_` 前缀（默认否）。
    #[serde(default)]
    pub prefix_unknown: bool,
    #[serde(default = "default_concurrency")]
    pub concurrency: usize,
    /// JPEG 边长上限（识别用缩略图）。
    #[serde(default = "default_max_edge")]
    pub max_edge: u32,
    /// 是否启用模型「深度思考」（Thinking 模型默认很慢；场景分类建议关闭）。
    #[serde(default)]
    pub enable_thinking: bool,
    /// 思考 token 上限（仅部分模型有效；0 表示不传）。
    #[serde(default = "default_thinking_budget")]
    pub thinking_budget: u32,
}

fn default_base_url() -> String {
    "https://dashscope.aliyuncs.com/compatible-mode/v1".into()
}

fn default_model() -> String {
    // 百炼视觉理解推荐轻量档：https://help.aliyun.com/zh/model-studio/vision-model
    "qwen3.7-flash".into()
}

fn default_timeout() -> u64 {
    60
}

fn default_concurrency() -> usize {
    4
}

fn default_max_edge() -> u32 {
    640
}

fn default_thinking_budget() -> u32 {
    32
}

impl Default for SceneRecognizeConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            base_url: default_base_url(),
            model: default_model(),
            timeout_secs: default_timeout(),
            auto_on_import: false,
            prefix_unknown: false,
            concurrency: default_concurrency(),
            max_edge: default_max_edge(),
            enable_thinking: false,
            thinking_budget: default_thinking_budget(),
        }
    }
}

impl SceneRecognizeConfig {
    pub fn path() -> ReviewResult<std::path::PathBuf> {
        Ok(app_data_dir()?.join("scene_recognize_config.json"))
    }

    pub fn load() -> ReviewResult<Self> {
        let path = Self::path()?;
        if !path.exists() {
            return Ok(Self::default());
        }
        let text = std::fs::read_to_string(&path)?;
        Ok(serde_json::from_str(&text)?)
    }

    pub fn save(&self) -> ReviewResult<()> {
        let path = Self::path()?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&path, serde_json::to_string_pretty(self)?)?;
        Ok(())
    }

    /// 解析 API Key（钥匙串优先，其次环境变量）。
    pub fn resolve_api_key() -> Option<String> {
        secret::resolve_api_key()
    }

    /// 兼容旧调用名。
    pub fn api_key_from_env() -> Option<String> {
        Self::resolve_api_key()
    }

    pub fn has_api_key() -> bool {
        secret::has_api_key()
    }
}
