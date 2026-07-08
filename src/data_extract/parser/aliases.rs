//! 各测试域常用字段别名。

use crate::data_extract::domain::ImatestModule;

/// 返回 `(metric_key, aliases)` 列表。
pub fn metric_aliases(module: ImatestModule) -> &'static [(&'static str, &'static [&'static str])] {
    match module {
        ImatestModule::Distortion => &[
            (
                "tv_distortion_pct",
                &[
                    "tv distortion",
                    "tv_distortion",
                    "tv distortion (%)",
                    "tv distortion %",
                ],
            ),
            (
                "smia_distortion",
                &["smia distortion", "smia_distortion", "smia tv distortion"],
            ),
            (
                "max_distortion_pct",
                &["max distortion", "maximum distortion", "max distortion (%)"],
            ),
            (
                "barrel_pincushion",
                &["barrel", "pincushion", "barrel/pincushion"],
            ),
        ],
        ImatestModule::DynamicRange => &[
            (
                "dynamic_range_db",
                &["dynamic range", "dynamic_range", "dr (db)"],
            ),
            ("snr_db", &["snr", "snr (db)", "signal to noise"]),
            ("stops", &["stops", "dynamic range (stops)"]),
        ],
        ImatestModule::ChromaticAberration => &[
            (
                "ca_pixels",
                &["ca", "chromatic aberration", "lateral ca", "ca (pixels)"],
            ),
            ("ca_percent", &["ca (%)", "chromatic aberration (%)"]),
        ],
        ImatestModule::ToneResponse => &[
            ("gamma", &["gamma", "avg gamma"]),
            ("oecf_error", &["oecf", "oecf error", "tone error"]),
            ("grayscale_error", &["grayscale error", "gray error"]),
        ],
        ImatestModule::ColorAccuracy => &[
            (
                "delta_e_mean",
                &[
                    "delta e",
                    "deltae",
                    "delta e mean",
                    "mean delta e",
                    "δe",
                    "avg delta e",
                    "delta e 00",
                ],
            ),
            (
                "delta_e_max",
                &["max delta e", "delta e max", "maximum delta e"],
            ),
            ("delta_e_94", &["delta e 94", "deltae94"]),
            (
                "white_balance_error",
                &["white balance", "wb error", "white balance error"],
            ),
            ("saturation_error", &["saturation error", "chroma error"]),
        ],
        ImatestModule::Mtf => &[
            (
                "mtf50",
                &[
                    "mtf50",
                    "mtf 50",
                    "mtf50 lp/ph",
                    "mtf50 (lp/ph)",
                    "mtf50 lp/mm",
                    "mtf50p",
                ],
            ),
            ("mtf30", &["mtf30", "mtf 30", "mtf30 lp/ph"]),
            (
                "mtf_nyquist",
                &["mtf at nyquist", "nyquist", "mtf nyquist", "mtf@nyquist"],
            ),
            ("mtf_center", &["center mtf", "mtf center", "center mtf50"]),
            ("mtf_corner", &["corner mtf", "mtf corner", "corner mtf50"]),
            ("mtf_weighted", &["weighted mtf", "mtf weighted average"]),
        ],
        ImatestModule::TextureDetail => &[
            ("texture_acutance", &["texture acutance", "acutance"]),
            ("texture_mtf", &["texture mtf", "dead leaves mtf"]),
            ("spilled_coins", &["spilled coins", "dead leaves"]),
        ],
        ImatestModule::Fov => &[
            ("fov_horizontal", &["horizontal fov", "hfov", "fov h"]),
            ("fov_vertical", &["vertical fov", "vfov", "fov v"]),
            ("fov_diagonal", &["diagonal fov", "dfov", "fov diagonal"]),
        ],
        ImatestModule::Noise => &[
            ("snr_db", &["snr", "snr (db)"]),
            ("noise_pct", &["noise (%)", "noise percent", "noise %"]),
            ("chroma_noise", &["chroma noise", "color noise"]),
            ("luma_noise", &["luma noise", "luminance noise"]),
        ],
        ImatestModule::Shading => &[
            ("shading_pct", &["shading", "nonuniformity", "vignetting"]),
            ("corner_falloff", &["corner falloff", "corner shading"]),
            ("color_shading", &["color shading", "color uniformity"]),
        ],
        ImatestModule::DepthOfField => &[
            ("near_dof", &["near dof", "near limit"]),
            ("far_dof", &["far dof", "far limit"]),
            ("total_dof", &["total dof", "depth of field"]),
        ],
        ImatestModule::ExposureError => &[
            (
                "exposure_error_ev",
                &["exposure error", "ev error", "exposure (ev)"],
            ),
            ("brightness_error", &["brightness error", "luminance error"]),
        ],
        ImatestModule::LowLight => &[
            ("low_light_snr", &["low light snr", "snr low light"]),
            (
                "low_light_delta_e",
                &["low light delta e", "color error low light"],
            ),
            ("low_light_exposure", &["low light exposure"]),
        ],
    }
}

/// 规范化字段名用于匹配。
pub fn normalize_field_name(name: &str) -> String {
    name.trim()
        .to_ascii_lowercase()
        .replace(['_', '-'], " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// 将原始字段名映射到标准 metric_key。
pub fn map_field_to_metric(module: ImatestModule, raw_name: &str) -> Option<&'static str> {
    let norm = normalize_field_name(raw_name);
    for (key, aliases) in metric_aliases(module) {
        for alias in *aliases {
            if norm == normalize_field_name(alias) || norm.contains(&normalize_field_name(alias)) {
                return Some(key);
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_mtf50_alias() {
        assert_eq!(
            map_field_to_metric(ImatestModule::Mtf, "MTF50 (lp/ph)"),
            Some("mtf50")
        );
    }
}
