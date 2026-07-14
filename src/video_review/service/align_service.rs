//! 多视频高精度帧对齐：音频互相关（目标 ±1–2ms）+ 按帧步进。

use std::path::Path;
use std::process::Command;

use rustfft::{num_complex::Complex, FftPlanner};
use tempfile::TempDir;

use crate::video_review::domain::VideoItem;
use crate::video_review::error::{VideoReviewError, VideoReviewResult};
use crate::video_review::service::ffmpeg_backend::FfmpegBackend;

/// 默认分析片段时长（秒）。
pub const DEFAULT_ALIGN_SECONDS: f32 = 30.0;
/// 互相关最大搜索窗口（毫秒），超出视为不可靠。
const MAX_LAG_MS: i64 = 15_000;
const SAMPLE_RATE: u32 = 16_000;

#[derive(Debug, Clone)]
pub struct AlignPairResult {
    pub video_id: i64,
    /// 相对主视频的偏移（毫秒）；正值表示该路比主视频晚开始，应把 offset 设为该值。
    pub offset_ms: i64,
    pub confidence: f32,
}

#[derive(Debug, Clone)]
pub struct AlignBatchResult {
    pub reference_id: i64,
    pub pairs: Vec<AlignPairResult>,
}

pub struct AlignService {
    ffmpeg_path: String,
}

impl AlignService {
    pub fn new(ffmpeg_path: impl Into<String>) -> Self {
        Self {
            ffmpeg_path: ffmpeg_path.into(),
        }
    }

    pub fn with_backend(backend: &FfmpegBackend) -> Self {
        Self::new(backend.ffmpeg_path())
    }

    /// 将各路对齐到 `reference`（主视频）。
    /// `around_ms`：以该时间为中心截取分析窗；`None` 则从片头开始。
    pub fn align_to_reference(
        &self,
        reference: &VideoItem,
        others: &[VideoItem],
        analysis_secs: f32,
        around_ms: Option<u64>,
    ) -> VideoReviewResult<AlignBatchResult> {
        let secs = analysis_secs.clamp(5.0, 90.0);
        let tmp = TempDir::new().map_err(|e| VideoReviewError::Message(e.to_string()))?;
        let ref_pcm = tmp.path().join("ref.f32");
        self.extract_mono_f32(&reference.file_path, &ref_pcm, secs, around_ms)?;
        let ref_samples = read_f32_pcm(&ref_pcm)?;
        if ref_samples.len() < SAMPLE_RATE as usize {
            return Err(VideoReviewError::Message(
                "主视频可用音频过短，无法自动对齐".into(),
            ));
        }

        let mut pairs = Vec::with_capacity(others.len() + 1);
        pairs.push(AlignPairResult {
            video_id: reference.id,
            offset_ms: 0,
            confidence: 1.0,
        });

        for video in others {
            if video.id == reference.id {
                continue;
            }
            let pcm_path = tmp.path().join(format!("v{}.f32", video.id));
            match self.extract_mono_f32(&video.file_path, &pcm_path, secs, around_ms) {
                Ok(()) => {}
                Err(e) => {
                    pairs.push(AlignPairResult {
                        video_id: video.id,
                        offset_ms: video.offset_ms,
                        confidence: 0.0,
                    });
                    tracing::warn!(
                        video_id = video.id,
                        error = %e,
                        "音频提取失败，跳过自动对齐"
                    );
                    continue;
                }
            }
            let samples = match read_f32_pcm(&pcm_path) {
                Ok(s) if s.len() >= SAMPLE_RATE as usize / 2 => s,
                _ => {
                    pairs.push(AlignPairResult {
                        video_id: video.id,
                        offset_ms: video.offset_ms,
                        confidence: 0.0,
                    });
                    continue;
                }
            };
            let (lag_samples, confidence) = cross_correlate_lag(&ref_samples, &samples);
            let mut offset_ms =
                ((lag_samples as f64) * 1000.0 / f64::from(SAMPLE_RATE)).round() as i64;
            let mut confidence = confidence;
            if offset_ms.abs() > MAX_LAG_MS {
                confidence = 0.0;
                offset_ms = video.offset_ms;
            } else {
                offset_ms = quantize_offset_to_fps(offset_ms, video.fps);
            }
            pairs.push(AlignPairResult {
                video_id: video.id,
                offset_ms,
                confidence,
            });
        }

        Ok(AlignBatchResult {
            reference_id: reference.id,
            pairs,
        })
    }

    fn extract_mono_f32(
        &self,
        video: &Path,
        dest: &Path,
        seconds: f32,
        around_ms: Option<u64>,
    ) -> VideoReviewResult<()> {
        let half = (seconds * 500.0) as u64;
        let start_secs = around_ms
            .map(|t| t.saturating_sub(half) as f64 / 1000.0)
            .unwrap_or(0.0);

        let mut cmd = crate::process_util::command(&self.ffmpeg_path);
        cmd.args(["-hide_banner", "-loglevel", "error"]);
        if start_secs > 0.01 {
            // 输入侧粗定位，再精确截取时长
            cmd.args(["-ss", &format!("{start_secs:.3}")]);
        }
        cmd.args([
            "-i",
            video.to_string_lossy().as_ref(),
            "-t",
            &format!("{seconds:.2}"),
            "-vn",
            "-ac",
            "1",
            "-ar",
            &SAMPLE_RATE.to_string(),
            "-f",
            "f32le",
            "-y",
            dest.to_string_lossy().as_ref(),
        ]);
        let output = cmd
            .output()
            .map_err(|e| VideoReviewError::Message(format!("ffmpeg 抽音频失败: {e}")))?;
        if !output.status.success() {
            return Err(VideoReviewError::Message(format!(
                "ffmpeg 抽音频失败: {}",
                String::from_utf8_lossy(&output.stderr)
            )));
        }
        if !dest.exists() || std::fs::metadata(dest).map(|m| m.len()).unwrap_or(0) == 0 {
            return Err(VideoReviewError::Message(
                "视频无音轨或音频为空，无法音频对齐".into(),
            ));
        }
        Ok(())
    }
}

fn read_f32_pcm(path: &Path) -> VideoReviewResult<Vec<f32>> {
    let bytes = std::fs::read(path)?;
    if bytes.len() < 4 || bytes.len() % 4 != 0 {
        return Err(VideoReviewError::Message("PCM 数据无效".into()));
    }
    let mut out = Vec::with_capacity(bytes.len() / 4);
    for chunk in bytes.chunks_exact(4) {
        out.push(f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]));
    }
    Ok(log_energy_envelope(&out, 256))
}

fn log_energy_envelope(samples: &[f32], hop: usize) -> Vec<f32> {
    let hop = hop.max(32);
    let mut env = Vec::with_capacity(samples.len() / hop + 1);
    for window in samples.chunks(hop) {
        let e: f32 = window.iter().map(|s| s * s).sum::<f32>() / window.len() as f32;
        env.push((e + 1e-12).ln());
    }
    let mut stretched = Vec::with_capacity(samples.len());
    for v in env {
        for _ in 0..hop {
            stretched.push(v);
        }
    }
    if stretched.len() > samples.len() {
        stretched.truncate(samples.len());
    }
    stretched
}

/// 返回 (lag_samples, confidence)。lag>0 表示 `other` 相对 `reference` 延后。
fn cross_correlate_lag(reference: &[f32], other: &[f32]) -> (i64, f32) {
    let n = reference.len().max(other.len()).next_power_of_two() * 2;
    let mut planner = FftPlanner::<f32>::new();
    let fft = planner.plan_fft_forward(n);
    let ifft = planner.plan_fft_inverse(n);

    let mut a = vec![Complex::new(0.0, 0.0); n];
    let mut b = vec![Complex::new(0.0, 0.0); n];
    for (i, &s) in reference.iter().enumerate() {
        a[i] = Complex::new(s, 0.0);
    }
    for (i, &s) in other.iter().enumerate() {
        b[i] = Complex::new(s, 0.0);
    }
    fft.process(&mut a);
    fft.process(&mut b);
    for i in 0..n {
        a[i] = a[i].conj() * b[i];
    }
    ifft.process(&mut a);

    let max_lag_samples = ((MAX_LAG_MS as f64) * f64::from(SAMPLE_RATE) / 1000.0).round() as i64;
    let mut best_i = 0usize;
    let mut best_v = f32::NEG_INFINITY;
    let mut energy = 0.0f32;
    for (i, c) in a.iter().enumerate() {
        let v = c.re;
        energy += v * v;
        let lag = if i <= n / 2 {
            i as i64
        } else {
            i as i64 - n as i64
        };
        if lag.abs() > max_lag_samples {
            continue;
        }
        if v > best_v {
            best_v = v;
            best_i = i;
        }
    }
    let lag = if best_i <= n / 2 {
        best_i as i64
    } else {
        best_i as i64 - n as i64
    };
    let rms = (energy / n as f32).sqrt().max(1e-9);
    let confidence = (best_v.abs() / (rms * n as f32)).clamp(0.0, 1.0);
    let mean = a.iter().map(|c| c.re.abs()).sum::<f32>() / n as f32;
    let conf2 = if mean > 1e-12 {
        (best_v.abs() / (mean * 8.0)).clamp(0.0, 1.0)
    } else {
        0.0
    };
    (lag, conf2.max(confidence * 0.5))
}

/// 按帧调整偏移：`frames` 为正表示该路画面应更早（offset 减小）。
pub fn offset_after_frame_step(offset_ms: i64, fps: f32, frames: i64) -> i64 {
    if fps <= 0.01 {
        return offset_ms + frames;
    }
    let frame_ms = (1000.0 / fps).round() as i64;
    offset_ms - frames * frame_ms.max(1)
}

/// 将毫秒偏移量化到最近整帧，减少抽帧抖动。
pub fn quantize_offset_to_fps(offset_ms: i64, fps: f32) -> i64 {
    if fps <= 0.01 {
        return offset_ms;
    }
    let frame_ms = (1000.0 / fps).round() as i64;
    if frame_ms <= 0 {
        return offset_ms;
    }
    let frames = (offset_ms as f64 / frame_ms as f64).round() as i64;
    frames * frame_ms
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_step_uses_fps() {
        assert_eq!(offset_after_frame_step(0, 25.0, 1), -40);
        assert_eq!(offset_after_frame_step(0, 30.0, -1), 33);
    }

    #[test]
    fn quantize_rounds_to_frame() {
        assert_eq!(quantize_offset_to_fps(21, 25.0), 40);
        assert_eq!(quantize_offset_to_fps(19, 25.0), 0);
    }

    #[test]
    fn correlate_detects_delay() {
        let mut a = vec![0.0f32; 8000];
        let mut b = vec![0.0f32; 8000];
        for i in 1000..1200 {
            a[i] = 1.0;
        }
        for i in 1300..1500 {
            b[i] = 1.0;
        }
        let (lag, conf) = cross_correlate_lag(&a, &b);
        assert!(conf > 0.0);
        assert!((lag - 300).abs() < 50, "lag={lag}");
    }
}
