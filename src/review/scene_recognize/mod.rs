//! 基于场景列表的云端视觉识别与文件前缀命名。

mod catalog;
mod client;
mod config;
mod naming;
mod secret;
mod service;

pub use catalog::{SceneCatalog, SceneSpec};
pub use client::{SceneMatch, VisionSceneClient};
pub use config::SceneRecognizeConfig;
pub use naming::{
    apply_scene_prefix, build_prefixed_filename, sanitize_scene_name, strip_known_scene_prefix,
};
pub use secret::{clear_api_key, has_api_key, has_keychain_api_key, store_api_key};
pub use service::{recognize_and_rename_batch, RecognizeBatchReport, RecognizeItemResult};
