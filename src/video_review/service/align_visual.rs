//! 画面内容对齐：rawvideo 管线运动能量（快）+ 可选缓冲内 NCC + 特征（精细）。

use std::io::Read;
use std::path::Path;
use std::process::Stdio;

use image::GrayImage;

use crate::video_review::domain::VideoItem;
use crate::video_review::error::{VideoReviewError, VideoReviewResult};
use crate::video_review::service::align_dsp::{
    cross_correlate_lag_limited, quantize_offset_to_fps, MAX_LAG_MS,
};

const MIN_SIGNAL_VAR: f32 = 1e-4;
const FEATURE_CANDIDATE_STEPS: i64 = 8;

#[derive(Debug, Clone, Copy)]
pub struct VisualAlignParams {
    pub sample_ms: u64,
    pub width: u32,
    pub do_ncc: bool,
    pub ncc_frames: i64,
    pub enable_features: bool,
}

impl Default for VisualAlignParams {
    fn default() -> Self {
        Self {
            sample_ms: 200,
            width: 160,
            do_ncc: false,
            ncc_frames: 0,
            enable_features: false,
        }
    }
}

#[derive(Debug, Clone)]
pub struct VisualAlignOutcome {
    pub offset_ms: i64,
    pub confidence: f32,
    pub method: &'static str,
}

#[derive(Debug, Clone)]
pub struct GraySequence {
    pub frames: Vec<GrayImage>,
    pub width: u32,
    pub height: u32,
}

/// A（+可选 B）：raw 灰度管线 → 运动/亮度信号 xcorr → 缓冲内 NCC。
pub fn align_visual_ab_params(
    ffmpeg: &str,
    reference: &VideoItem,
    other: &VideoItem,
    analysis_secs: f32,
    around_ms: Option<u64>,
    params: VisualAlignParams,
) -> VideoReviewResult<VisualAlignOutcome> {
    align_visual_ab_with_optional_ref(
        ffmpeg,
        reference,
        other,
        analysis_secs,
        around_ms,
        params,
        None,
    )
}

/// 主路灰度已抽取时复用，避免每路重复 decode。
pub fn align_visual_ab_with_ref_seq(
    ffmpeg: &str,
    other: &VideoItem,
    analysis_secs: f32,
    around_ms: Option<u64>,
    params: VisualAlignParams,
    ref_seq: &GraySequence,
) -> VideoReviewResult<VisualAlignOutcome> {
    let (oth_frames, _, _) = extract_gray_rawvideo(
        ffmpeg,
        &other.file_path,
        analysis_secs,
        around_ms,
        params.sample_ms,
        params.width,
    )?;
    finish_visual_align(
        &ref_seq.frames,
        &oth_frames,
        ref_seq.width,
        ref_seq.height,
        params,
        other.offset_ms,
        other.fps,
    )
}

fn align_visual_ab_with_optional_ref(
    ffmpeg: &str,
    reference: &VideoItem,
    other: &VideoItem,
    analysis_secs: f32,
    around_ms: Option<u64>,
    params: VisualAlignParams,
    ref_seq: Option<&GraySequence>,
) -> VideoReviewResult<VisualAlignOutcome> {
    let (ref_frames, oth_frames, ref_w, ref_h) = if let Some(seq) = ref_seq {
        let (oth, _, _) = extract_gray_rawvideo(
            ffmpeg,
            &other.file_path,
            analysis_secs,
            around_ms,
            params.sample_ms,
            params.width,
        )?;
        (seq.frames.clone(), oth, seq.width, seq.height)
    } else {
        let ffmpeg_a = ffmpeg.to_string();
        let path_a = reference.file_path.clone();
        let path_b = other.file_path.clone();
        let secs = analysis_secs;
        let sample = params.sample_ms;
        let width = params.width;
        let (ra, rb) = rayon::join(
            || extract_gray_rawvideo(&ffmpeg_a, &path_a, secs, around_ms, sample, width),
            || extract_gray_rawvideo(ffmpeg, &path_b, secs, around_ms, sample, width),
        );
        let (ref_f, w, h) = ra?;
        let (oth_f, _, _) = rb?;
        (ref_f, oth_f, w, h)
    };

    finish_visual_align(
        &ref_frames,
        &oth_frames,
        ref_w,
        ref_h,
        params,
        other.offset_ms,
        other.fps,
    )
}

fn finish_visual_align(
    ref_seq: &[GrayImage],
    oth_seq: &[GrayImage],
    ref_w: u32,
    ref_h: u32,
    params: VisualAlignParams,
    fallback_offset: i64,
    fps: f32,
) -> VideoReviewResult<VisualAlignOutcome> {
    if ref_seq.len() < 8 || oth_seq.len() < 8 {
        return Ok(VisualAlignOutcome {
            offset_ms: fallback_offset,
            confidence: 0.0,
            method: "visual_xcorr",
        });
    }

    let ref_sig = temporal_signal(ref_seq);
    let oth_sig = temporal_signal(oth_seq);
    if signal_variance(&ref_sig) < MIN_SIGNAL_VAR || signal_variance(&oth_sig) < MIN_SIGNAL_VAR {
        return Ok(VisualAlignOutcome {
            offset_ms: fallback_offset,
            confidence: 0.0,
            method: "visual_xcorr",
        });
    }

    let hz = 1000.0 / params.sample_ms.max(1) as f32;
    let max_lag_samples = ((MAX_LAG_MS as f64) * f64::from(hz) / 1000.0).round() as i64;
    let (lag_samples, mut conf) = cross_correlate_lag_limited(&ref_sig, &oth_sig, max_lag_samples);
    let mut offset_ms = ((lag_samples as f64) * params.sample_ms as f64).round() as i64;

    if offset_ms.abs() > MAX_LAG_MS {
        return Ok(VisualAlignOutcome {
            offset_ms: fallback_offset,
            confidence: 0.0,
            method: "visual_xcorr",
        });
    }

    let mut used_ncc = false;
    if params.do_ncc && params.ncc_frames > 0 {
        let (refined, ncc_conf, ok) = refine_ncc_from_seqs(
            ref_seq,
            oth_seq,
            params.sample_ms,
            offset_ms,
            fps,
            params.ncc_frames,
        );
        if ok {
            offset_ms = refined;
            conf = conf.max(ncc_conf);
            used_ncc = true;
        }
    }

    let _ = (ref_w, ref_h);
    offset_ms = quantize_offset_to_fps(offset_ms, fps);
    Ok(VisualAlignOutcome {
        offset_ms,
        confidence: conf.clamp(0.0, 1.0),
        method: if used_ncc {
            "visual_ncc"
        } else {
            "visual_xcorr"
        },
    })
}

/// 预抽主路灰度序列（批量对齐复用）。
pub fn extract_reference_gray_seq(
    ffmpeg: &str,
    reference: &VideoItem,
    analysis_secs: f32,
    around_ms: Option<u64>,
    params: VisualAlignParams,
) -> VideoReviewResult<GraySequence> {
    let (frames, width, height) = extract_gray_rawvideo(
        ffmpeg,
        &reference.file_path,
        analysis_secs,
        around_ms,
        params.sample_ms,
        params.width,
    )?;
    Ok(GraySequence {
        frames,
        width,
        height,
    })
}

/// D：在已抽序列上滑 lag（不再按候选反复 spawn ffmpeg）。
pub fn align_visual_features_params(
    ffmpeg: &str,
    reference: &VideoItem,
    other: &VideoItem,
    analysis_secs: f32,
    around_ms: Option<u64>,
    seed_offset_ms: Option<i64>,
    params: VisualAlignParams,
) -> VideoReviewResult<VisualAlignOutcome> {
    let (ref_seq, _, _) = extract_gray_rawvideo(
        ffmpeg,
        &reference.file_path,
        analysis_secs,
        around_ms,
        params.sample_ms,
        params.width,
    )?;
    let (oth_seq, _, _) = extract_gray_rawvideo(
        ffmpeg,
        &other.file_path,
        analysis_secs,
        around_ms,
        params.sample_ms,
        params.width,
    )?;
    if ref_seq.len() < 4 || oth_seq.len() < 4 {
        return Ok(VisualAlignOutcome {
            offset_ms: other.offset_ms,
            confidence: 0.0,
            method: "visual_orb",
        });
    }

    let sample = params.sample_ms.max(1) as i64;
    let seed = seed_offset_ms.unwrap_or(0);
    let seed_steps = (seed as f64 / sample as f64).round() as i64;

    let mut best_offset = seed;
    let mut best_score = 0usize;
    let mut second = 0usize;

    // 在序列索引差上搜索
    for step in -FEATURE_CANDIDATE_STEPS..=FEATURE_CANDIDATE_STEPS {
        let lag_steps = seed_steps + step;
        let lag_ms = lag_steps * sample;
        if lag_ms.abs() > MAX_LAG_MS {
            continue;
        }
        let mut score = 0usize;
        // 取 3 个锚点
        let n = ref_seq.len().min(oth_seq.len());
        for k in 0..3usize {
            let ri = (n / 4) * (k + 1);
            if ri >= ref_seq.len() {
                continue;
            }
            let oi = (ri as i64 + lag_steps).clamp(0, oth_seq.len() as i64 - 1) as usize;
            score += match_brief_inliers(&ref_seq[ri], &oth_seq[oi]);
        }
        if score > best_score {
            second = best_score;
            best_score = score;
            best_offset = lag_ms;
        } else if score > second {
            second = score;
        }
    }

    let confidence = if best_score < 6 {
        0.0
    } else {
        let ratio = if second == 0 {
            1.0
        } else {
            best_score as f32 / second as f32
        };
        ((best_score as f32 / 40.0).min(1.0) * 0.5 + (ratio - 1.0).clamp(0.0, 1.0) * 0.5)
            .clamp(0.0, 1.0)
    };

    Ok(VisualAlignOutcome {
        offset_ms: quantize_offset_to_fps(best_offset, other.fps),
        confidence,
        method: "visual_orb",
    })
}

/// 从 stdout raw gray 读帧；返回 (frames, width, height)。
fn extract_gray_rawvideo(
    ffmpeg: &str,
    video: &Path,
    seconds: f32,
    around_ms: Option<u64>,
    interval_ms: u64,
    width: u32,
) -> VideoReviewResult<(Vec<GrayImage>, u32, u32)> {
    let half = (seconds * 500.0) as u64;
    let start_secs = around_ms
        .map(|t| t.saturating_sub(half) as f64 / 1000.0)
        .unwrap_or(0.0);
    let fps = 1000.0 / interval_ms.max(1) as f64;
    // 固定偶数高度近似 9:16 或 16:9：用 scale=W:-2 后用 ffprobe 不现实；
    // 改为 scale=W:H 固定高度，保证 raw 尺寸可知。
    let height = ((width as f32 * 9.0 / 16.0).round() as u32).max(90) & !1;

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
        "-an",
        "-vf",
        &format!("fps={fps:.4},scale={width}:{height}"),
        "-pix_fmt",
        "gray",
        "-f",
        "rawvideo",
        "pipe:1",
    ]);
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());

    let mut child = cmd
        .spawn()
        .map_err(|e| VideoReviewError::Message(format!("ffmpeg 抽帧失败: {e}")))?;
    let mut stdout = child
        .stdout
        .take()
        .ok_or_else(|| VideoReviewError::Message("ffmpeg stdout 不可用".into()))?;
    let mut bytes = Vec::new();
    stdout
        .read_to_end(&mut bytes)
        .map_err(|e| VideoReviewError::Message(format!("读 rawvideo 失败: {e}")))?;
    let _ = child.wait();

    let frame_bytes = (width * height) as usize;
    if frame_bytes == 0 || bytes.len() < frame_bytes {
        return Err(VideoReviewError::Message("画面序列为空".into()));
    }
    let mut frames = Vec::with_capacity(bytes.len() / frame_bytes);
    for chunk in bytes.chunks_exact(frame_bytes) {
        let img = GrayImage::from_raw(width, height, chunk.to_vec())
            .ok_or_else(|| VideoReviewError::Message("构造灰度帧失败".into()))?;
        frames.push(img);
    }
    Ok((frames, width, height))
}

fn temporal_signal(frames: &[GrayImage]) -> Vec<f32> {
    let mut luma = Vec::with_capacity(frames.len());
    let mut prev: Option<&GrayImage> = None;
    let mut motion = Vec::with_capacity(frames.len());
    for f in frames {
        let mean = f.iter().map(|&p| p as f32).sum::<f32>() / (f.len().max(1) as f32) / 255.0;
        luma.push(mean);
        let m = if let Some(p) = prev {
            let n = f.len().min(p.len()).max(1);
            let mut acc = 0.0f32;
            for i in 0..n {
                acc += (f.as_raw()[i] as f32 - p.as_raw()[i] as f32).abs();
            }
            acc / (n as f32) / 255.0
        } else {
            0.0
        };
        motion.push(m);
        prev = Some(f);
    }
    let luma_n = normalize(&luma);
    let mot_n = normalize(&motion);
    luma_n.into_iter().zip(mot_n).map(|(a, b)| a + b).collect()
}

fn normalize(v: &[f32]) -> Vec<f32> {
    let mean = v.iter().sum::<f32>() / v.len().max(1) as f32;
    let var = v.iter().map(|x| (x - mean).powi(2)).sum::<f32>() / v.len().max(1) as f32;
    let std = var.sqrt().max(1e-6);
    v.iter().map(|x| (x - mean) / std).collect()
}

fn signal_variance(v: &[f32]) -> f32 {
    if v.is_empty() {
        return 0.0;
    }
    let mean = v.iter().sum::<f32>() / v.len() as f32;
    v.iter().map(|x| (x - mean).powi(2)).sum::<f32>() / v.len() as f32
}

/// 用已抽序列做 NCC：按 sample_ms 索引，不再 spawn ffmpeg。
fn refine_ncc_from_seqs(
    ref_seq: &[GrayImage],
    oth_seq: &[GrayImage],
    sample_ms: u64,
    seed_offset_ms: i64,
    fps: f32,
    ncc_frames: i64,
) -> (i64, f32, bool) {
    let sample = sample_ms.max(1) as i64;
    let frame_ms = if fps > 0.01 {
        (1000.0 / fps).round() as i64
    } else {
        sample
    }
    .max(1);

    let mid = ref_seq.len() / 2;
    let ref_g = &ref_seq[mid];

    let mut best_lag = seed_offset_ms;
    let mut best_ncc = f32::NEG_INFINITY;
    let mut second = f32::NEG_INFINITY;

    for step in -ncc_frames..=ncc_frames {
        let lag = seed_offset_ms + step * frame_ms;
        let oth_idx = mid as i64 + (lag as f64 / sample as f64).round() as i64;
        if oth_idx < 0 || oth_idx as usize >= oth_seq.len() {
            continue;
        }
        let ncc = normalized_cross_correlation(ref_g, &oth_seq[oth_idx as usize]);
        if ncc > best_ncc {
            second = best_ncc;
            best_ncc = ncc;
            best_lag = lag;
        } else if ncc > second {
            second = ncc;
        }
    }

    if best_ncc < 0.15 {
        return (seed_offset_ms, 0.0, false);
    }
    let peakiness = if second <= -1.0 {
        1.0
    } else {
        ((best_ncc - second) / best_ncc.max(1e-3)).clamp(0.0, 1.0)
    };
    let conf = (best_ncc.clamp(0.0, 1.0) * 0.7 + peakiness * 0.3).clamp(0.0, 1.0);
    (best_lag, conf, true)
}

fn normalized_cross_correlation(a: &GrayImage, b: &GrayImage) -> f32 {
    let w = a.width().min(b.width());
    let h = a.height().min(b.height());
    if w == 0 || h == 0 {
        return 0.0;
    }
    let n = (w * h) as f32;
    let mut sum_a = 0.0f32;
    let mut sum_b = 0.0f32;
    for y in 0..h {
        for x in 0..w {
            sum_a += a.get_pixel(x, y).0[0] as f32;
            sum_b += b.get_pixel(x, y).0[0] as f32;
        }
    }
    let mean_a = sum_a / n;
    let mean_b = sum_b / n;
    let mut num = 0.0f32;
    let mut da = 0.0f32;
    let mut db = 0.0f32;
    for y in 0..h {
        for x in 0..w {
            let va = a.get_pixel(x, y).0[0] as f32 - mean_a;
            let vb = b.get_pixel(x, y).0[0] as f32 - mean_b;
            num += va * vb;
            da += va * va;
            db += vb * vb;
        }
    }
    let den = (da * db).sqrt();
    if den < 1e-6 {
        // 双方无方差：内容一致（同色块）视为完全相关
        if (mean_a - mean_b).abs() < 1e-3 {
            1.0
        } else {
            0.0
        }
    } else {
        (num / den).clamp(-1.0, 1.0)
    }
}

fn match_brief_inliers(a: &GrayImage, b: &GrayImage) -> usize {
    let ka = detect_fast(a, 20, 80);
    let kb = detect_fast(b, 20, 80);
    if ka.len() < 4 || kb.len() < 4 {
        return 0;
    }
    let da: Vec<[u8; 32]> = ka.iter().map(|&(x, y)| brief_desc(a, x, y)).collect();
    let db: Vec<[u8; 32]> = kb.iter().map(|&(x, y)| brief_desc(b, x, y)).collect();

    let mut good = 0usize;
    for d in &da {
        let mut best = u32::MAX;
        let mut second = u32::MAX;
        for e in &db {
            let dist = hamming(d, e);
            if dist < best {
                second = best;
                best = dist;
            } else if dist < second {
                second = dist;
            }
        }
        if best < 48 && (second == u32::MAX || best as f32 * 0.8 < second as f32) {
            good += 1;
        }
    }
    good
}

fn detect_fast(img: &GrayImage, threshold: u8, max_pts: usize) -> Vec<(u32, u32)> {
    let w = img.width();
    let h = img.height();
    let mut pts = Vec::new();
    if w < 16 || h < 16 {
        return pts;
    }
    let offs: [(i32, i32); 16] = [
        (0, -3),
        (1, -3),
        (2, -2),
        (3, -1),
        (3, 0),
        (3, 1),
        (2, 2),
        (1, 3),
        (0, 3),
        (-1, 3),
        (-2, 2),
        (-3, 1),
        (-3, 0),
        (-3, -1),
        (-2, -2),
        (-1, -3),
    ];
    let step = 4u32;
    for y in (3..h.saturating_sub(3)).step_by(step as usize) {
        for x in (3..w.saturating_sub(3)).step_by(step as usize) {
            let c = img.get_pixel(x, y).0[0];
            let mut brighter = 0u8;
            let mut darker = 0u8;
            let mut max_run = 0u8;
            for &(dx, dy) in &offs {
                let p = img
                    .get_pixel((x as i32 + dx) as u32, (y as i32 + dy) as u32)
                    .0[0];
                if p > c.saturating_add(threshold) {
                    brighter += 1;
                    darker = 0;
                    max_run = max_run.max(brighter);
                } else if p < c.saturating_sub(threshold) {
                    darker += 1;
                    brighter = 0;
                    max_run = max_run.max(darker);
                } else {
                    brighter = 0;
                    darker = 0;
                }
            }
            for &(dx, dy) in &offs[..9] {
                let p = img
                    .get_pixel((x as i32 + dx) as u32, (y as i32 + dy) as u32)
                    .0[0];
                if p > c.saturating_add(threshold) {
                    brighter += 1;
                    darker = 0;
                    max_run = max_run.max(brighter);
                } else if p < c.saturating_sub(threshold) {
                    darker += 1;
                    brighter = 0;
                    max_run = max_run.max(darker);
                } else {
                    brighter = 0;
                    darker = 0;
                }
            }
            if max_run >= 9 {
                pts.push((x, y));
            }
        }
    }
    pts.truncate(max_pts);
    pts
}

fn brief_desc(img: &GrayImage, x: u32, y: u32) -> [u8; 32] {
    let mut out = [0u8; 32];
    let w = img.width() as i32;
    let h = img.height() as i32;
    let xi = x as i32;
    let yi = y as i32;
    for bit in 0..256 {
        let ax = (bit as i32 * 37 + 11) % 31 - 15;
        let ay = (bit as i32 * 91 + 7) % 31 - 15;
        let bx = (bit as i32 * 53 + 19) % 31 - 15;
        let by = (bit as i32 * 17 + 29) % 31 - 15;
        let x1 = (xi + ax).clamp(0, w - 1) as u32;
        let y1 = (yi + ay).clamp(0, h - 1) as u32;
        let x2 = (xi + bx).clamp(0, w - 1) as u32;
        let y2 = (yi + by).clamp(0, h - 1) as u32;
        let v1 = img.get_pixel(x1, y1).0[0];
        let v2 = img.get_pixel(x2, y2).0[0];
        if v1 < v2 {
            out[bit / 8] |= 1 << (bit % 8);
        }
    }
    out
}

fn hamming(a: &[u8; 32], b: &[u8; 32]) -> u32 {
    a.iter()
        .zip(b.iter())
        .map(|(x, y)| (x ^ y).count_ones())
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Luma;

    #[test]
    fn ncc_identical_is_one() {
        let mut a = GrayImage::new(8, 8);
        for p in a.pixels_mut() {
            *p = Luma([120]);
        }
        let b = a.clone();
        assert!((normalized_cross_correlation(&a, &b) - 1.0).abs() < 1e-3);
    }

    #[test]
    fn brief_hamming_self_zero() {
        let mut a = GrayImage::new(64, 64);
        for (i, p) in a.pixels_mut().enumerate() {
            *p = Luma([((i * 13) % 255) as u8]);
        }
        let d = brief_desc(&a, 32, 32);
        assert_eq!(hamming(&d, &d), 0);
    }
}
