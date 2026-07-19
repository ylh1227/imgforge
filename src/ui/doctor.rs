//! 环境与 feature 诊断。

use std::process::Command;

/// 打印运行时环境与已启用 feature 状态。
pub fn run_doctor() {
    println!("imgforge doctor — environment check");
    println!("───────────────────────────────────────");
    println!("Version:     {}", env!("CARGO_PKG_VERSION"));
    println!("Rust:        {}", rustc_version());
    println!("CPU cores:   {}", num_cpus::get());
    print_features();
    print_backend_status();
    print_runtime_dependencies();
    print_remote_status();
    print_jira_status();
    println!("───────────────────────────────────────");
}

fn rustc_version() -> String {
    Command::new("rustc")
        .arg("--version")
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|| "unknown".into())
}

fn print_features() {
    let features: &[(&str, bool)] = &[
        ("gui", cfg!(feature = "gui")),
        ("review", cfg!(feature = "review")),
        ("video-review", cfg!(feature = "video-review")),
        ("data-extract", cfg!(feature = "data-extract")),
        ("ocr", cfg!(feature = "ocr")),
        ("incremental", cfg!(feature = "incremental")),
        ("rename", cfg!(feature = "rename")),
        ("thumbnails", cfg!(feature = "thumbnails")),
        ("watermark", cfg!(feature = "watermark")),
        ("avif", cfg!(feature = "avif")),
        ("avif-decode", cfg!(feature = "avif-decode")),
        ("jpegxl", cfg!(feature = "jpegxl")),
        ("bayer", cfg!(feature = "bayer")),
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

fn print_runtime_dependencies() {
    println!("Runtime dependencies:");
    print_tool_status(
        "ffmpeg",
        "ffmpeg",
        &["-version"],
        cfg!(feature = "video-review"),
    );
    print_tool_status(
        "ffprobe",
        "ffprobe",
        &["-version"],
        cfg!(feature = "video-review"),
    );
    print_tool_status(
        "tesseract",
        "tesseract",
        &["--version"],
        cfg!(feature = "data-extract"),
    );

    #[cfg(feature = "video-review")]
    {
        use crate::video_review::service::VideoBackend;
        let backend = crate::video_review::service::FfmpegBackend::with_defaults();
        let avail = backend.availability();
        if avail.ffmpeg_ok {
            if let Some(v) = avail.ffmpeg_version {
                println!("  ffmpeg detail   {v}");
            }
        }
        if avail.ffprobe_ok {
            if let Some(v) = avail.ffprobe_version {
                println!("  ffprobe detail  {v}");
            }
        }
    }

    #[cfg(feature = "data-extract")]
    {
        let ocr = crate::data_extract::ocr::check_availability();
        println!(
            "  tesseract detail {}",
            if ocr.tesseract_ok {
                ocr.detail
            } else {
                format!("unavailable ({})", ocr.detail)
            }
        );
    }

    #[cfg(feature = "vips")]
    {
        match crate::processing::backends::vips_backend::probe_vips() {
            Ok(s) => println!("  libvips detail  {s}"),
            Err(e) => println!("  libvips detail  unavailable ({e})"),
        }
    }
}

fn print_remote_status() {
    let mut remote = crate::remote::RemoteConfig::default();
    remote.apply_env_overrides();
    println!("Remote:");
    println!("  status           {}", remote.status_label());
    println!("  enabled          {}", remote.enabled);
    println!(
        "  base_url         {}",
        remote.base_url.as_deref().unwrap_or("(none)")
    );
    println!("  auth_mode        {}", remote.auth_mode.label());
    println!(
        "  token            {}",
        if remote.resolve_token().is_some() {
            "present"
        } else {
            "absent"
        }
    );
    println!(
        "  cache            {}",
        remote.resolved_cache_path().display()
    );
    println!(
        "  http_client      {}",
        if remote.is_configured() {
            "reqwest (blocking JSON)"
        } else {
            "idle (configure base_url to enable)"
        }
    );
}

fn print_jira_status() {
    let jira = crate::jira::load_jira_config();
    println!("JIRA:");
    println!("  status           {}", jira.status_label());
    println!("  enabled          {}", jira.enabled);
    println!(
        "  base_url         {}",
        jira.base_url.as_deref().unwrap_or("(none)")
    );
    println!(
        "  project_key      {}",
        jira.project_key.as_deref().unwrap_or("(none)")
    );
    println!("  auth_mode        {}", jira.auth_mode.label());
    println!("  api_version      {}", jira.api_version.label());
    println!(
        "  credentials      {}",
        if jira.has_credentials() {
            "present"
        } else {
            "absent"
        }
    );
}

fn print_tool_status(label: &str, bin: &str, args: &[&str], relevant: bool) {
    if !relevant {
        println!("  {label:16} not required (feature disabled)");
        return;
    }

    match crate::process_util::command(bin).args(args).output() {
        Ok(out) if out.status.success() => {
            let first = String::from_utf8_lossy(&out.stdout)
                .lines()
                .next()
                .unwrap_or("available")
                .to_string();
            let detail = if first.is_empty() {
                "available".to_string()
            } else {
                first
            };
            println!("  {label:16} available ({detail})");
        }
        Ok(out) => {
            println!(
                "  {label:16} unavailable (exit {})",
                out.status.code().unwrap_or(-1)
            );
        }
        Err(e) => {
            println!("  {label:16} unavailable ({e})");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn doctor_runs_without_panic() {
        run_doctor();
    }
}
