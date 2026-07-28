//! 对齐共用 DSP：包络 / GCC-PHAT / chromagram 互相关与时钟漂移估计。

use rustfft::{num_complex::Complex, FftPlanner};

/// 互相关最大搜索窗口（毫秒）。
pub const MAX_LAG_MS: i64 = 15_000;

/// 返回 (lag_samples, confidence)。普通幅度互相关（适合包络）。
pub fn cross_correlate_lag_limited(
    reference: &[f32],
    other: &[f32],
    max_lag_samples: i64,
) -> (i64, f32) {
    correlate_fft(reference, other, max_lag_samples, CorrelateKind::Plain)
}

/// GCC-PHAT：相位变换互相关，对频响差 / 混响更稳。
pub fn gcc_phat_lag_limited(
    reference: &[f32],
    other: &[f32],
    max_lag_samples: i64,
) -> (i64, f32) {
    correlate_fft(reference, other, max_lag_samples, CorrelateKind::GccPhat)
}

#[derive(Clone, Copy)]
enum CorrelateKind {
    Plain,
    GccPhat,
}

fn correlate_fft(
    reference: &[f32],
    other: &[f32],
    max_lag_samples: i64,
    kind: CorrelateKind,
) -> (i64, f32) {
    if reference.is_empty() || other.is_empty() {
        return (0, 0.0);
    }
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
        let mut c = a[i].conj() * b[i];
        if matches!(kind, CorrelateKind::GccPhat) {
            let mag = c.norm().max(1e-8);
            c /= mag;
        }
        a[i] = c;
    }
    ifft.process(&mut a);
    peak_from_ifft(&a, n, max_lag_samples)
}

fn peak_from_ifft(a: &[Complex<f32>], n: usize, max_lag_samples: i64) -> (i64, f32) {
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

/// 对数能量包络（拉伸回原采样长度，便于与 raw 共用 lag→ms 换算）。
pub fn log_energy_envelope(samples: &[f32], hop: usize) -> Vec<f32> {
    let hop = hop.max(32);
    if samples.is_empty() {
        return Vec::new();
    }
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

/// 12 维 chromagram 帧序列。
pub fn chromagram_frames(
    samples: &[f32],
    sample_rate: u32,
    n_fft: usize,
    hop: usize,
) -> Vec<[f32; 12]> {
    let n_fft = n_fft.max(256);
    let hop = hop.max(64);
    if samples.len() < n_fft {
        return Vec::new();
    }
    let mut planner = FftPlanner::<f32>::new();
    let fft = planner.plan_fft_forward(n_fft);
    let mut window = vec![0.0f32; n_fft];
    for (i, w) in window.iter_mut().enumerate() {
        *w = 0.5 - 0.5 * (std::f32::consts::TAU * i as f32 / (n_fft as f32 - 1.0)).cos();
    }

    let mut frames = Vec::new();
    let mut buf = vec![Complex::new(0.0, 0.0); n_fft];
    let mut i = 0usize;
    while i + n_fft <= samples.len() {
        for k in 0..n_fft {
            buf[k] = Complex::new(samples[i + k] * window[k], 0.0);
        }
        fft.process(&mut buf);
        let mut chroma = [0.0f32; 12];
        let half = n_fft / 2;
        for bin in 1..half {
            let freq = bin as f32 * sample_rate as f32 / n_fft as f32;
            if !(50.0..=sample_rate as f32 * 0.45).contains(&freq) {
                continue;
            }
            let mag = buf[bin].norm();
            let pc = hz_to_pitch_class(freq);
            chroma[pc] += mag;
        }
        let energy: f32 = chroma.iter().sum::<f32>().max(1e-9);
        for c in &mut chroma {
            *c /= energy;
        }
        frames.push(chroma);
        i += hop;
    }
    frames
}

fn hz_to_pitch_class(freq: f32) -> usize {
    let midi = 69.0 + 12.0 * (freq / 440.0).log2();
    let pc = midi.round().rem_euclid(12.0) as i32;
    pc.rem_euclid(12) as usize
}

/// Chromagram 滞后搜索：对 EQ / 压缩等处理差异更稳。
pub fn chromagram_lag_limited(
    reference: &[f32],
    other: &[f32],
    sample_rate: u32,
    max_lag_samples: i64,
) -> (i64, f32) {
    let hop = ((sample_rate as usize) / 40).max(128);
    let n_fft = (hop * 4).next_power_of_two().max(512);
    let ref_ch = chromagram_frames(reference, sample_rate, n_fft, hop);
    let oth_ch = chromagram_frames(other, sample_rate, n_fft, hop);
    if ref_ch.len() < 4 || oth_ch.len() < 4 {
        return (0, 0.0);
    }
    let max_lag_frames = (max_lag_samples / hop as i64).max(1);
    let mut best_lag_f = 0i64;
    let mut best_score = f32::NEG_INFINITY;
    let mut score_energy = 0.0f32;
    let mut score_count = 0usize;

    for lag in -max_lag_frames..=max_lag_frames {
        let mut sum = 0.0f32;
        let mut n = 0usize;
        for (i, r) in ref_ch.iter().enumerate() {
            let j = i as i64 + lag;
            if j < 0 || j as usize >= oth_ch.len() {
                continue;
            }
            let o = &oth_ch[j as usize];
            let mut dot = 0.0f32;
            for k in 0..12 {
                dot += r[k] * o[k];
            }
            sum += dot;
            n += 1;
        }
        if n < 4 {
            continue;
        }
        let score = sum / n as f32;
        score_energy += score * score;
        score_count += 1;
        if score > best_score {
            best_score = score;
            best_lag_f = lag;
        }
    }
    let mean = if score_count > 0 {
        (score_energy / score_count as f32).sqrt()
    } else {
        0.0
    };
    let confidence = if mean > 1e-6 {
        ((best_score - 0.3) / 0.5).clamp(0.0, 1.0)
            * (best_score / mean.max(1e-6) / 3.0).clamp(0.0, 1.0)
    } else {
        best_score.clamp(0.0, 1.0)
    };
    (best_lag_f * hop as i64, confidence.clamp(0.0, 1.0))
}

/// `(t_ms, offset_ms)` 线性回归 → `(offset_at_t0_ms, drift_ppm)`。
pub fn estimate_clock_drift(samples: &[(i64, i64)]) -> Option<(i64, f32)> {
    if samples.len() < 3 {
        return None;
    }
    let n = samples.len() as f64;
    let mut sum_t = 0.0;
    let mut sum_o = 0.0;
    let mut sum_tt = 0.0;
    let mut sum_to = 0.0;
    for &(t, o) in samples {
        let t = t as f64;
        let o = o as f64;
        sum_t += t;
        sum_o += o;
        sum_tt += t * t;
        sum_to += t * o;
    }
    let denom = n * sum_tt - sum_t * sum_t;
    if denom.abs() < 1.0 {
        return None;
    }
    let slope = (n * sum_to - sum_t * sum_o) / denom;
    let intercept = (sum_o - slope * sum_t) / n;
    let mut ss_res = 0.0;
    let mut ss_tot = 0.0;
    let mean_o = sum_o / n;
    for &(t, o) in samples {
        let pred = intercept + slope * t as f64;
        let e = o as f64 - pred;
        ss_res += e * e;
        let d = o as f64 - mean_o;
        ss_tot += d * d;
    }
    let r2 = if ss_tot > 1e-6 {
        1.0 - ss_res / ss_tot
    } else {
        0.0
    };
    if r2 < 0.5 {
        return None;
    }
    let ppm = (slope * 1_000_000.0) as f32;
    if !ppm.is_finite() || ppm.abs() > 2_000.0 {
        return None;
    }
    Some((intercept.round() as i64, ppm))
}

pub fn offset_with_drift(offset0_ms: i64, drift_ppm: f32, at_ms: i64) -> i64 {
    let delta = (drift_ppm as f64 * 1e-6) * at_ms as f64;
    offset0_ms + delta.round() as i64
}

pub fn offset_after_frame_step(offset_ms: i64, fps: f32, frames: i64) -> i64 {
    if fps <= 0.01 {
        return offset_ms + frames;
    }
    let frame_ms = (1000.0 / fps).round() as i64;
    offset_ms - frames * frame_ms.max(1)
}

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
        let (lag, conf) = cross_correlate_lag_limited(&a, &b, 5000);
        assert!(conf > 0.0);
        assert!((lag - 300).abs() < 50, "lag={lag}");
    }

    #[test]
    fn gcc_phat_detects_delay() {
        let mut a = vec![0.0f32; 8000];
        let mut b = vec![0.0f32; 8000];
        for i in 2000..2400 {
            a[i] = (i as f32 * 0.1).sin();
        }
        for i in 2500..2900 {
            b[i] = (i as f32 * 0.1).sin() * 0.3;
        }
        let (lag, conf) = gcc_phat_lag_limited(&a, &b, 5000);
        assert!(conf > 0.0, "conf={conf}");
        assert!((lag - 500).abs() < 80, "lag={lag}");
    }

    #[test]
    fn drift_fit_linear() {
        let pts: Vec<(i64, i64)> = (0..5)
            .map(|i| {
                let t = i * 10_000;
                (t, 100 + t / 1000)
            })
            .collect();
        let (o0, ppm) = estimate_clock_drift(&pts).expect("fit");
        assert!((o0 - 100).abs() < 5, "o0={o0}");
        assert!((ppm - 1000.0).abs() < 50.0, "ppm={ppm}");
    }
}
