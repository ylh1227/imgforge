//! OCR 元数据。

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OcrMetadata {
    pub engine: String,
    pub language: String,
    pub confidence: Option<f32>,
    pub text_cache_path: Option<PathBuf>,
}

impl OcrMetadata {
    pub fn new(engine: impl Into<String>, language: impl Into<String>) -> Self {
        Self {
            engine: engine.into(),
            language: language.into(),
            confidence: None,
            text_cache_path: None,
        }
    }

    pub fn with_confidence(mut self, confidence: f32) -> Self {
        self.confidence = Some(confidence);
        self
    }

    pub fn with_text_cache(mut self, path: PathBuf) -> Self {
        self.text_cache_path = Some(path);
        self
    }
}
