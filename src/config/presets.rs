//! 内置处理预设。

use crate::config::app_config::AppConfig;
use crate::core::types::{ImageFormat, Quality, ResizeOptions};

/// 预设名称。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Preset {
    WebOptimized,
    MinimalSize,
    PrintQuality,
}

impl Preset {
    pub fn parse(name: &str) -> Option<Self> {
        match name.to_ascii_lowercase().as_str() {
            "web" | "web-optimized" | "web_optimized" => Some(Self::WebOptimized),
            "minimal" | "minimal-size" | "minimal_size" => Some(Self::MinimalSize),
            "print" | "print-quality" | "print_quality" => Some(Self::PrintQuality),
            _ => None,
        }
    }

    /// 将预设参数合并到配置（不覆盖已显式设置的字段由 loader 负责）。
    pub fn apply(self, config: &mut AppConfig) {
        match self {
            Self::WebOptimized => {
                config.target_format = ImageFormat::WebP;
                config.quality = Quality::new(82).expect("valid quality");
                config.resize = ResizeOptions {
                    width: Some(1920),
                    height: Some(1080),
                    mode: crate::core::types::ResizeMode::Fit,
                };
                config.metadata_policy = crate::core::types::MetadataPolicy::Strip;
            }
            Self::MinimalSize => {
                config.target_format = ImageFormat::WebP;
                config.quality = Quality::new(60).expect("valid quality");
                config.resize = ResizeOptions {
                    width: Some(1280),
                    height: None,
                    mode: crate::core::types::ResizeMode::Fit,
                };
                config.metadata_policy = crate::core::types::MetadataPolicy::Strip;
            }
            Self::PrintQuality => {
                config.target_format = ImageFormat::Jpeg;
                config.quality = Quality::new(95).expect("valid quality");
                config.resize = ResizeOptions {
                    width: None,
                    height: None,
                    mode: crate::core::types::ResizeMode::Fit,
                };
                config.metadata_policy = crate::core::types::MetadataPolicy::Preserve;
            }
        }
    }
}
