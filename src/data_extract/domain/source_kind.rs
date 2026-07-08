//! 数据来源类型。

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum SourceKind {
    #[default]
    StructuredFile,
    TextFile,
    HtmlFile,
    OcrImage,
}

impl SourceKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::StructuredFile => "结构化文件",
            Self::TextFile => "文本",
            Self::HtmlFile => "HTML",
            Self::OcrImage => "OCR 图片",
        }
    }
}
