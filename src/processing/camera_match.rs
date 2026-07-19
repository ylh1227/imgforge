//! Vitrine 风格相机匹配：ridge 3×3 矩阵 + PAVA 单调曲线 + 感知加权 13³ 残差 LUT。
//!
//! 目标：将解马赛克后的 RAW（或其它源图）拟合到参考 JPG 的外观。
//! 失败时返回 `None`，由调用方 fail-open 回退到简单亮度增益。

use image::{DynamicImage, RgbaImage};

/// 分析网格（与 Vitrine 一致：cell 平均抹平锐化/噪点差异）。
pub const GW: usize = 200;
pub const GH: usize = 150;
const CURVE_KNOTS: usize = 33;
const RESIDUAL_N: usize = 13;
const RESIDUAL_LAMBDA: f64 = 0.5;
const RESIDUAL_ITERS: usize = 150;
const MIN_FIT_SAMPLES: usize = 500;
const ENC_BINS: usize = 4096;

#[inline]
fn s2l(v: f64) -> f64 {
    if v <= 0.04045 {
        v / 12.92
    } else {
        ((v + 0.055) / 1.055).powf(2.4)
    }
}

#[inline]
fn l2s(v: f64) -> f64 {
    if v <= 0.0031308 {
        v * 12.92
    } else {
        1.055 * v.powf(1.0 / 2.4) - 0.055
    }
}

/// 拟合得到的可序列化变换模型。
#[derive(Debug, Clone)]
pub struct CameraMatchModel {
    pub m: [[f64; 3]; 3],
    pub curves: [Vec<f64>; 3],
    pub residual: [Vec<f64>; 3],
    pub n: usize,
}

/// 将图像缩放到分析网格（fill），返回交错 RGB float 0..1，长度 `GW*GH*3`。
pub fn image_to_grid(image: &DynamicImage) -> Vec<f64> {
    let resized = image.resize_exact(
        GW as u32,
        GH as u32,
        image::imageops::FilterType::Triangle,
    );
    let rgba = resized.to_rgba8();
    let mut grid = vec![0.0f64; GW * GH * 3];
    for i in 0..GW * GH {
        let p = rgba.get_pixel((i % GW) as u32, (i / GW) as u32).0;
        grid[i * 3] = p[0] as f64 / 255.0;
        grid[i * 3 + 1] = p[1] as f64 / 255.0;
        grid[i * 3 + 2] = p[2] as f64 / 255.0;
    }
    grid
}

fn usable_cell(s: &[f64; 3], t: &[f64; 3]) -> bool {
    s.iter().chain(t.iter()).all(|&v| v > 0.005 && v < 0.995)
}

/// 线性域 ridge 最小二乘 3×3：求解 `M` 使 `M * s ≈ t`。
pub fn fit_matrix(pairs: &[([f64; 3], [f64; 3])]) -> Option<[[f64; 3]; 3]> {
    let mut a = [[0.0f64; 3]; 3];
    let mut b = [[0.0f64; 3]; 3];
    for (s, t) in pairs {
        for i in 0..3 {
            for j in 0..3 {
                a[i][j] += s[i] * s[j];
                b[i][j] += t[i] * s[j];
            }
        }
    }
    for i in 0..3 {
        a[i][i] += 1e-4;
    }
    let inv = invert3(a)?;
    let mut m = [[0.0f64; 3]; 3];
    for r in 0..3 {
        for c in 0..3 {
            for k in 0..3 {
                m[r][c] += b[r][k] * inv[k][c];
            }
        }
    }
    Some(m)
}

fn invert3(a: [[f64; 3]; 3]) -> Option<[[f64; 3]; 3]> {
    let (aa, bb, cc, d, e, f, g, h, i) = (
        a[0][0], a[0][1], a[0][2], a[1][0], a[1][1], a[1][2], a[2][0], a[2][1], a[2][2],
    );
    let det = aa * (e * i - f * h) - bb * (d * i - f * g) + cc * (d * h - e * g);
    if !det.is_finite() || det.abs() < 1e-12 {
        return None;
    }
    Some([
        [
            (e * i - f * h) / det,
            (cc * h - bb * i) / det,
            (bb * f - cc * e) / det,
        ],
        [
            (f * g - d * i) / det,
            (aa * i - cc * g) / det,
            (cc * d - aa * f) / det,
        ],
        [
            (d * h - e * g) / det,
            (bb * g - aa * h) / det,
            (aa * e - bb * d) / det,
        ],
    ])
}

#[inline]
fn apply_m(m: &[[f64; 3]; 3], s: &[f64; 3]) -> [f64; 3] {
    [
        m[0][0] * s[0] + m[0][1] * s[1] + m[0][2] * s[2],
        m[1][0] * s[0] + m[1][1] * s[1] + m[1][2] * s[2],
        m[2][0] * s[0] + m[2][1] * s[1] + m[2][2] * s[2],
    ]
}

/// 分箱条件均值 + PAVA 单调回归。
pub fn fit_curve(us: &[f64], vs: &[f64], knots: usize) -> Vec<f64> {
    let knots = knots.max(2);
    let mut sum = vec![0.0f64; knots];
    let mut cnt = vec![0.0f64; knots];
    for (&u, &v) in us.iter().zip(vs.iter()) {
        let b = ((u * (knots - 1) as f64).round() as isize)
            .clamp(0, (knots - 1) as isize) as usize;
        sum[b] += v;
        cnt[b] += 1.0;
    }
    let mut y = vec![0.0f64; knots];
    let mut last = 0.0;
    for b in 0..knots {
        y[b] = if cnt[b] > 0.0 {
            sum[b] / cnt[b]
        } else {
            last
        };
        last = y[b];
    }

    // PAVA：level = (value, weight, count)
    let mut level: Vec<(f64, f64, usize)> = Vec::new();
    for b in 0..knots {
        let mut v = y[b];
        let mut wt = cnt[b].max(1e-6);
        let mut n = 1usize;
        while level
            .last()
            .is_some_and(|(pv, _, _)| *pv > v)
        {
            let (pv, pw, pn) = level.pop().unwrap();
            v = (pv * pw + v * wt) / (pw + wt);
            wt += pw;
            n += pn;
        }
        level.push((v, wt, n));
    }

    let mut curve = vec![0.0f64; knots];
    let mut b2 = 0usize;
    for (v, _, n) in level {
        for _ in 0..n {
            if b2 < knots {
                curve[b2] = v;
                b2 += 1;
            }
        }
    }
    // 填满（数值边界）
    while b2 < knots {
        curve[b2] = curve[b2 - 1];
        b2 += 1;
    }
    curve
}

pub fn eval_curve(curve: &[f64], u: f64) -> f64 {
    let k = curve.len();
    if k == 0 {
        return u;
    }
    if k == 1 {
        return curve[0];
    }
    let t = u.clamp(0.0, 1.0) * (k - 1) as f64;
    let i0 = t.floor() as usize;
    let i1 = (i0 + 1).min(k - 1);
    let f = t - i0 as f64;
    curve[i0] + (curve[i1] - curve[i0]) * f
}

/// 残差 3D LUT：编码域 post-(matrix+curves) → 目标残差，Laplacian 平滑。
pub fn fit_residual_lut(
    samples: &[([f64; 3], [f64; 3])],
    weights: &[f64],
    n: usize,
    lambda: f64,
    iters: usize,
) -> [Vec<f64>; 3] {
    let n3 = n * n * n;
    let mut w_sum = vec![0.0f64; n3];
    let mut t_sum = [vec![0.0f64; n3], vec![0.0f64; n3], vec![0.0f64; n3]];
    let idx = |r: usize, g: usize, b: usize| (r * n + g) * n + b;

    for (k, (s, t)) in samples.iter().enumerate() {
        let sw = weights.get(k).copied().unwrap_or(1.0);
        let fr = (s[0] * (n - 1) as f64).clamp(0.0, (n - 1) as f64 - 1e-4);
        let fg = (s[1] * (n - 1) as f64).clamp(0.0, (n - 1) as f64 - 1e-4);
        let fb = (s[2] * (n - 1) as f64).clamp(0.0, (n - 1) as f64 - 1e-4);
        let r0 = fr.floor() as usize;
        let g0 = fg.floor() as usize;
        let b0 = fb.floor() as usize;
        let dr = fr - r0 as f64;
        let dg = fg - g0 as f64;
        let db = fb - b0 as f64;
        for cr in 0..2 {
            for cg in 0..2 {
                for cb in 0..2 {
                    let w = (if cr == 1 { dr } else { 1.0 - dr })
                        * (if cg == 1 { dg } else { 1.0 - dg })
                        * (if cb == 1 { db } else { 1.0 - db })
                        * sw;
                    if w < 1e-8 {
                        continue;
                    }
                    let ii = idx(r0 + cr, g0 + cg, b0 + cb);
                    w_sum[ii] += w;
                    for c in 0..3 {
                        t_sum[c][ii] += w * (t[c] - s[c]);
                    }
                }
            }
        }
    }

    let mut lut = [vec![0.0f64; n3], vec![0.0f64; n3], vec![0.0f64; n3]];
    for _ in 0..iters {
        for c in 0..3 {
            let cur = &lut[c];
            let mut next = vec![0.0f64; n3];
            for r in 0..n {
                for g in 0..n {
                    for b in 0..n {
                        let ii = idx(r, g, b);
                        let mut nb = 0.0;
                        let mut nb_sum = 0.0;
                        if r > 0 {
                            nb += 1.0;
                            nb_sum += cur[idx(r - 1, g, b)];
                        }
                        if r + 1 < n {
                            nb += 1.0;
                            nb_sum += cur[idx(r + 1, g, b)];
                        }
                        if g > 0 {
                            nb += 1.0;
                            nb_sum += cur[idx(r, g - 1, b)];
                        }
                        if g + 1 < n {
                            nb += 1.0;
                            nb_sum += cur[idx(r, g + 1, b)];
                        }
                        if b > 0 {
                            nb += 1.0;
                            nb_sum += cur[idx(r, g, b - 1)];
                        }
                        if b + 1 < n {
                            nb += 1.0;
                            nb_sum += cur[idx(r, g, b + 1)];
                        }
                        next[ii] = (t_sum[c][ii] + lambda * nb_sum) / (w_sum[ii] + lambda * nb + 1e-9);
                    }
                }
            }
            lut[c] = next;
        }
    }
    lut
}

/// 从解码网格与相机参考网格拟合完整变换；样本不足返回 `None`。
pub fn fit_transform(dec_grid: &[f64], cam_grid: &[f64]) -> Option<CameraMatchModel> {
    debug_assert_eq!(dec_grid.len(), GW * GH * 3);
    debug_assert_eq!(cam_grid.len(), GW * GH * 3);

    let mut pairs: Vec<([f64; 3], [f64; 3])> = Vec::new();
    for i in 0..GW * GH {
        let s = [
            dec_grid[i * 3],
            dec_grid[i * 3 + 1],
            dec_grid[i * 3 + 2],
        ];
        let t = [
            cam_grid[i * 3],
            cam_grid[i * 3 + 1],
            cam_grid[i * 3 + 2],
        ];
        if usable_cell(&s, &t) {
            pairs.push((
                [s2l(s[0]), s2l(s[1]), s2l(s[2])],
                [s2l(t[0]), s2l(t[1]), s2l(t[2])],
            ));
        }
    }
    if pairs.len() < MIN_FIT_SAMPLES {
        return None;
    }

    let m = fit_matrix(&pairs)?;

    let mut us = [Vec::new(), Vec::new(), Vec::new()];
    let mut vs = [Vec::new(), Vec::new(), Vec::new()];
    for (s, t) in &pairs {
        let mm = apply_m(&m, s);
        for c in 0..3 {
            us[c].push(l2s(mm[c].clamp(0.0, 1.0)));
            vs[c].push(l2s(t[c].clamp(0.0, 1.0)));
        }
    }
    let curves = [
        fit_curve(&us[0], &vs[0], CURVE_KNOTS),
        fit_curve(&us[1], &vs[1], CURVE_KNOTS),
        fit_curve(&us[2], &vs[2], CURVE_KNOTS),
    ];

    let mut rsamples = Vec::with_capacity(GW * GH);
    let mut rweights = Vec::with_capacity(GW * GH);
    for i in 0..GW * GH {
        let s = [
            dec_grid[i * 3],
            dec_grid[i * 3 + 1],
            dec_grid[i * 3 + 2],
        ];
        let t = [
            cam_grid[i * 3],
            cam_grid[i * 3 + 1],
            cam_grid[i * 3 + 2],
        ];
        let mm = apply_m(&m, &[s2l(s[0]), s2l(s[1]), s2l(s[2])]);
        let a = [
            eval_curve(&curves[0], l2s(mm[0].clamp(0.0, 1.0))).clamp(0.0, 1.0),
            eval_curve(&curves[1], l2s(mm[1].clamp(0.0, 1.0))).clamp(0.0, 1.0),
            eval_curve(&curves[2], l2s(mm[2].clamp(0.0, 1.0))).clamp(0.0, 1.0),
        ];
        rsamples.push((a, t));
        let y = 0.2126 * s2l(t[0]) + 0.7152 * s2l(t[1]) + 0.0722 * s2l(t[2]);
        rweights.push(1.0 / (y + 0.05));
    }
    let residual = fit_residual_lut(
        &rsamples,
        &rweights,
        RESIDUAL_N,
        RESIDUAL_LAMBDA,
        RESIDUAL_ITERS,
    );

    Some(CameraMatchModel {
        m,
        curves,
        residual,
        n: RESIDUAL_N,
    })
}

/// 对源图拟合并施加相机匹配。成功返回 `true`；无法拟合返回 `false`（fail-open）。
pub fn try_apply_camera_match(image: &mut DynamicImage, reference: &DynamicImage) -> bool {
    let dec = image_to_grid(image);
    let cam = image_to_grid(reference);
    let Some(model) = fit_transform(&dec, &cam) else {
        return false;
    };
    apply_model(image, &model);
    true
}

fn apply_model(image: &mut DynamicImage, model: &CameraMatchModel) {
    let mut rgba = image.to_rgba8();
    apply_model_rgba(&mut rgba, model);
    *image = DynamicImage::ImageRgba8(rgba);
}

fn apply_model_rgba(rgba: &mut RgbaImage, model: &CameraMatchModel) {
    let n = model.n;
    let nm1 = (n - 1) as f64;

    // u8 → linear LUT
    let mut s2l_tab = [0.0f64; 256];
    for i in 0..256 {
        s2l_tab[i] = s2l(i as f64 / 255.0);
    }

    // matrix-output linear 0..1 → post-curve encoded（每通道）
    let mut enc = [[0.0f64; ENC_BINS]; 3];
    for c in 0..3 {
        for i in 0..ENC_BINS {
            let m = i as f64 / (ENC_BINS - 1) as f64;
            enc[c][i] = eval_curve(&model.curves[c], l2s(m)).clamp(0.0, 1.0);
        }
    }

    let m = model.m;
    let r0 = &model.residual[0];
    let r1 = &model.residual[1];
    let r2 = &model.residual[2];
    let idx = |rr: usize, gg: usize, bb: usize| (rr * n + gg) * n + bb;

    for p in rgba.pixels_mut() {
        let lr = s2l_tab[p.0[0] as usize];
        let lg = s2l_tab[p.0[1] as usize];
        let lb = s2l_tab[p.0[2] as usize];

        let mut mr = m[0][0] * lr + m[0][1] * lg + m[0][2] * lb;
        let mut mg = m[1][0] * lr + m[1][1] * lg + m[1][2] * lb;
        let mut mb = m[2][0] * lr + m[2][1] * lg + m[2][2] * lb;
        mr = mr.clamp(0.0, 1.0);
        mg = mg.clamp(0.0, 1.0);
        mb = mb.clamp(0.0, 1.0);

        let mut er = enc[0][(mr * (ENC_BINS - 1) as f64) as usize];
        let mut eg = enc[1][(mg * (ENC_BINS - 1) as f64) as usize];
        let mut eb = enc[2][(mb * (ENC_BINS - 1) as f64) as usize];

        let fr = er * nm1;
        let fg = eg * nm1;
        let fb = eb * nm1;
        let rr0 = (fr.floor() as usize).min(n - 2);
        let gg0 = (fg.floor() as usize).min(n - 2);
        let bb0 = (fb.floor() as usize).min(n - 2);
        let dr = fr - rr0 as f64;
        let dg = fg - gg0 as f64;
        let db = fb - bb0 as f64;

        let i000 = idx(rr0, gg0, bb0);
        let i001 = i000 + 1;
        let i010 = i000 + n;
        let i011 = i010 + 1;
        let i100 = i000 + n * n;
        let i101 = i100 + 1;
        let i110 = i100 + n;
        let i111 = i110 + 1;

        let w000 = (1.0 - dr) * (1.0 - dg) * (1.0 - db);
        let w001 = (1.0 - dr) * (1.0 - dg) * db;
        let w010 = (1.0 - dr) * dg * (1.0 - db);
        let w011 = (1.0 - dr) * dg * db;
        let w100 = dr * (1.0 - dg) * (1.0 - db);
        let w101 = dr * (1.0 - dg) * db;
        let w110 = dr * dg * (1.0 - db);
        let w111 = dr * dg * db;

        er += w000 * r0[i000]
            + w001 * r0[i001]
            + w010 * r0[i010]
            + w011 * r0[i011]
            + w100 * r0[i100]
            + w101 * r0[i101]
            + w110 * r0[i110]
            + w111 * r0[i111];
        eg += w000 * r1[i000]
            + w001 * r1[i001]
            + w010 * r1[i010]
            + w011 * r1[i011]
            + w100 * r1[i100]
            + w101 * r1[i101]
            + w110 * r1[i110]
            + w111 * r1[i111];
        eb += w000 * r2[i000]
            + w001 * r2[i001]
            + w010 * r2[i010]
            + w011 * r2[i011]
            + w100 * r2[i100]
            + w101 * r2[i101]
            + w110 * r2[i110]
            + w111 * r2[i111];

        p.0[0] = (er.clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
        p.0[1] = (eg.clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
        p.0[2] = (eb.clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn synthetic_grid() -> Vec<f64> {
        let mut g = vec![0.0f64; GW * GH * 3];
        for y in 0..GH {
            for x in 0..GW {
                let i = (y * GW + x) * 3;
                g[i] = 0.05 + 0.9 * (x as f64 / (GW - 1) as f64);
                g[i + 1] = 0.05 + 0.9 * (y as f64 / (GH - 1) as f64);
                g[i + 2] = 0.05 + 0.9 * ((x + y) as f64 / (GW + GH - 2) as f64);
            }
        }
        g
    }

    #[test]
    fn fit_matrix_recovers_known() {
        let true_m = [
            [0.9, 0.08, 0.02],
            [0.05, 0.92, 0.03],
            [0.01, 0.06, 0.93],
        ];
        let mut seed = 1u64;
        let mut rand = || {
            seed = seed.wrapping_mul(16807) % 2147483647;
            seed as f64 / 2147483647.0
        };
        let mut pairs = Vec::new();
        for _ in 0..2000 {
            let s = [rand() * 0.9 + 0.02, rand() * 0.9 + 0.02, rand() * 0.9 + 0.02];
            let t = apply_m(&true_m, &s);
            pairs.push((s, t));
        }
        let m = fit_matrix(&pairs).unwrap();
        for r in 0..3 {
            for c in 0..3 {
                assert!(
                    (m[r][c] - true_m[r][c]).abs() < 0.02,
                    "m[{r}][{c}]={} vs {}",
                    m[r][c],
                    true_m[r][c]
                );
            }
        }
    }

    #[test]
    fn fit_curve_monotone_on_noise() {
        let mut seed = 7u64;
        let mut rand = || {
            seed = seed.wrapping_mul(16807) % 2147483647;
            seed as f64 / 2147483647.0
        };
        let mut us = Vec::new();
        let mut vs = Vec::new();
        for _ in 0..3000 {
            let u = rand();
            us.push(u);
            vs.push(u * 0.8 + (rand() - 0.5) * 0.3);
        }
        let curve = fit_curve(&us, &vs, CURVE_KNOTS);
        for i in 1..curve.len() {
            assert!(curve[i] + 1e-12 >= curve[i - 1]);
        }
    }

    #[test]
    fn fit_curve_tracks_gamma() {
        let us: Vec<f64> = (0..5000).map(|i| i as f64 / 4999.0).collect();
        let vs: Vec<f64> = us.iter().map(|u| u.powf(0.8)).collect();
        let curve = fit_curve(&us, &vs, CURVE_KNOTS);
        for u in [0.1, 0.3, 0.5, 0.7, 0.9] {
            let got = eval_curve(&curve, u);
            let expect = u.powf(0.8);
            assert!((got - expect).abs() < 0.05, "u={u} got={got} expect={expect}");
        }
    }

    #[test]
    fn fit_transform_round_trips_synthetic_look() {
        let dec = synthetic_grid();
        let mut cam = vec![0.0f64; dec.len()];
        for i in 0..GW * GH {
            let lin = [
                s2l(dec[i * 3]),
                s2l(dec[i * 3 + 1]),
                s2l(dec[i * 3 + 2]),
            ];
            let mixed = [
                (1.32 * (0.95 * lin[0] + 0.05 * lin[1])).min(1.0),
                (1.32 * (0.03 * lin[0] + 0.94 * lin[1] + 0.03 * lin[2])).min(1.0),
                (1.32 * (0.06 * lin[1] + 0.94 * lin[2])).min(1.0),
            ];
            for c in 0..3 {
                cam[i * 3 + c] = l2s(mixed[c]).powf(1.1);
            }
        }
        let model = fit_transform(&dec, &cam).expect("fit");

        let apply_one = |r: f64, g: f64, b: f64| -> [f64; 3] {
            let lin = [s2l(r), s2l(g), s2l(b)];
            let mm = apply_m(&model.m, &lin);
            let enc = [
                eval_curve(&model.curves[0], l2s(mm[0].clamp(0.0, 1.0))).clamp(0.0, 1.0),
                eval_curve(&model.curves[1], l2s(mm[1].clamp(0.0, 1.0))).clamp(0.0, 1.0),
                eval_curve(&model.curves[2], l2s(mm[2].clamp(0.0, 1.0))).clamp(0.0, 1.0),
            ];
            let n = model.n;
            let fr = (enc[0] * (n - 1) as f64).min((n - 1) as f64 - 1e-4);
            let fg = (enc[1] * (n - 1) as f64).min((n - 1) as f64 - 1e-4);
            let fb = (enc[2] * (n - 1) as f64).min((n - 1) as f64 - 1e-4);
            let r0 = fr.floor() as usize;
            let g0 = fg.floor() as usize;
            let b0 = fb.floor() as usize;
            let dr = fr - r0 as f64;
            let dg = fg - g0 as f64;
            let db = fb - b0 as f64;
            let mut out = enc;
            for cr in 0..2 {
                for cg in 0..2 {
                    for cb in 0..2 {
                        let w = (if cr == 1 { dr } else { 1.0 - dr })
                            * (if cg == 1 { dg } else { 1.0 - dg })
                            * (if cb == 1 { db } else { 1.0 - db });
                        let ii = ((r0 + cr) * n + (g0 + cg)) * n + (b0 + cb);
                        for c in 0..3 {
                            out[c] += w * model.residual[c][ii];
                        }
                    }
                }
            }
            [
                out[0].clamp(0.0, 1.0),
                out[1].clamp(0.0, 1.0),
                out[2].clamp(0.0, 1.0),
            ]
        };

        let mut worst = 0.0f64;
        let mut sum = 0.0f64;
        let mut n = 0usize;
        for i in (0..GW * GH).step_by(7) {
            let s = [dec[i * 3], dec[i * 3 + 1], dec[i * 3 + 2]];
            let t = [cam[i * 3], cam[i * 3 + 1], cam[i * 3 + 2]];
            if s.iter().any(|&v| v < 0.05 || v > 0.95) || t.iter().any(|&v| v < 0.05 || v > 0.95)
            {
                continue;
            }
            let o = apply_one(s[0], s[1], s[2]);
            let err = (o[0] - t[0])
                .abs()
                .max((o[1] - t[1]).abs())
                .max((o[2] - t[2]).abs());
            worst = worst.max(err);
            sum += err;
            n += 1;
        }
        assert!(n > 1000, "n={n}");
        assert!(sum / (n as f64) < 0.03, "mean err={}", sum / (n as f64));
        assert!(worst < 0.12, "worst={worst}");
    }

    #[test]
    fn clipped_grids_return_none() {
        let dec = vec![1.0f64; GW * GH * 3];
        let cam = vec![1.0f64; GW * GH * 3];
        assert!(fit_transform(&dec, &cam).is_none());
    }
}
