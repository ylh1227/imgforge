//! 链接 libmpv 时补充 Homebrew 库搜索路径（macOS）。

fn main() {
    if std::env::var("CARGO_FEATURE_MPV").is_err() {
        return;
    }
    #[cfg(target_os = "macos")]
    {
        for candidate in [
            "/opt/homebrew/lib",
            "/usr/local/lib",
            "/opt/homebrew/opt/mpv/lib",
            "/usr/local/opt/mpv/lib",
        ] {
            let path = std::path::Path::new(candidate);
            if path.join("libmpv.dylib").exists() || path.join("libmpv.2.dylib").exists() {
                println!("cargo:rustc-link-search=native={candidate}");
            }
        }
        if let Ok(output) = std::process::Command::new("brew")
            .args(["--prefix", "mpv"])
            .output()
        {
            if output.status.success() {
                let prefix = String::from_utf8_lossy(&output.stdout).trim().to_string();
                if !prefix.is_empty() {
                    let lib = format!("{prefix}/lib");
                    println!("cargo:rustc-link-search=native={lib}");
                }
            }
        }
        println!("cargo:rerun-if-changed=build.rs");
    }
}
