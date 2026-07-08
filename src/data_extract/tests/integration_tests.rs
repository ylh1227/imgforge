//! 13 类 Imatest 模块解析集成测试。

use std::io::Write;
use std::path::Path;

use tempfile::tempdir;

use crate::data_extract::domain::ImatestModule;
use crate::data_extract::parser::aliases::map_field_to_metric;
use crate::data_extract::parser::module_detector::detect_module_from_path;
use crate::data_extract::service::DataExtractService;

fn write_module_csv(dir: &Path, folder: &str, header: &str, row: &str) -> std::path::PathBuf {
    let sub = dir.join(folder);
    std::fs::create_dir_all(&sub).unwrap();
    let path = sub.join("results.csv");
    let mut f = std::fs::File::create(&path).unwrap();
    writeln!(f, "{header}").unwrap();
    writeln!(f, "{row}").unwrap();
    path
}

#[test]
fn extract_all_thirteen_modules_from_directory() {
    let dir = tempdir().unwrap();
    let root = dir.path();

    let cases: [(ImatestModule, &str, &str, &str); 13] = [
        (
            ImatestModule::Distortion,
            "畸变Distortion",
            "ROI,TV Distortion (%),SMIA Distortion",
            "Center,1.2,0.8",
        ),
        (
            ImatestModule::DynamicRange,
            "动态范围DynamicRange",
            "Metric,Dynamic Range (dB),SNR (dB)",
            "A,68.5,42.1",
        ),
        (
            ImatestModule::ChromaticAberration,
            "横向色差ChromaticAberration",
            "ROI,CA (pixels),CA (%)",
            "Edge,2.1,0.5",
        ),
        (
            ImatestModule::ToneResponse,
            "灰阶响应ToneResponse",
            "Patch,Gamma,OECF Error",
            "Avg,2.2,0.03",
        ),
        (
            ImatestModule::ColorAccuracy,
            "色彩还原ColorAccuracy",
            "Patch,Mean Delta E,Max Delta E",
            "All,2.8,5.1",
        ),
        (
            ImatestModule::Mtf,
            "清晰度MTF",
            "ROI,MTF50,MTF30",
            "Center,0.45,0.32",
        ),
        (
            ImatestModule::TextureDetail,
            "细节纹理TextureDetail",
            "ROI,Texture Acutance,Dead Leaves",
            "Center,0.38,0.29",
        ),
        (
            ImatestModule::Fov,
            "视场角FOV",
            "Metric,Horizontal FOV,Vertical FOV",
            "Lens,62.0,48.5",
        ),
        (
            ImatestModule::Noise,
            "噪声Noise",
            "ISO,SNR (dB),Noise (%)",
            "800,36.5,1.2",
        ),
        (
            ImatestModule::Shading,
            "均匀性Shading",
            "Corner,Corner Falloff,Color Shading",
            "UL,12.5,2.1",
        ),
        (
            ImatestModule::DepthOfField,
            "景深DoF",
            "Metric,Near DOF,Far DOF",
            "Lens,0.8m,3.2m",
        ),
        (
            ImatestModule::ExposureError,
            "曝光误差ExposureError",
            "Scene,Exposure Error,EV Error",
            "Lab,-0.3,-0.3",
        ),
        (
            ImatestModule::LowLight,
            "低照度LowLight",
            "Scene,Low Light SNR,Low Light Delta E",
            "Night,28.4,4.2",
        ),
    ];

    for (module, folder, header, row) in cases {
        let path = write_module_csv(root, folder, header, row);
        assert_eq!(detect_module_from_path(&path), Some(module));
    }

    let batch = DataExtractService::extract_from_path(root).unwrap();
    assert_eq!(batch.files_scanned, 13);
    assert_eq!(batch.files_parsed, 13);
    assert_eq!(batch.modules_found().len(), 13);
    assert!(batch.record_count() >= 13);
}

#[test]
fn alias_mapping_covers_key_metrics() {
    let checks: [(ImatestModule, &str, &str); 8] = [
        (ImatestModule::Mtf, "MTF50", "mtf50"),
        (ImatestModule::ColorAccuracy, "Mean Delta E", "delta_e_mean"),
        (
            ImatestModule::Distortion,
            "TV Distortion (%)",
            "tv_distortion_pct",
        ),
        (ImatestModule::Noise, "SNR (dB)", "snr_db"),
        (ImatestModule::Fov, "Horizontal FOV", "fov_horizontal"),
        (ImatestModule::Shading, "Corner Falloff", "corner_falloff"),
        (
            ImatestModule::ExposureError,
            "EV Error",
            "exposure_error_ev",
        ),
        (ImatestModule::LowLight, "Low Light SNR", "low_light_snr"),
    ];
    for (module, raw, key) in checks {
        assert_eq!(map_field_to_metric(module, raw), Some(key));
    }
}
