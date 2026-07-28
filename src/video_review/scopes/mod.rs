//! 视频示波器：Histogram / Waveform / Vectorscope（wgpu 离屏）。

mod aggregate;
mod color;
mod engine;
mod histogram;
mod vectorscope;
mod waveform;

pub use aggregate::{
    sample_count_for_range, sample_timestamps, scope_mode_uniforms, AggregateAccumulator,
    AggregateRange, AggregatedScope, AGG_FRAME_WIDTH, MAX_SAMPLES, MIN_SAMPLES,
};
pub use color::{luma709, luma709_f32, rgb_to_cb_cr};
pub use engine::ScopeEngine;
pub use histogram::{HistogramMode, HistogramScale};
pub use waveform::WaveformMode;

use image::RgbaImage;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ScopeKind {
    Histogram,
    Waveform,
    Vectorscope,
}

impl ScopeKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Histogram => "Histogram",
            Self::Waveform => "Waveform",
            Self::Vectorscope => "Vectorscope",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScopeOptions {
    pub histogram_mode: HistogramMode,
    pub histogram_scale: HistogramScale,
    pub waveform_mode: WaveformMode,
    pub vectorscope_75_box: bool,
}

impl Default for ScopeOptions {
    fn default() -> Self {
        Self {
            histogram_mode: HistogramMode::Parade,
            histogram_scale: HistogramScale::Linear,
            waveform_mode: WaveformMode::Luma,
            vectorscope_75_box: true,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ScopeRequest<'a> {
    pub kind: ScopeKind,
    pub rgba: &'a RgbaImage,
    pub out_width: u32,
    pub out_height: u32,
    /// 分析前最长边上限（拖动时间轴时可降低）。
    pub max_input_edge: u32,
    pub options: ScopeOptions,
}

#[derive(Debug, Clone)]
pub struct ScopeRgba {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

/// 缓存键（面板侧使用）。
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ScopeCacheKey {
    pub video_id: i64,
    pub time_ms: u64,
    pub kind: ScopeKind,
    pub histogram_mode: u8,
    pub histogram_scale: u8,
    pub waveform_mode: u8,
    pub vectorscope_75_box: bool,
    pub out_w: u32,
    pub out_h: u32,
    pub frame_path: String,
}

impl ScopeCacheKey {
    pub fn from_parts(
        video_id: i64,
        time_ms: u64,
        kind: ScopeKind,
        options: &ScopeOptions,
        out_w: u32,
        out_h: u32,
        frame_path: &str,
    ) -> Self {
        Self {
            video_id,
            time_ms,
            kind,
            histogram_mode: options.histogram_mode as u8,
            histogram_scale: options.histogram_scale as u8,
            waveform_mode: options.waveform_mode as u8,
            vectorscope_75_box: options.vectorscope_75_box,
            out_w,
            out_h,
            frame_path: frame_path.to_string(),
        }
    }
}

/// 聚合视图缓存键。
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct AggregateCacheKey {
    pub video_id: i64,
    pub start_ms: u64,
    pub end_ms: u64,
    pub kind: ScopeKind,
    pub histogram_mode: u8,
    pub histogram_scale: u8,
    pub waveform_mode: u8,
    pub vectorscope_75_box: bool,
    pub sample_n: usize,
    pub out_w: u32,
    pub out_h: u32,
}

impl AggregateCacheKey {
    pub fn from_parts(
        video_id: i64,
        range: AggregateRange,
        kind: ScopeKind,
        options: &ScopeOptions,
        sample_n: usize,
        out_w: u32,
        out_h: u32,
    ) -> Self {
        Self {
            video_id,
            start_ms: range.start_ms,
            end_ms: range.end_ms,
            kind,
            histogram_mode: options.histogram_mode as u8,
            histogram_scale: options.histogram_scale as u8,
            waveform_mode: options.waveform_mode as u8,
            vectorscope_75_box: options.vectorscope_75_box,
            sample_n,
            out_w,
            out_h,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ScopeViewMode {
    #[default]
    Current,
    Aggregate,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AggregateRangeMode {
    #[default]
    Full,
    InOut,
}
