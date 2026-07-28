//! 多视频帧对齐：音频优先短窗 + 画面 raw 管线 + 质量档。

use std::io::Read;
use std::path::Path;
use std::sync::Arc;
use std::time::Instant;

use rayon::prelude::*;
use tempfile::TempDir;

use crate::ui::progress::ProgressReporter;
use crate::video_review::domain::VideoItem;
use crate::video_review::error::{VideoReviewError, VideoReviewResult};
use crate::video_review::service::align_dsp::{
    chromagram_lag_limited, estimate_clock_drift, gcc_phat_lag_limited, log_energy_envelope,
    offset_with_drift,
};
use crate::video_review::service::align_visual::{
    align_visual_ab_params, align_visual_ab_with_ref_seq, align_visual_features_params,
    extract_reference_gray_seq, GraySequence, VisualAlignParams,
};
use crate::video_review::service::ffmpeg_backend::FfmpegBackend;

/// 兼容旧调用：精细档默认窗长。
pub const DEFAULT_ALIGN_SECONDS: f32 = 30.0;
/// 自动模式：音频置信达到此值则跳过画面。
pub const AUDIO_SHORTCIRCUIT_CONF: f32 = 0.55;
/// 级联下一档音频方法的置信门槛。
const AUDIO_FALLBACK_CONF: f32 = 0.42;
/// PCM RMS 低于此视为无声/近无声，直接走画面。
const SILENCE_RMS: f32 = 1e-4;

pub use crate::video_review::service::align_dsp::{
    cross_correlate_lag_limited, offset_after_frame_step, quantize_offset_to_fps, MAX_LAG_MS,
};

const PARALLEL_OTHERS: usize = 3;

/// 对齐策略。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AlignMode {
    #[default]
    Auto,
    Audio,
    Visual,
    Features,
}

impl AlignMode {
    pub const ALL: [AlignMode; 4] = [Self::Auto, Self::Audio, Self::Visual, Self::Features];

    pub fn label(self) -> &'static str {
        match self {
            Self::Auto => "自动",
            Self::Audio => "音频",
            Self::Visual => "画面",
            Self::Features => "特征",
        }
    }
}

/// 质量档：控制窗长 / 采样 / 是否精修与特征。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AlignQuality {
    #[default]
    Fast,
    Standard,
    Fine,
}

impl AlignQuality {
    pub const ALL: [AlignQuality; 3] = [Self::Fast, Self::Standard, Self::Fine];

    pub fn label(self) -> &'static str {
        match self {
            Self::Fast => "快速",
            Self::Standard => "标准",
            Self::Fine => "精细",
        }
    }

    pub fn analysis_secs(self) -> f32 {
        match self {
            Self::Fast => 5.0,
            Self::Standard => 12.0,
            Self::Fine => 30.0,
        }
    }

    pub fn audio_sample_rate(self) -> u32 {
        match self {
            Self::Fast => 8_000,
            Self::Standard | Self::Fine => 16_000,
        }
    }

    /// 嘈杂环境：包络不够时用 GCC-PHAT（快速也开，很便宜）。
    pub fn use_gcc_phat(self) -> bool {
        true
    }

    /// chromagram 较重，仅精细档在 GCC 仍弱时使用。
    pub fn use_chroma(self) -> bool {
        matches!(self, Self::Fine)
    }

    /// 兼容旧名：标准/精细级联（快速仅包络+GCC）。
    pub fn audio_cascade(self) -> bool {
        self.use_gcc_phat()
    }

    /// 精细：多分窗估计时钟漂移。
    pub fn drift_enabled(self) -> bool {
        matches!(self, Self::Fine)
    }

    pub fn drift_span_secs(self) -> f32 {
        match self {
            Self::Fine => 90.0,
            _ => 0.0,
        }
    }

    pub fn drift_windows(self) -> usize {
        match self {
            Self::Fine => 5,
            _ => 0,
        }
    }

    pub fn visual_params(self) -> VisualAlignParams {
        match self {
            Self::Fast => VisualAlignParams {
                sample_ms: 250,
                width: 128,
                do_ncc: false,
                ncc_frames: 0,
                enable_features: false,
            },
            Self::Standard => VisualAlignParams {
                sample_ms: 160,
                width: 192,
                do_ncc: true,
                ncc_frames: 2,
                enable_features: false,
            },
            Self::Fine => VisualAlignParams {
                sample_ms: 100,
                width: 320,
                do_ncc: true,
                ncc_frames: 5,
                enable_features: true,
            },
        }
    }
}

#[derive(Debug, Clone)]
pub struct AlignPairResult {
    pub video_id: i64,
    pub offset_ms: i64,
    pub confidence: f32,
    pub method: String,
    /// 时钟漂移（ppm）；仅精细档多分窗拟合成功时有值。
    pub drift_ppm: Option<f32>,
}

#[derive(Debug, Clone)]
pub struct AlignBatchResult {
    pub reference_id: i64,
    pub pairs: Vec<AlignPairResult>,
    pub mode: AlignMode,
    pub quality: AlignQuality,
    pub elapsed_ms: u64,
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

    pub fn align_to_reference(
        &self,
        reference: &VideoItem,
        others: &[VideoItem],
        around_ms: Option<u64>,
        mode: AlignMode,
        quality: AlignQuality,
        progress: Option<&dyn ProgressReporter>,
    ) -> VideoReviewResult<AlignBatchResult> {
        let t0 = Instant::now();
        let secs = quality.analysis_secs();
        let sample_rate = quality.audio_sample_rate();
        let visual_params = quality.visual_params();

        let others: Vec<&VideoItem> = others.iter().filter(|v| v.id != reference.id).collect();

        // 1 步准备主路 + 每路副视频 1 步。
        let total_steps = 1 + others.len();
        if let Some(p) = progress {
            p.set_total(total_steps);
            p.set_current_label("准备主路…");
        }

        let need_audio = matches!(mode, AlignMode::Auto | AlignMode::Audio);
        let ref_pcm = if need_audio {
            if let Some(p) = progress {
                p.set_current_label("抽取主路音频…");
            }
            match self.extract_mono_pcm(&reference.file_path, secs, around_ms, sample_rate) {
                Ok(s) if s.len() >= sample_rate as usize / 2 => Some(Arc::new(s)),
                Ok(_) | Err(_) => None,
            }
        } else {
            None
        };
        let ref_silent = ref_pcm
            .as_ref()
            .map(|p| pcm_is_silent(p.as_slice()))
            .unwrap_or(true);
        // Auto 且主路无声：跳过音频级联。非纯音频模式预抽主路灰度复用。
        let skip_audio = matches!(mode, AlignMode::Auto) && ref_silent;
        if skip_audio {
            if let Some(p) = progress {
                p.set_current_label("主路近无声，改走画面…");
            }
        }
        let need_ref_gray = !matches!(mode, AlignMode::Audio);
        let ref_gray: Option<Arc<GraySequence>> = if need_ref_gray {
            if let Some(p) = progress {
                p.set_current_label("抽取主路画面…");
            }
            extract_reference_gray_seq(&self.ffmpeg_path, reference, secs, around_ms, visual_params)
                .ok()
                .map(Arc::new)
        } else {
            None
        };
        if let Some(p) = progress {
            p.inc(None);
        }

        let ffmpeg = self.ffmpeg_path.clone();
        let ref_item = reference.clone();
        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(PARALLEL_OTHERS.min(others.len().max(1)))
            .build()
            .unwrap_or_else(|_| rayon::ThreadPoolBuilder::new().build().expect("rayon"));

        let pair_results: Vec<AlignPairResult> = pool.install(|| {
            others
                .par_iter()
                .map(|video| {
                    let name = short_video_name(video);
                    if let Some(p) = progress {
                        p.set_current_label(&format!("{name} · 开始"));
                    }
                    let result = align_one_worker(
                        &ffmpeg,
                        &ref_item,
                        video,
                        secs,
                        around_ms,
                        mode,
                        quality,
                        sample_rate,
                        ref_pcm.clone(),
                        ref_gray.clone(),
                        skip_audio,
                        visual_params,
                        progress,
                        &name,
                    );
                    if let Some(p) = progress {
                        p.inc(None);
                        let stage = match result.method.as_str() {
                            "failed" => "失败",
                            m => m,
                        };
                        p.set_current_label(&format!("{name} · 完成（{stage}）"));
                    }
                    result
                })
                .collect()
        });

        if let Some(p) = progress {
            p.set_current_label("对齐完成");
            p.finish();
        }

        let mut pairs = Vec::with_capacity(pair_results.len() + 1);
        pairs.push(AlignPairResult {
            video_id: reference.id,
            offset_ms: 0,
            confidence: 1.0,
            method: "reference".into(),
            drift_ppm: None,
        });
        pairs.extend(pair_results);

        Ok(AlignBatchResult {
            reference_id: reference.id,
            pairs,
            mode,
            quality,
            elapsed_ms: t0.elapsed().as_millis() as u64,
        })
    }

    fn extract_mono_pcm(
        &self,
        video: &Path,
        seconds: f32,
        around_ms: Option<u64>,
        sample_rate: u32,
    ) -> VideoReviewResult<Vec<f32>> {
        extract_raw_pcm_bytes(&self.ffmpeg_path, video, seconds, around_ms, sample_rate)
    }
}

fn short_video_name(video: &VideoItem) -> String {
    video
        .file_path
        .file_stem()
        .and_then(|s| s.to_str())
        .map(|s| {
            if s.chars().count() > 18 {
                format!("{}…", s.chars().take(16).collect::<String>())
            } else {
                s.to_string()
            }
        })
        .unwrap_or_else(|| format!("#{}", video.id))
}

fn stage_label(progress: Option<&dyn ProgressReporter>, name: &str, stage: &str) {
    if let Some(p) = progress {
        p.set_current_label(&format!("{name} · {stage}"));
    }
}

fn align_one_worker(
    ffmpeg: &str,
    reference: &VideoItem,
    video: &VideoItem,
    secs: f32,
    around_ms: Option<u64>,
    mode: AlignMode,
    quality: AlignQuality,
    sample_rate: u32,
    ref_pcm: Option<Arc<Vec<f32>>>,
    ref_gray: Option<Arc<GraySequence>>,
    skip_audio: bool,
    visual_params: VisualAlignParams,
    progress: Option<&dyn ProgressReporter>,
    name: &str,
) -> AlignPairResult {
    let run_visual = || {
        stage_label(progress, name, "画面运动");
        if let Some(seq) = ref_gray.as_ref() {
            align_visual_ab_with_ref_seq(ffmpeg, video, secs, around_ms, visual_params, seq)
        } else {
            align_visual_ab_params(ffmpeg, reference, video, secs, around_ms, visual_params)
        }
    };

    match mode {
        AlignMode::Audio => {
            stage_label(progress, name, "音频对齐");
            match align_audio_with_ref(
                ffmpeg,
                reference,
                video,
                secs,
                around_ms,
                sample_rate,
                quality,
                ref_pcm.as_ref().map(|v| v.as_slice()),
                progress,
                name,
            ) {
                Ok(p) => p,
                Err(_) => fail_pair(video),
            }
        }
        AlignMode::Visual => match run_visual() {
            Ok(v) => pair_from_visual(video.id, v),
            Err(_) => fail_pair(video),
        },
        AlignMode::Features => {
            if !visual_params.enable_features {
                return match run_visual() {
                    Ok(v) => pair_from_visual(video.id, v),
                    Err(_) => fail_pair(video),
                };
            }
            let seed = run_visual()
                .ok()
                .filter(|o| o.confidence >= 0.2)
                .map(|o| o.offset_ms);
            stage_label(progress, name, "特征匹配");
            match align_visual_features_params(
                ffmpeg,
                reference,
                video,
                secs,
                around_ms,
                seed,
                visual_params,
            ) {
                Ok(v) => pair_from_visual(video.id, v),
                Err(_) => fail_pair(video),
            }
        }
        AlignMode::Auto => {
            if skip_audio {
                return match run_visual() {
                    Ok(v) => pair_from_visual(video.id, v),
                    Err(_) => fail_pair(video),
                };
            }
            stage_label(progress, name, "音频对齐");
            let audio = align_audio_with_ref(
                ffmpeg,
                reference,
                video,
                secs,
                around_ms,
                sample_rate,
                quality,
                ref_pcm.as_ref().map(|v| v.as_slice()),
                progress,
                name,
            );
            if let Ok(a) = &audio {
                if a.confidence >= AUDIO_SHORTCIRCUIT_CONF {
                    return a.clone();
                }
            }
            stage_label(progress, name, "音频不足，改画面");
            let visual = run_visual();
            match (audio, visual) {
                (Ok(a), Ok(v)) => {
                    if a.confidence >= v.confidence {
                        a
                    } else {
                        pair_from_visual(video.id, v)
                    }
                }
                (Ok(a), Err(_)) => a,
                (Err(_), Ok(v)) => pair_from_visual(video.id, v),
                (Err(_), Err(_)) => fail_pair(video),
            }
        }
    }
}

fn pair_from_visual(
    video_id: i64,
    v: crate::video_review::service::align_visual::VisualAlignOutcome,
) -> AlignPairResult {
    AlignPairResult {
        video_id,
        offset_ms: v.offset_ms,
        confidence: v.confidence,
        method: v.method.into(),
        drift_ppm: None,
    }
}

fn fail_pair(video: &VideoItem) -> AlignPairResult {
    AlignPairResult {
        video_id: video.id,
        offset_ms: video.offset_ms,
        confidence: 0.0,
        method: "failed".into(),
        drift_ppm: None,
    }
}

fn align_audio_with_ref(
    ffmpeg: &str,
    reference: &VideoItem,
    video: &VideoItem,
    secs: f32,
    around_ms: Option<u64>,
    sample_rate: u32,
    quality: AlignQuality,
    ref_pcm: Option<&[f32]>,
    progress: Option<&dyn ProgressReporter>,
    name: &str,
) -> VideoReviewResult<AlignPairResult> {
    let ref_owned;
    let ref_raw: &[f32] = if let Some(r) = ref_pcm {
        r
    } else {
        stage_label(progress, name, "抽取主路音频");
        ref_owned =
            extract_raw_pcm_bytes(ffmpeg, &reference.file_path, secs, around_ms, sample_rate)?;
        &ref_owned
    };
    if ref_raw.len() < sample_rate as usize / 2 {
        return Err(VideoReviewError::Message(
            "主视频可用音频过短，无法音频对齐".into(),
        ));
    }
    if pcm_is_silent(ref_raw) {
        return Err(VideoReviewError::Message("主视频近无声".into()));
    }
    stage_label(progress, name, "抽取副路音频");
    let other_raw = extract_raw_pcm_bytes(ffmpeg, &video.file_path, secs, around_ms, sample_rate)?;
    if other_raw.len() < sample_rate as usize / 2 {
        return Err(VideoReviewError::Message("副视频音频过短".into()));
    }
    if pcm_is_silent(&other_raw) {
        return Err(VideoReviewError::Message("副视频近无声".into()));
    }

    let max_lag_samples = ((MAX_LAG_MS as f64) * f64::from(sample_rate) / 1000.0).round() as i64;
    stage_label(progress, name, "包络互相关");
    let mut best = score_envelope(ref_raw, &other_raw, sample_rate, max_lag_samples, video.fps);

    if quality.use_gcc_phat() && best.1 < AUDIO_FALLBACK_CONF {
        stage_label(progress, name, "GCC-PHAT");
        let gcc = gcc_phat_lag_limited(ref_raw, &other_raw, max_lag_samples);
        let gcc_ms = lag_to_offset_ms(gcc.0, sample_rate, video.fps);
        if gcc.1 > best.1 {
            best = (gcc_ms, gcc.1, "audio_gcc_phat");
        }
    }
    if quality.use_chroma() && best.1 < AUDIO_FALLBACK_CONF {
        stage_label(progress, name, "色度图");
        let chroma = chromagram_lag_limited(ref_raw, &other_raw, sample_rate, max_lag_samples);
        let chroma_ms = lag_to_offset_ms(chroma.0, sample_rate, video.fps);
        if chroma.1 > best.1 {
            best = (chroma_ms, chroma.1, "audio_chroma");
        }
    }

    let (mut offset_ms, mut confidence, method) = best;
    if offset_ms.abs() > MAX_LAG_MS {
        confidence = 0.0;
        offset_ms = video.offset_ms;
    }

    let mut drift_ppm = None;
    if quality.drift_enabled() && confidence >= 0.25 {
        stage_label(progress, name, "估计漂移");
        if let Some((o0, ppm)) =
            estimate_pair_drift(ffmpeg, reference, video, sample_rate, quality, around_ms)
        {
            let at = around_ms.unwrap_or(0) as i64;
            offset_ms = quantize_offset_to_fps(offset_with_drift(o0, ppm, at), video.fps);
            drift_ppm = Some(ppm);
        }
    }

    Ok(AlignPairResult {
        video_id: video.id,
        offset_ms,
        confidence,
        method: method.into(),
        drift_ppm,
    })
}

fn score_envelope(
    ref_raw: &[f32],
    other_raw: &[f32],
    sample_rate: u32,
    max_lag_samples: i64,
    fps: f32,
) -> (i64, f32, &'static str) {
    let hop = (sample_rate as usize / 32).max(128);
    let ref_env = log_energy_envelope(ref_raw, hop);
    let oth_env = log_energy_envelope(other_raw, hop);
    let (lag, conf) = cross_correlate_lag_limited(&ref_env, &oth_env, max_lag_samples);
    (
        lag_to_offset_ms(lag, sample_rate, fps),
        conf,
        "audio_envelope",
    )
}

fn lag_to_offset_ms(lag_samples: i64, sample_rate: u32, fps: f32) -> i64 {
    let ms = ((lag_samples as f64) * 1000.0 / f64::from(sample_rate)).round() as i64;
    quantize_offset_to_fps(ms, fps)
}

fn estimate_pair_drift(
    ffmpeg: &str,
    reference: &VideoItem,
    video: &VideoItem,
    sample_rate: u32,
    quality: AlignQuality,
    around_ms: Option<u64>,
) -> Option<(i64, f32)> {
    let span = quality.drift_span_secs();
    let n_win = quality.drift_windows();
    if span < 20.0 || n_win < 3 {
        return None;
    }
    let win_secs = quality.analysis_secs().min(12.0);
    let half = (win_secs * 500.0) as u64;
    let max_t = reference
        .duration_ms
        .min(video.duration_ms)
        .saturating_sub(half + 500);
    if max_t < half + 5_000 {
        return None;
    }
    let start = around_ms
        .unwrap_or(0)
        .min(max_t.saturating_sub((span * 1000.0) as u64));
    let end = (start + (span * 1000.0) as u64).min(max_t);
    if end <= start + 5_000 {
        return None;
    }

    let mut points = Vec::with_capacity(n_win);
    for i in 0..n_win {
        let t = start + (end - start) * i as u64 / (n_win as u64 - 1).max(1);
        let ref_pcm =
            extract_raw_pcm_bytes(ffmpeg, &reference.file_path, win_secs, Some(t), sample_rate)
                .ok()?;
        let oth_pcm =
            extract_raw_pcm_bytes(ffmpeg, &video.file_path, win_secs, Some(t), sample_rate).ok()?;
        if ref_pcm.len() < sample_rate as usize / 2 || oth_pcm.len() < sample_rate as usize / 2 {
            continue;
        }
        let max_lag = ((MAX_LAG_MS as f64) * f64::from(sample_rate) / 1000.0).round() as i64;
        let (lag, conf) = gcc_phat_lag_limited(&ref_pcm, &oth_pcm, max_lag);
        if conf < 0.2 {
            continue;
        }
        let offset_ms = ((lag as f64) * 1000.0 / f64::from(sample_rate)).round() as i64;
        points.push((t as i64, offset_ms));
    }
    estimate_clock_drift(&points)
}

/// 抽取 raw mono f32 PCM（不做包络）。
fn extract_raw_pcm_bytes(
    ffmpeg: &str,
    video: &Path,
    seconds: f32,
    around_ms: Option<u64>,
    sample_rate: u32,
) -> VideoReviewResult<Vec<f32>> {
    let half = (seconds * 500.0) as u64;
    let start_secs = around_ms
        .map(|t| t.saturating_sub(half) as f64 / 1000.0)
        .unwrap_or(0.0);

    let mut cmd = crate::process_util::command(ffmpeg);
    cmd.args(["-hide_banner", "-loglevel", "error"]);
    if start_secs > 0.01 {
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
        &sample_rate.to_string(),
        "-f",
        "f32le",
        "pipe:1",
    ]);
    cmd.stdout(std::process::Stdio::piped());
    cmd.stderr(std::process::Stdio::piped());

    let mut child = cmd
        .spawn()
        .map_err(|e| VideoReviewError::Message(format!("ffmpeg 抽音频失败: {e}")))?;
    let mut stdout = child
        .stdout
        .take()
        .ok_or_else(|| VideoReviewError::Message("ffmpeg stdout 不可用".into()))?;
    let mut bytes = Vec::new();
    stdout
        .read_to_end(&mut bytes)
        .map_err(|e| VideoReviewError::Message(format!("读 PCM 失败: {e}")))?;
    let status = child
        .wait()
        .map_err(|e| VideoReviewError::Message(format!("ffmpeg 等待失败: {e}")))?;

    if !status.success() || bytes.len() < 4 {
        let tmp = TempDir::new().map_err(|e| VideoReviewError::Message(e.to_string()))?;
        let dest = tmp.path().join("a.f32");
        let mut cmd2 = crate::process_util::command(ffmpeg);
        cmd2.args(["-hide_banner", "-loglevel", "error"]);
        if start_secs > 0.01 {
            cmd2.args(["-ss", &format!("{start_secs:.3}")]);
        }
        cmd2.args([
            "-i",
            video.to_string_lossy().as_ref(),
            "-t",
            &format!("{seconds:.2}"),
            "-vn",
            "-ac",
            "1",
            "-ar",
            &sample_rate.to_string(),
            "-f",
            "f32le",
            "-y",
            dest.to_string_lossy().as_ref(),
        ]);
        let output = cmd2
            .output()
            .map_err(|e| VideoReviewError::Message(format!("ffmpeg 抽音频失败: {e}")))?;
        if !output.status.success() {
            return Err(VideoReviewError::Message(
                "视频无音轨或音频为空，无法音频对齐".into(),
            ));
        }
        bytes = std::fs::read(&dest)?;
    }

    if bytes.len() < 4 || bytes.len() % 4 != 0 {
        return Err(VideoReviewError::Message("PCM 数据无效".into()));
    }
    let mut out = Vec::with_capacity(bytes.len() / 4);
    for chunk in bytes.chunks_exact(4) {
        out.push(f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]));
    }
    Ok(out)
}

fn pcm_is_silent(samples: &[f32]) -> bool {
    if samples.is_empty() {
        return true;
    }
    let mean_sq: f32 = samples.iter().map(|s| s * s).sum::<f32>() / samples.len() as f32;
    mean_sq.sqrt() < SILENCE_RMS
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fast_quality_is_short() {
        assert!(AlignQuality::Fast.analysis_secs() < AlignQuality::Fine.analysis_secs());
        assert_eq!(AlignQuality::Fast.audio_sample_rate(), 8_000);
        assert!(!AlignQuality::Fast.visual_params().do_ncc);
        assert!(AlignQuality::Fast.use_gcc_phat());
        assert!(!AlignQuality::Fast.use_chroma());
        assert!(AlignQuality::Fine.drift_enabled());
    }

    #[test]
    fn silence_detects_zeros() {
        assert!(pcm_is_silent(&[0.0; 1000]));
        assert!(!pcm_is_silent(&[0.01; 1000]));
    }
}
