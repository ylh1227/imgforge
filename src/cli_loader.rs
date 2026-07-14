//! CLI 配置加载（仅命令行二进制使用）。

use std::path::Path;

use figment::{
    providers::{Env, Format, Serialized, Toml},
    Figment,
};

use crate::cli::Cli;
use imgforge::config::app_config::AppConfig;
use imgforge::config::presets::Preset;
use imgforge::core::error::{AppError, AppResult};
use imgforge::core::types::Concurrency;

/// 从 CLI、环境变量与 TOML 文件合并加载配置。
pub fn load_config(cli: &Cli) -> AppResult<AppConfig> {
    let mut figment = Figment::from(Serialized::defaults(AppConfig::default()));

    if let Some(ref path) = cli.config {
        if path.exists() {
            figment = figment.merge(Toml::file(path));
        }
    }

    figment =
        figment.merge(Env::prefixed("IMGFORGE_").map(|k| k.as_str().to_ascii_lowercase().into()));

    let mut config: AppConfig = figment
        .extract()
        .map_err(|e| AppError::Config(e.to_string()))?;

    apply_cli_overrides(&mut config, cli);

    if let Some(ref preset_name) = cli.preset {
        let preset = Preset::parse(preset_name)
            .ok_or_else(|| AppError::Config(format!("unknown preset: {preset_name}")))?;
        preset.apply(&mut config);
    }

    config.validate()?;
    Ok(config)
}

fn apply_cli_overrides(config: &mut AppConfig, cli: &Cli) {
    if let Some(ref input) = cli.input {
        config.input_dir = input.clone();
    }
    if let Some(ref output) = cli.output {
        config.output_dir = output.clone();
    }
    if let Some(format) = cli.format {
        config.target_format = format;
    }
    if let Some(quality) = cli.quality {
        if let Ok(q) = imgforge::core::types::Quality::new(quality) {
            config.quality = q;
        }
    }
    if let Some(concurrency) = cli.concurrency {
        if let Ok(c) = Concurrency::new(concurrency) {
            config.concurrency = c;
        }
    }
    if cli.recursive {
        config.recursive = true;
    }
    if cli.no_recursive {
        config.recursive = false;
    }
    if cli.overwrite {
        config.overwrite = true;
    }
    if cli.preserve_structure {
        config.preserve_structure = true;
    }
    if cli.flat {
        config.preserve_structure = false;
    }
    if cli.dry_run {
        config.dry_run = true;
    }
    if let Some(w) = cli.width {
        config.resize.width = Some(w);
    }
    if let Some(h) = cli.height {
        config.resize.height = Some(h);
    }
    if let Some(b) = cli.brightness {
        config.adjust.brightness = b;
    }
    if let Some(c) = cli.contrast {
        config.adjust.contrast = c;
    }
    if let Some(s) = cli.sharpen {
        config.adjust.sharpen = s;
    }
    if cli.strip_metadata {
        config.metadata_policy = imgforge::core::types::MetadataPolicy::Strip;
    }
    if cli.keep_metadata {
        config.metadata_policy = imgforge::core::types::MetadataPolicy::Preserve;
    }
    if let Some(ref exts) = cli.extensions {
        config.extensions = exts.clone();
    }
    if let Some(min) = cli.min_size {
        config.min_size = Some(min);
    }
    if let Some(max) = cli.max_size {
        config.max_size = Some(max);
    }
    if cli.incremental {
        config.incremental = true;
    }
    if let Some(ref template) = cli.rename_template {
        config.rename_template = Some(template.clone());
    }

    if cli.watermark_image.is_some() || cli.watermark_text.is_some() || cli.watermark_font.is_some()
    {
        config.watermark.image_path = cli.watermark_image.clone();
        config.watermark.text = cli.watermark_text.clone();
        config.watermark.font_path = cli.watermark_font.clone();
        config.watermark.opacity = cli.watermark_opacity;
        config.watermark.font_size = cli.watermark_size;
    }

    if let Some(ref sizes) = cli.thumbnail_sizes {
        config.thumbnails.clear();
        for spec in sizes {
            match imgforge::core::types::parse_thumbnail_spec(spec) {
                Ok(t) => config.thumbnails.push(t),
                Err(e) => {
                    tracing::warn!(spec = %spec, error = %e, "skipping invalid thumbnail spec")
                }
            }
        }
    }
    if let Some(transform) = cli.transform {
        config.transform = transform;
    }
    if let Some(mode) = cli.resize_mode {
        config.resize.mode = mode;
    }
    if let Some(pos) = cli.watermark_position {
        config.watermark.position = pos;
    }
    if cli.bayer_only {
        config.bayer_only = true;
    }
    if cli.remote {
        config.remote.enabled = true;
    }
    config.remote.apply_env_overrides();

    if cli.mobile_pull {
        config.mobile_pull.enabled = true;
    }
    if let Some(backend) = cli.mobile_backend {
        config.mobile_pull.backend = backend;
        config.mobile_pull.enabled = true;
    }
    if let Some(source) = &cli.mobile_source {
        config.mobile_pull.source_path = source.clone();
        config.mobile_pull.enabled = true;
    }
    if let Some(staging) = &cli.mobile_staging {
        config.mobile_pull.staging_dir = staging.clone();
        config.mobile_pull.enabled = true;
    }
    if let Some(serial) = &cli.adb_serial {
        config.mobile_pull.adb_serial = Some(serial.clone());
        config.mobile_pull.enabled = true;
    }
    if let Some(mode) = cli.adb_mode {
        config.mobile_pull.adb_mode = mode;
        config.mobile_pull.enabled = true;
    }
    if let Some(path) = &cli.adb_path {
        config.mobile_pull.adb_path = Some(path.clone());
        config.mobile_pull.enabled = true;
    }

    config.verbose = cli.verbose;
}

/// 解析配置文件路径（若存在）。
pub fn config_file_path(cli: &Cli) -> Option<&Path> {
    cli.config.as_deref()
}
