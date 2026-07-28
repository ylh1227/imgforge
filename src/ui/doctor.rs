//! 环境与 feature 诊断。

use std::process::Command;

use serde::Serialize;

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

/// 结构化诊断报告（Host / Flutter 用）。
#[derive(Debug, Clone, Serialize)]
pub struct DoctorReport {
    pub version: String,
    pub rustc: String,
    pub cpu_cores: usize,
    pub platform: String,
    pub features: Vec<DoctorFeature>,
    pub tools: Vec<DoctorTool>,
    pub remote: DoctorRemote,
    pub jira: DoctorJira,
}

#[derive(Debug, Clone, Serialize)]
pub struct DoctorFeature {
    pub name: String,
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct DoctorTool {
    pub name: String,
    pub available: bool,
    pub detail: String,
    pub required: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct DoctorRemote {
    pub status: String,
    pub enabled: bool,
    pub base_url: Option<String>,
    pub configured: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct DoctorJira {
    pub status: String,
    pub enabled: bool,
    pub base_url: Option<String>,
    pub project_key: Option<String>,
    pub has_credentials: bool,
}

/// 收集结构化 doctor 报告。
pub fn doctor_report() -> DoctorReport {
    let features = [
        ("gui", cfg!(feature = "gui")),
        ("host", cfg!(feature = "host")),
        ("review", cfg!(feature = "review")),
        ("video-review", cfg!(feature = "video-review")),
        ("data-extract", cfg!(feature = "data-extract")),
        ("ocr", cfg!(feature = "ocr")),
        ("incremental", cfg!(feature = "incremental")),
        ("rename", cfg!(feature = "rename")),
        ("thumbnails", cfg!(feature = "thumbnails")),
        ("watermark", cfg!(feature = "watermark")),
        ("avif", cfg!(feature = "avif")),
        ("jpegxl", cfg!(feature = "jpegxl")),
        ("bayer", cfg!(feature = "bayer")),
        ("vips", cfg!(feature = "vips")),
    ]
    .into_iter()
    .map(|(name, enabled)| DoctorFeature {
        name: name.into(),
        enabled,
    })
    .collect();

    let mut tools = vec![
        probe_tool(
            "ffmpeg",
            "ffmpeg",
            &["-version"],
            cfg!(feature = "video-review"),
        ),
        probe_tool(
            "ffprobe",
            "ffprobe",
            &["-version"],
            cfg!(feature = "video-review"),
        ),
        probe_tool(
            "tesseract",
            "tesseract",
            &["--version"],
            cfg!(feature = "data-extract"),
        ),
    ];

    #[cfg(feature = "video-review")]
    {
        use crate::video_review::service::VideoBackend;
        let backend = crate::video_review::service::FfmpegBackend::with_defaults();
        let avail = backend.availability();
        if let Some(v) = avail.ffmpeg_version {
            if let Some(t) = tools.iter_mut().find(|t| t.name == "ffmpeg") {
                t.detail = v;
                t.available = avail.ffmpeg_ok;
            }
        }
        if let Some(v) = avail.ffprobe_version {
            if let Some(t) = tools.iter_mut().find(|t| t.name == "ffprobe") {
                t.detail = v;
                t.available = avail.ffprobe_ok;
            }
        }
    }

    let mut remote = crate::remote::RemoteConfig::default();
    remote.apply_env_overrides();
    let jira = crate::jira::load_jira_config();

    DoctorReport {
        version: env!("CARGO_PKG_VERSION").into(),
        rustc: rustc_version(),
        cpu_cores: num_cpus::get(),
        platform: std::env::consts::OS.into(),
        features,
        tools,
        remote: DoctorRemote {
            status: remote.status_label().to_string(),
            enabled: remote.enabled,
            base_url: remote.base_url.clone(),
            configured: remote.is_configured(),
        },
        jira: DoctorJira {
            status: jira.status_label().to_string(),
            enabled: jira.enabled,
            base_url: jira.base_url.clone(),
            project_key: jira.project_key.clone(),
            has_credentials: jira.has_credentials(),
        },
    }
}

fn probe_tool(name: &str, bin: &str, args: &[&str], required: bool) -> DoctorTool {
    if !required {
        return DoctorTool {
            name: name.into(),
            available: false,
            detail: "not required (feature disabled)".into(),
            required: false,
        };
    }
    match crate::process_util::command(bin).args(args).output() {
        Ok(out) if out.status.success() => {
            let first = String::from_utf8_lossy(&out.stdout)
                .lines()
                .next()
                .unwrap_or("available")
                .to_string();
            DoctorTool {
                name: name.into(),
                available: true,
                detail: if first.is_empty() {
                    "available".into()
                } else {
                    first
                },
                required: true,
            }
        }
        Ok(out) => DoctorTool {
            name: name.into(),
            available: false,
            detail: format!("exit {}", out.status.code().unwrap_or(-1)),
            required: true,
        },
        Err(e) => DoctorTool {
            name: name.into(),
            available: false,
            detail: e.to_string(),
            required: true,
        },
    }
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
