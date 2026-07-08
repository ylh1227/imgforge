//! 环境与 feature 诊断。

/// 打印运行时环境与已启用 feature 状态。
pub fn run_doctor() {
    println!("imgforge doctor — environment check");
    println!("───────────────────────────────────────");
    println!("Version:     {}", env!("CARGO_PKG_VERSION"));
    println!("Rust:        {}", rustc_version());
    println!("CPU cores:   {}", num_cpus::get());
    print_features();
    print_backend_status();
    println!("───────────────────────────────────────");
}

fn rustc_version() -> String {
    std::process::Command::new("rustc")
        .arg("--version")
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|| "unknown".into())
}

fn print_features() {
    let features: &[(&str, bool)] = &[
        ("incremental", cfg!(feature = "incremental")),
        ("rename", cfg!(feature = "rename")),
        ("thumbnails", cfg!(feature = "thumbnails")),
        ("watermark", cfg!(feature = "watermark")),
        ("avif", cfg!(feature = "avif")),
        ("avif-decode", cfg!(feature = "avif-decode")),
        ("jpegxl", cfg!(feature = "jpegxl")),
        ("vips", cfg!(feature = "vips")),
    ];
    println!("Features:");
    for (name, enabled) in features {
        let status = if *enabled { "enabled" } else { "disabled" };
        println!("  {name:16} {status}");
    }
}

fn print_backend_status() {
    println!("Backends:");
    println!("  native           available");
    println!("  platform         {}", std::env::consts::OS);
    #[cfg(windows)]
    println!("  long paths       enable Win10+ long path support for best results");
    #[cfg(feature = "vips")]
    {
        let status = crate::processing::backends::vips_backend::probe_vips()
            .map(|s| s.to_string())
            .unwrap_or_else(|e| format!("unavailable ({e})"));
        println!("  vips             {status}");
    }
    #[cfg(not(feature = "vips"))]
    println!("  vips             not compiled (rebuild with --features vips)");
}
