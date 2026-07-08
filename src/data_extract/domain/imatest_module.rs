//! Imatest 测试域枚举（13 类）。

use serde::{Deserialize, Serialize};

/// Imatest 测试模块。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum ImatestModule {
    Distortion,
    DynamicRange,
    ChromaticAberration,
    ToneResponse,
    ColorAccuracy,
    Mtf,
    TextureDetail,
    Fov,
    Noise,
    Shading,
    DepthOfField,
    ExposureError,
    LowLight,
}

impl ImatestModule {
    pub const ALL: [ImatestModule; 13] = [
        ImatestModule::Distortion,
        ImatestModule::DynamicRange,
        ImatestModule::ChromaticAberration,
        ImatestModule::ToneResponse,
        ImatestModule::ColorAccuracy,
        ImatestModule::Mtf,
        ImatestModule::TextureDetail,
        ImatestModule::Fov,
        ImatestModule::Noise,
        ImatestModule::Shading,
        ImatestModule::DepthOfField,
        ImatestModule::ExposureError,
        ImatestModule::LowLight,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Distortion => "畸变 Distortion",
            Self::DynamicRange => "动态范围 Dynamic Range",
            Self::ChromaticAberration => "横向色差 Chromatic Aberration",
            Self::ToneResponse => "灰阶响应 Tone Response",
            Self::ColorAccuracy => "色彩还原 Color Accuracy",
            Self::Mtf => "清晰度 MTF",
            Self::TextureDetail => "细节纹理 Texture Detail",
            Self::Fov => "视场角 FOV",
            Self::Noise => "噪声 Noise",
            Self::Shading => "均匀性 Shading",
            Self::DepthOfField => "景深 DoF",
            Self::ExposureError => "曝光误差 Exposure Error",
            Self::LowLight => "低照度 Low-Light",
        }
    }

    pub fn short_label(self) -> &'static str {
        match self {
            Self::Distortion => "畸变",
            Self::DynamicRange => "动态范围",
            Self::ChromaticAberration => "横向色差",
            Self::ToneResponse => "灰阶响应",
            Self::ColorAccuracy => "色彩还原",
            Self::Mtf => "MTF",
            Self::TextureDetail => "纹理细节",
            Self::Fov => "FOV",
            Self::Noise => "噪声",
            Self::Shading => "均匀性",
            Self::DepthOfField => "景深",
            Self::ExposureError => "曝光误差",
            Self::LowLight => "低照度",
        }
    }

    /// 文件名/目录名关键字（小写匹配）。
    pub fn keywords(self) -> &'static [&'static str] {
        match self {
            Self::Distortion => &["distortion", "畸变", "tv_distortion", "smia"],
            Self::DynamicRange => &["dynamic_range", "dynamic range", "动态范围", "drange"],
            Self::ChromaticAberration => &[
                "chromatic_aberration",
                "chromatic aberration",
                "横向色差",
                "lateral_ca",
                "ca_",
            ],
            Self::ToneResponse => &["tone_response", "tone response", "灰阶", "oecf", "gamma"],
            Self::ColorAccuracy => &[
                "color_accuracy",
                "color accuracy",
                "色彩还原",
                "colorcheck",
                "delta_e",
                "deltae",
            ],
            Self::Mtf => &["mtf", "sfr", "清晰度", "sharpness", "esfr"],
            Self::TextureDetail => &[
                "texture",
                "texture_detail",
                "dead_leaves",
                "spilled_coins",
                "细节纹理",
            ],
            Self::Fov => &["fov", "field_of_view", "视场角", "field of view"],
            Self::Noise => &["noise", "噪声", "snr"],
            Self::Shading => &["shading", "uniformity", "均匀性", "vignetting", "falloff"],
            Self::DepthOfField => &["dof", "depth_of_field", "景深", "depth of field"],
            Self::ExposureError => &["exposure", "曝光", "exposure_error", "ev error"],
            Self::LowLight => &["low_light", "low-light", "低照度", "lowlight"],
        }
    }
}
