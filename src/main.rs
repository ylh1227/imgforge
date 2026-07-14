//! 程序入口：命令行模式。

mod cli;
mod cli_loader;

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use clap::Parser;
use eyre::WrapErr;
use imgforge::job::run_batch;
use imgforge::ui;

use crate::cli::{Cli, Commands, RemoteCommands, RemoteSubmitSource};
use crate::cli_loader::load_config;

fn run_remote_command(cli: &Cli, command: &RemoteCommands) -> eyre::Result<()> {
    match command {
        RemoteCommands::Status | RemoteCommands::Pull { .. } => {
            let mut remote = imgforge::remote::RemoteConfig::default();
            remote.apply_env_overrides();
            if let Ok(raw) = std::fs::read_to_string("imgforge.toml") {
                if let Ok(cfg) = toml::from_str::<imgforge::config::AppConfig>(&raw) {
                    remote = cfg.remote;
                    remote.apply_env_overrides();
                }
            }
            // CLI 显式 --remote 时打开开关
            if cli.remote {
                remote.enabled = true;
            }

            let client = match imgforge::remote::try_build_http_client(&remote) {
                Ok(c) => c,
                Err(_) => imgforge::remote::build_client(&remote),
            };
            let sync = imgforge::remote::TaskSyncService::new(remote.clone(), client);

            match command {
                RemoteCommands::Status => {
                    println!("imgforge remote status");
                    println!("───────────────────────────────────────");
                    println!("enabled:       {}", remote.enabled);
                    println!("configured:    {}", remote.is_configured());
                    println!("status:        {}", remote.status_label());
                    println!(
                        "base_url:      {}",
                        remote.base_url.as_deref().unwrap_or("(none)")
                    );
                    println!(
                        "workspace:     {}",
                        remote.workspace_id.as_deref().unwrap_or("(none)")
                    );
                    println!("auth_mode:     {}", remote.auth_mode.label());
                    println!("timeout_secs:  {}", remote.timeout_secs);
                    println!("offline_cache: {}", remote.offline_cache);
                    println!("cache_path:    {}", remote.resolved_cache_path().display());
                    println!(
                        "token:         {}",
                        if remote.resolve_token().is_some() {
                            "present"
                        } else {
                            "absent"
                        }
                    );
                    match sync.sync_jobs(1) {
                        Ok(snap) => {
                            println!(
                                "health:        {} ({})",
                                if snap.online {
                                    "online"
                                } else {
                                    "offline/cache"
                                },
                                snap.health_message
                            );
                        }
                        Err(e) => println!("health:        error ({e})"),
                    }
                    println!("───────────────────────────────────────");
                    println!(
                        "API: GET /v1/health, POST/GET /v1/jobs, GET /v1/jobs/{{id}}[/result]"
                    );
                }
                RemoteCommands::Pull { limit } => {
                    let snap = sync.sync_jobs(*limit).wrap_err("remote pull failed")?;
                    println!(
                        "remote jobs ({}): online={} from_cache={}",
                        snap.jobs.len(),
                        snap.online,
                        snap.from_cache
                    );
                    println!("message: {}", snap.health_message);
                    for job in &snap.jobs {
                        println!(
                            "- {} [{}] {} {}/{} updated={}",
                            job.job_id,
                            job.source.label(),
                            job.phase.label(),
                            job.processed,
                            job.total,
                            job.updated_at
                        );
                    }
                    if snap.jobs.is_empty() {
                        println!("(empty)");
                    }
                }
                RemoteCommands::Submit { .. } => unreachable!(),
            }
        }
        RemoteCommands::Submit { source, paths } => {
            if *source != RemoteSubmitSource::Convert {
                return run_remote_module_submit(cli, *source, paths);
            }
            let mut config = load_config(cli).wrap_err("failed to load configuration")?;
            config.remote.enabled = true;
            config.remote.apply_env_overrides();
            config.remote.validate()?;
            if !config.remote.is_configured() {
                eyre::bail!(
                    "remote submit 需要 remote.base_url（配置文件或 IMGFORGE_REMOTE_BASE_URL）"
                );
            }
            let client = imgforge::remote::try_build_http_client(&config.remote)
                .wrap_err("failed to build remote HTTP client")?;
            let sync = imgforge::remote::TaskSyncService::new(config.remote.clone(), client);
            let outcome = sync
                .run_convert_and_download(&config, None)
                .wrap_err("remote convert failed")?;
            println!(
                "remote job {} phase={} successes={} failures={} downloaded={}",
                outcome.status.job_id,
                outcome.status.phase.label(),
                outcome.result.successes,
                outcome.result.failures,
                outcome.downloaded.len()
            );
            for path in &outcome.downloaded {
                println!("  {}", path.display());
            }
            if let Some(err) = &outcome.result.error_summary {
                println!("error: {err}");
            }
        }
    }
    Ok(())
}

fn run_remote_module_submit(
    cli: &Cli,
    source: RemoteSubmitSource,
    paths: &[PathBuf],
) -> eyre::Result<()> {
    let mut remote = load_remote_config(cli)?;
    remote.enabled = true;
    remote.apply_env_overrides();
    remote.validate()?;
    if !remote.is_configured() {
        eyre::bail!("remote submit 需要 remote.base_url（配置文件或 IMGFORGE_REMOTE_BASE_URL）");
    }

    let requested = if paths.is_empty() {
        vec![cli.input.clone().ok_or_else(|| {
            eyre::eyre!("remote submit --source {source:?} 需要 PATH 或 -i/--input")
        })?]
    } else {
        paths.to_vec()
    };
    let inputs = collect_remote_submit_paths(source, &requested);
    if inputs.is_empty() {
        eyre::bail!("未找到可提交的输入文件");
    }

    let assets = imgforge::remote::upload_paths_as_assets(&remote, &inputs)
        .wrap_err("remote asset upload failed")?;
    let job_source = match source {
        RemoteSubmitSource::Convert => unreachable!(),
        RemoteSubmitSource::Review => imgforge::remote::RemoteJobSource::Review,
        RemoteSubmitSource::Video => imgforge::remote::RemoteJobSource::VideoReview,
        RemoteSubmitSource::Extract => imgforge::remote::RemoteJobSource::DataExtract,
    };
    let batch_name = requested
        .first()
        .and_then(|path| path.file_name())
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_else(|| job_source.label().to_string());
    let extras = vec![
        ("batch_name".into(), batch_name),
        ("module".into(), "cli".into()),
    ];
    let (status, result) = imgforge::remote::submit_module_job(&remote, job_source, assets, extras)
        .wrap_err("remote module submit failed")?;
    println!(
        "remote job {} source={} phase={} inputs={} successes={} failures={}",
        status.job_id,
        status.source.label(),
        status.phase.label(),
        inputs.len(),
        result.successes,
        result.failures
    );
    for artifact in &result.artifacts {
        println!("artifact {} {}", artifact.id, artifact.name);
    }
    if let Some(err) = &result.error_summary {
        println!("error: {err}");
    }
    Ok(())
}

fn load_remote_config(cli: &Cli) -> eyre::Result<imgforge::remote::RemoteConfig> {
    let mut remote = imgforge::remote::RemoteConfig::default();
    remote.apply_env_overrides();
    let config_path = cli.config.clone().or_else(|| {
        PathBuf::from("imgforge.toml")
            .exists()
            .then(|| PathBuf::from("imgforge.toml"))
    });
    if let Some(path) = config_path {
        let raw = std::fs::read_to_string(&path)
            .wrap_err_with(|| format!("failed to read {}", path.display()))?;
        if let Ok(cfg) = toml::from_str::<imgforge::config::AppConfig>(&raw) {
            remote = cfg.remote;
            remote.apply_env_overrides();
        }
    }
    if cli.remote {
        remote.enabled = true;
    }
    Ok(remote)
}

fn collect_remote_submit_paths(source: RemoteSubmitSource, roots: &[PathBuf]) -> Vec<PathBuf> {
    let mut out = std::collections::BTreeSet::new();
    for root in roots {
        if root.is_file() {
            if path_matches_remote_source(source, root) {
                out.insert(root.clone());
            }
            continue;
        }
        if root.is_dir() {
            for entry in jwalk::WalkDir::new(root).into_iter().filter_map(|e| e.ok()) {
                if !entry.file_type().is_file() {
                    continue;
                }
                let path = entry.path();
                if path_matches_remote_source(source, &path) {
                    out.insert(path);
                }
            }
        }
    }
    out.into_iter().collect()
}

fn path_matches_remote_source(source: RemoteSubmitSource, path: &Path) -> bool {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        .unwrap_or_default();
    match source {
        RemoteSubmitSource::Convert => false,
        RemoteSubmitSource::Review => matches!(
            ext.as_str(),
            "jpg" | "jpeg" | "png" | "webp" | "bmp" | "tiff" | "tif" | "gif"
        ),
        RemoteSubmitSource::Video => matches!(
            ext.as_str(),
            "mp4" | "mov" | "m4v" | "avi" | "mkv" | "webm" | "hevc" | "h265"
        ),
        RemoteSubmitSource::Extract => {
            matches!(
                ext.as_str(),
                "csv"
                    | "tsv"
                    | "json"
                    | "txt"
                    | "ini"
                    | "log"
                    | "html"
                    | "htm"
                    | "png"
                    | "jpg"
                    | "jpeg"
                    | "webp"
                    | "tif"
                    | "tiff"
            ) || path
                .file_name()
                .and_then(|name| name.to_str())
                .map(|name| {
                    let name = name.to_ascii_lowercase();
                    name.contains("imatest")
                        || name.contains("results")
                        || name.contains("summary")
                        || name.contains("report")
                })
                .unwrap_or(false)
        }
    }
}

#[tokio::main]
async fn main() -> eyre::Result<()> {
    color_eyre::install().ok();

    let cli = Cli::parse();

    if let Some(Commands::Completions { shell }) = cli.command {
        Cli::generate_completions(shell.into());
        return Ok(());
    }

    if matches!(cli.command, Some(Commands::Doctor)) {
        ui::doctor::run_doctor();
        return Ok(());
    }

    if let Some(Commands::Remote { command }) = &cli.command {
        return run_remote_command(&cli, command);
    }

    // 未指定输入时给出用法提示（避免 IDE 直接 Run 扫当前目录报错）
    if cli.command.is_none()
        && cli.input.is_none()
        && cli.config.is_none()
        && !cli.mobile_pull
        && std::env::var("IMGFORGE_INPUT").is_err()
        && std::env::var("IMGFORGE_INPUT_DIR").is_err()
    {
        use clap::CommandFactory;
        Cli::command().print_help()?;
        eprintln!();
        eprintln!("示例:");
        eprintln!("  imgforge -i ./photos -o ./output -f webp");
        eprintln!("图形界面: cargo run --features gui --bin imgforge-app");
        return Ok(());
    }

    ui::init_logger(cli.verbose);

    let config = load_config(&cli).wrap_err("failed to load configuration")?;

    // `--remote`：上传、远端转换并下载结果
    if cli.remote {
        if !config.remote.is_configured() {
            eyre::bail!(
                "--remote 需要 remote.base_url（配置 [remote] 或 IMGFORGE_REMOTE_BASE_URL）"
            );
        }
        let client = imgforge::remote::try_build_http_client(&config.remote)
            .wrap_err("failed to build remote HTTP client")?;
        let sync = imgforge::remote::TaskSyncService::new(config.remote.clone(), client);
        let cancelled = Arc::new(AtomicBool::new(false));
        install_shutdown_handler(Arc::clone(&cancelled));
        let outcome = sync
            .run_convert_and_download(&config, Some(&cancelled))
            .wrap_err("remote convert failed")?;
        println!(
            "remote job {} ({}) successes={} failures={} downloaded={}",
            outcome.status.job_id,
            outcome.status.phase.label(),
            outcome.result.successes,
            outcome.result.failures,
            outcome.downloaded.len()
        );
        for path in &outcome.downloaded {
            println!("  {}", path.display());
        }
        return Ok(());
    }

    tracing::info!(
      input = %config.input_dir.display(),
      output = %config.output_dir.display(),
      format = %config.target_format,
      concurrency = config.concurrency.value(),
      "starting imgforge"
    );

    let cancelled = Arc::new(AtomicBool::new(false));
    install_shutdown_handler(Arc::clone(&cancelled));

    if config.dry_run {
        tracing::info!("dry-run mode: no files will be written");
        match imgforge::job::preview_batch(&config) {
            Ok(preview) => {
                ui::report::ProcessReport::print_preview_summary(
                    &preview,
                    config.target_format.extension(),
                );
            }
            Err(e) => tracing::warn!(error = %e, "preview scan failed"),
        }
    }

    let report = run_batch(config, cancelled, None)
        .await
        .wrap_err("execution failed")?;

    report.print_summary();

    if !report.failures.is_empty() {
        std::process::exit(1);
    }

    Ok(())
}

/// 跨平台优雅退出：Unix Ctrl+C / Windows Ctrl+C 与 Ctrl+Break。
fn install_shutdown_handler(cancelled: Arc<AtomicBool>) {
    #[cfg(not(windows))]
    tokio::spawn(async move {
        if tokio::signal::ctrl_c().await.is_ok() {
            tracing::warn!("received Ctrl+C, shutting down gracefully...");
            cancelled.store(true, Ordering::Relaxed);
        }
    });

    #[cfg(windows)]
    tokio::spawn(async move {
        let mut ctrl_break = match tokio::signal::windows::ctrl_break() {
            Ok(signal) => signal,
            Err(e) => {
                tracing::warn!(error = %e, "failed to install Ctrl+Break handler");
                return;
            }
        };
        let mut ctrl_close = match tokio::signal::windows::ctrl_close() {
            Ok(signal) => signal,
            Err(e) => {
                tracing::warn!(error = %e, "failed to install console close handler");
                return;
            }
        };

        tokio::select! {
          _ = tokio::signal::ctrl_c() => {
            tracing::warn!("received Ctrl+C, shutting down gracefully...");
          }
          _ = ctrl_break.recv() => {
            tracing::warn!("received Ctrl+Break, shutting down gracefully...");
          }
          _ = ctrl_close.recv() => {
            tracing::warn!("received console close, shutting down gracefully...");
          }
        }
        cancelled.store(true, Ordering::Relaxed);
    });
}
