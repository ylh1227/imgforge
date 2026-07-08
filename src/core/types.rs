//! 类型安全 newtype 定义，将参数合法性前置到编译期或构造期。

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

use crate::core::error::{AppError, AppResult};

/// 图像输出质量，范围 1–100。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "u8", into = "u8")]
pub struct Quality(u8);

impl Quality {
    pub const MIN: u8 = 1;
    pub const MAX: u8 = 100;
    pub const DEFAULT: Self = Self(85);
    pub const LOSSLESS: Self = Self(100);

    /// 编译期安全构造；超出范围返回 `None`。
    pub const fn try_new(value: u8) -> Option<Self> {
        if value >= Self::MIN && value <= Self::MAX {
            Some(Self(value))
        } else {
            None
        }
    }

    /// 运行时安全构造。
    pub fn new(value: u8) -> AppResult<Self> {
        Self::try_new(value).ok_or(AppError::InvalidQuality(value))
    }

    pub const fn value(self) -> u8 {
        self.0
    }

    pub const fn is_lossless(self) -> bool {
        self.0 == Self::MAX
    }
}

impl TryFrom<u8> for Quality {
    type Error = AppError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl From<Quality> for u8 {
    fn from(q: Quality) -> Self {
        q.0
    }
}

/// 并发度 newtype，至少为 1。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "usize", into = "usize")]
pub struct Concurrency(usize);

impl Concurrency {
    pub fn new(value: usize) -> AppResult<Self> {
        if value < 1 {
            return Err(AppError::InvalidConcurrency(value));
        }
        Ok(Self(value))
    }

    pub fn default_parallel() -> Self {
        Self(num_cpus::get().max(1))
    }

    pub const fn value(self) -> usize {
        self.0
    }
}

impl TryFrom<usize> for Concurrency {
    type Error = AppError;

    fn try_from(value: usize) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl From<Concurrency> for usize {
    fn from(c: Concurrency) -> Self {
        c.0
    }
}

/// 支持的图像格式。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ImageFormat {
    Jpeg,
    Png,
    WebP,
    Bmp,
    Tiff,
    Gif,
    #[cfg(feature = "avif")]
    Avif,
    #[cfg(feature = "jpegxl")]
    JpegXl,
}

impl ImageFormat {
    pub fn extension(self) -> &'static str {
        match self {
            Self::Jpeg => "jpg",
            Self::Png => "png",
            Self::WebP => "webp",
            Self::Bmp => "bmp",
            Self::Tiff => "tiff",
            Self::Gif => "gif",
            #[cfg(feature = "avif")]
            Self::Avif => "avif",
            #[cfg(feature = "jpegxl")]
            Self::JpegXl => "jxl",
        }
    }

    pub fn mime_type(self) -> &'static str {
        match self {
            Self::Jpeg => "image/jpeg",
            Self::Png => "image/png",
            Self::WebP => "image/webp",
            Self::Bmp => "image/bmp",
            Self::Tiff => "image/tiff",
            Self::Gif => "image/gif",
            #[cfg(feature = "avif")]
            Self::Avif => "image/avif",
            #[cfg(feature = "jpegxl")]
            Self::JpegXl => "image/jxl",
        }
    }

    pub fn from_extension(ext: &str) -> Option<Self> {
        match ext.to_ascii_lowercase().as_str() {
            "jpg" | "jpeg" => Some(Self::Jpeg),
            "png" => Some(Self::Png),
            "webp" => Some(Self::WebP),
            "bmp" => Some(Self::Bmp),
            "tiff" | "tif" => Some(Self::Tiff),
            "gif" => Some(Self::Gif),
            #[cfg(feature = "avif")]
            "avif" => Some(Self::Avif),
            #[cfg(feature = "jpegxl")]
            "jxl" => Some(Self::JpegXl),
            _ => None,
        }
    }

    pub fn all_supported() -> Vec<Self> {
        let mut formats = vec![
            Self::Jpeg,
            Self::Png,
            Self::WebP,
            Self::Bmp,
            Self::Tiff,
            Self::Gif,
        ];
        #[cfg(feature = "avif")]
        formats.push(Self::Avif);
        #[cfg(feature = "jpegxl")]
        formats.push(Self::JpegXl);
        formats
    }
}

impl fmt::Display for ImageFormat {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.extension())
    }
}

impl FromStr for ImageFormat {
    type Err = AppError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::from_extension(s).ok_or_else(|| AppError::UnsupportedFormat(s.to_string()))
    }
}

impl clap::ValueEnum for ImageFormat {
    fn value_variants<'a>() -> &'a [Self] {
        &[
            Self::Jpeg,
            Self::Png,
            Self::WebP,
            Self::Bmp,
            Self::Tiff,
            Self::Gif,
            #[cfg(feature = "avif")]
            Self::Avif,
            #[cfg(feature = "jpegxl")]
            Self::JpegXl,
        ]
    }

    fn to_possible_value(&self) -> Option<clap::builder::PossibleValue> {
        Some(clap::builder::PossibleValue::new(self.extension()))
    }
}

/// 旋转变换。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default, clap::ValueEnum)]
#[serde(rename_all = "snake_case")]
pub enum Transform {
    #[default]
    None,
    Rotate90,
    Rotate180,
    Rotate270,
    FlipHorizontal,
    FlipVertical,
}

impl Transform {
    pub fn from_cli_name(name: &str) -> Option<Self> {
        match name.to_ascii_lowercase().as_str() {
            "none" => Some(Self::None),
            "rotate90" | "rotate-90" | "90" => Some(Self::Rotate90),
            "rotate180" | "rotate-180" | "180" => Some(Self::Rotate180),
            "rotate270" | "rotate-270" | "270" => Some(Self::Rotate270),
            "flip_h" | "flip-h" | "fliph" | "horizontal" => Some(Self::FlipHorizontal),
            "flip_v" | "flip-v" | "flipv" | "vertical" => Some(Self::FlipVertical),
            _ => None,
        }
    }
}

/// 缩放模式。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default, clap::ValueEnum)]
#[serde(rename_all = "snake_case")]
pub enum ResizeMode {
    #[default]
    Fit,
    Fill,
    Exact,
}

/// 缩放参数。
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct ResizeOptions {
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub mode: ResizeMode,
}

impl ResizeOptions {
    pub fn is_active(self) -> bool {
        self.width.is_some() || self.height.is_some()
    }
}

/// 画质调整参数。
#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default)]
pub struct AdjustOptions {
    pub brightness: f32,
    pub contrast: f32,
    pub sharpen: f32,
}

impl AdjustOptions {
    pub fn is_active(self) -> bool {
        self.brightness != 0.0 || self.contrast != 0.0 || self.sharpen > 0.0
    }
}

/// 元数据处理策略。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum MetadataPolicy {
    #[default]
    Preserve,
    Strip,
}

/// 水印位置。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default, clap::ValueEnum)]
#[serde(rename_all = "snake_case")]
pub enum WatermarkPosition {
    TopLeft,
    TopRight,
    #[default]
    BottomRight,
    BottomLeft,
    Center,
}

/// 水印配置。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WatermarkOptions {
    pub text: Option<String>,
    pub image_path: Option<std::path::PathBuf>,
    pub font_path: Option<std::path::PathBuf>,
    #[serde(default = "default_opacity")]
    pub opacity: f32,
    #[serde(default)]
    pub position: WatermarkPosition,
    #[serde(default = "default_margin")]
    pub margin: u32,
    #[serde(default = "default_font_size")]
    pub font_size: f32,
}

fn default_opacity() -> f32 {
    0.5
}
fn default_margin() -> u32 {
    16
}
fn default_font_size() -> f32 {
    24.0
}

impl Default for WatermarkOptions {
    fn default() -> Self {
        Self {
            text: None,
            image_path: None,
            font_path: None,
            opacity: default_opacity(),
            position: WatermarkPosition::default(),
            margin: default_margin(),
            font_size: default_font_size(),
        }
    }
}

impl WatermarkOptions {
    pub fn is_active(&self) -> bool {
        self.text.is_some() || self.image_path.is_some()
    }
}

/// 缩略图规格。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThumbnailSpec {
    pub width: u32,
    pub height: Option<u32>,
    /// 输出文件名后缀，如 "_sm"。
    pub suffix: String,
}

/// 解析缩略图尺寸字符串，如 "256" 或 "512x384"。
pub fn parse_thumbnail_spec(spec: &str) -> AppResult<ThumbnailSpec> {
    let spec = spec.trim();
    if spec.is_empty() {
        return Err(AppError::Config("empty thumbnail spec".into()));
    }
    if let Some((w, h)) = spec.split_once('x') {
        let width: u32 = w
            .parse()
            .map_err(|_| AppError::Config(format!("invalid thumbnail width: {w}")))?;
        let height: u32 = h
            .parse()
            .map_err(|_| AppError::Config(format!("invalid thumbnail height: {h}")))?;
        return Ok(ThumbnailSpec {
            width,
            height: Some(height),
            suffix: format!("_{width}x{height}"),
        });
    }
    let width: u32 = spec
        .parse()
        .map_err(|_| AppError::Config(format!("invalid thumbnail spec: {spec}")))?;
    Ok(ThumbnailSpec {
        width,
        height: None,
        suffix: format!("_{width}"),
    })
}
