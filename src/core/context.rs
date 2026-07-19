//! 图像处理上下文，作为流水线唯一数据载体。

use std::path::PathBuf;

use image::DynamicImage;

use crate::core::types::{
    AdjustOptions, BrightnessMatchOptions, ImageFormat, MetadataPolicy, Quality, ResizeOptions,
    Transform, WatermarkOptions,
};
use crate::processing::brightness_match::BrightnessMatchCache;

/// 流水线各步骤共享的图像处理上下文。
#[derive(Debug)]
pub struct ImageContext {
    pub source_path: PathBuf,
    pub output_path: PathBuf,
    pub source_format: Option<ImageFormat>,
    pub target_format: ImageFormat,
    pub quality: Quality,
    pub raw_bytes: Option<Vec<u8>>,
    pub image: Option<DynamicImage>,
    pub encoded_bytes: Option<Vec<u8>>,
    pub exif_bytes: Option<Vec<u8>>,
    pub resize: ResizeOptions,
    pub adjust: AdjustOptions,
    pub brightness_match: BrightnessMatchOptions,
    pub brightness_match_cache: Option<BrightnessMatchCache>,
    pub metadata_policy: MetadataPolicy,
    pub transform: Transform,
    pub watermark: WatermarkOptions,
    pub source_size: u64,
    pub output_size: u64,
    pub dry_run: bool,
    pub bayer_only: bool,
}

impl ImageContext {
    pub fn new(
        source_path: PathBuf,
        output_path: PathBuf,
        target_format: ImageFormat,
        quality: Quality,
        source_size: u64,
    ) -> Self {
        Self {
            source_path,
            output_path,
            source_format: None,
            target_format,
            quality,
            raw_bytes: None,
            image: None,
            encoded_bytes: None,
            exif_bytes: None,
            resize: ResizeOptions {
                width: None,
                height: None,
                mode: crate::core::types::ResizeMode::Fit,
            },
            adjust: AdjustOptions::default(),
            brightness_match: BrightnessMatchOptions::default(),
            brightness_match_cache: None,
            metadata_policy: MetadataPolicy::Preserve,
            transform: Transform::None,
            watermark: WatermarkOptions::default(),
            source_size,
            output_size: 0,
            dry_run: false,
            bayer_only: false,
        }
    }
}
