//! 单图转换参数（评审页标记，加入转换队列时带入）。

use serde::{Deserialize, Serialize};

use crate::core::types::ImageFormat;

/// 评审页为单张图片标记的转换参数（可选，默认不改变全局配置）。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ConvertParams {
    pub format: Option<ImageFormat>,
    pub quality: Option<u8>,
    pub width: Option<u32>,
}

impl ConvertParams {
    pub fn is_empty(&self) -> bool {
        self.format.is_none() && self.quality.is_none() && self.width.is_none()
    }
}
