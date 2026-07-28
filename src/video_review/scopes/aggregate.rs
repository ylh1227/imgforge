//! 整段/片段示波器时间聚合：自适应抽帧 + bins 累加。

use image::{imageops::FilterType, RgbaImage};

use super::histogram::{self, HistogramBins, HistogramScale};
use super::vectorscope::{self, VectorscopeData, VECTOR_SIZE};
use super::waveform::{self, WaveformData, WaveformMode};
use super::{ScopeKind, ScopeOptions};

pub const AGG_FRAME_WIDTH: u32 = 960;
pub const WAVEFORM_FIXED_WIDTH: u32 = 640;
pub const MIN_SAMPLES: usize = 16;
pub const MAX_SAMPLES: usize = 160;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AggregateRange {
    pub start_ms: u64,
    pub end_ms: u64,
}

impl AggregateRange {
    pub fn full(duration_ms: u64) -> Self {
        Self {
            start_ms: 0,
            end_ms: duration_ms.max(1),
        }
    }

    pub fn clamped(self, duration_ms: u64) -> Self {
        let end_cap = duration_ms.max(1);
        let start = self.start_ms.min(end_cap.saturating_sub(1));
        let end = self.end_ms.clamp(start + 1, end_cap);
        Self {
            start_ms: start,
            end_ms: end,
        }
    }

    pub fn is_valid(self) -> bool {
        self.end_ms > self.start_ms
    }

    pub fn duration_ms(self) -> u64 {
        self.end_ms.saturating_sub(self.start_ms)
    }
}

/// 按时长自适应采样点数（含两端），夹在 `[MIN_SAMPLES, MAX_SAMPLES]`。
pub fn sample_count_for_range(range_ms: u64) -> usize {
    let secs = (range_ms as f64 / 1000.0).max(0.001);
    // 约每 1.25 秒一帧，提高时间维有效覆盖。
    let n = (secs / 1.25).ceil() as usize;
    n.clamp(MIN_SAMPLES, MAX_SAMPLES)
}

/// 在 `[start, end]` 上均匀取点（含两端）。
pub fn sample_timestamps(range: AggregateRange) -> Vec<u64> {
    let range = if range.is_valid() {
        range
    } else {
        return vec![range.start_ms];
    };
    let n = sample_count_for_range(range.duration_ms());
    if n <= 1 {
        return vec![range.start_ms];
    }
    let mut out = Vec::with_capacity(n);
    let span = range.duration_ms();
    for i in 0..n {
        let t = range.start_ms + span * i as u64 / (n as u64 - 1);
        out.push(t.min(range.end_ms));
    }
    out
}

#[derive(Debug, Clone)]
pub struct AggregatedScope {
    pub kind: ScopeKind,
    pub sample_count: usize,
    pub skipped: usize,
    pub range: AggregateRange,
    pub data_w: u32,
    pub data_h: u32,
    /// 归一化后的单通道强度/高度图，供 GPU。
    pub pixels: Vec<u8>,
}

pub struct AggregateAccumulator {
    kind: ScopeKind,
    options: ScopeOptions,
    hist: Option<HistogramBins>,
    wave: Option<WaveformData>,
    vect: Option<VectorscopeData>,
    sample_count: usize,
    skipped: usize,
}

impl AggregateAccumulator {
    pub fn new(kind: ScopeKind, options: ScopeOptions) -> Self {
        Self {
            kind,
            options,
            hist: None,
            wave: None,
            vect: None,
            sample_count: 0,
            skipped: 0,
        }
    }

    pub fn note_skip(&mut self) {
        self.skipped += 1;
    }

    pub fn push_frame(&mut self, img: &RgbaImage) {
        match self.kind {
            ScopeKind::Histogram => {
                let bins = histogram::analyze(img);
                match &mut self.hist {
                    Some(acc) => add_histogram(acc, &bins),
                    None => self.hist = Some(bins),
                }
            }
            ScopeKind::Waveform => {
                let resized = resize_to_width(img, WAVEFORM_FIXED_WIDTH);
                let data = waveform::analyze(&resized, self.options.waveform_mode);
                match &mut self.wave {
                    Some(acc) => add_waveform(acc, &data),
                    None => self.wave = Some(data),
                }
            }
            ScopeKind::Vectorscope => {
                let data = vectorscope::analyze(img);
                match &mut self.vect {
                    Some(acc) => add_vectorscope(acc, &data),
                    None => self.vect = Some(data),
                }
            }
        }
        self.sample_count += 1;
    }

    pub fn finish(self, range: AggregateRange) -> Result<AggregatedScope, String> {
        if self.sample_count == 0 {
            return Err("聚合失败：没有成功分析的帧".into());
        }
        let (data_w, data_h, pixels) = match self.kind {
            ScopeKind::Histogram => {
                let bins = self.hist.ok_or("直方图累加为空")?;
                let map = histogram::bins_to_height_map(&bins, self.options.histogram_scale);
                (256u32, 4u32, map.to_vec())
            }
            ScopeKind::Waveform => {
                let data = self.wave.ok_or("波形累加为空")?;
                let map = waveform::to_intensity_map(&data);
                (data.width, 256u32, map)
            }
            ScopeKind::Vectorscope => {
                let data = self.vect.ok_or("矢量累加为空")?;
                let map = vectorscope::to_intensity_map(&data);
                (VECTOR_SIZE, VECTOR_SIZE, map)
            }
        };
        Ok(AggregatedScope {
            kind: self.kind,
            sample_count: self.sample_count,
            skipped: self.skipped,
            range,
            data_w,
            data_h,
            pixels,
        })
    }

    pub fn sample_count(&self) -> usize {
        self.sample_count
    }

    pub fn skipped(&self) -> usize {
        self.skipped
    }
}

fn add_histogram(dst: &mut HistogramBins, src: &HistogramBins) {
    for i in 0..256 {
        dst.y[i] = dst.y[i].saturating_add(src.y[i]);
        dst.r[i] = dst.r[i].saturating_add(src.r[i]);
        dst.g[i] = dst.g[i].saturating_add(src.g[i]);
        dst.b[i] = dst.b[i].saturating_add(src.b[i]);
    }
}

fn add_waveform(dst: &mut WaveformData, src: &WaveformData) {
    if dst.width != src.width || dst.bins.len() != src.bins.len() {
        return;
    }
    for (d, s) in dst.bins.iter_mut().zip(src.bins.iter()) {
        *d = d.saturating_add(*s);
    }
}

fn add_vectorscope(dst: &mut VectorscopeData, src: &VectorscopeData) {
    if dst.density.len() != src.density.len() {
        return;
    }
    for (d, s) in dst.density.iter_mut().zip(src.density.iter()) {
        *d = d.saturating_add(*s);
    }
}

fn resize_to_width(img: &RgbaImage, width: u32) -> RgbaImage {
    let w = img.width().max(1);
    let h = img.height().max(1);
    if w == width {
        return img.clone();
    }
    let nh = ((h as f32 * width as f32 / w as f32).round() as u32).max(1);
    image::imageops::resize(img, width, nh, FilterType::CatmullRom)
}

/// 供测试/引擎复用的模式编码。
pub fn scope_mode_uniforms(kind: ScopeKind, options: &ScopeOptions) -> (u32, u32, u32) {
    match kind {
        ScopeKind::Histogram => {
            let mode = options.histogram_mode as u32;
            let scale = match options.histogram_scale {
                HistogramScale::Linear => 0u32,
                HistogramScale::Log => 1,
            };
            (0, mode, scale)
        }
        ScopeKind::Waveform => {
            let mode = match options.waveform_mode {
                WaveformMode::Luma => 0u32,
                WaveformMode::RgbParade => 1,
            };
            (1, mode, 0)
        }
        ScopeKind::Vectorscope => (2, 0, 0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{ImageBuffer, Rgba};

    #[test]
    fn sample_timestamps_includes_ends() {
        let range = AggregateRange {
            start_ms: 1000,
            end_ms: 5000,
        };
        let ts = sample_timestamps(range);
        assert!(ts.len() >= MIN_SAMPLES);
        assert_eq!(*ts.first().unwrap(), 1000);
        assert_eq!(*ts.last().unwrap(), 5000);
    }

    #[test]
    fn sample_count_clamped() {
        assert_eq!(sample_count_for_range(100), MIN_SAMPLES);
        assert_eq!(sample_count_for_range(1_000_000), MAX_SAMPLES);
    }

    #[test]
    fn histogram_black_and_white_peaks() {
        let black = ImageBuffer::from_pixel(4, 4, Rgba([0u8, 0, 0, 255]));
        let white = ImageBuffer::from_pixel(4, 4, Rgba([255u8, 255, 255, 255]));
        let mut acc = AggregateAccumulator::new(ScopeKind::Histogram, ScopeOptions::default());
        acc.push_frame(&black);
        acc.push_frame(&white);
        let range = AggregateRange {
            start_ms: 0,
            end_ms: 1000,
        };
        let out = acc.finish(range).unwrap();
        assert_eq!(out.sample_count, 2);
        // pixels are normalized heights; just ensure non-empty and dims.
        assert_eq!(out.pixels.len(), 256 * 4);
        let bins_black = histogram::analyze(&black);
        let bins_white = histogram::analyze(&white);
        let mut sum = HistogramBins::default();
        add_histogram(&mut sum, &bins_black);
        add_histogram(&mut sum, &bins_white);
        assert!(sum.y[0] > 0);
        assert!(sum.y[255] > 0);
    }

    #[test]
    fn waveform_accumulates_counts() {
        let img = ImageBuffer::from_pixel(8, 4, Rgba([255u8, 255, 255, 255]));
        let mut acc = AggregateAccumulator::new(ScopeKind::Waveform, ScopeOptions::default());
        acc.push_frame(&img);
        acc.push_frame(&img);
        let one = waveform::analyze(
            &resize_to_width(&img, WAVEFORM_FIXED_WIDTH),
            WaveformMode::Luma,
        );
        let range = AggregateRange {
            start_ms: 0,
            end_ms: 500,
        };
        let finished = acc.finish(range).unwrap();
        assert_eq!(finished.sample_count, 2);
        // Reconstruct raw sum via fresh accumulator internals check:
        let mut raw = one.clone();
        add_waveform(&mut raw, &one);
        assert_eq!(raw.bins.iter().sum::<u32>(), one.bins.iter().sum::<u32>() * 2);
    }
}
