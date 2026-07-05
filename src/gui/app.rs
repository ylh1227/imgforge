//! ImgForge 图形界面：文件夹选择、格式设置、进度与结果展示。

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::sync::Arc;
use std::thread;

use eframe::egui::{self, RichText, ScrollArea};

use crate::config::AppConfig;
use crate::core::types::{ImageFormat, MetadataPolicy, Quality, ResizeOptions};
use crate::gui::prefs::{self, ConvertPresetSnapshot, GuiPrefs, TaskHistoryEntry};
use crate::gui::quality_preview::{self, QualityPreviewWorker, QualitySizeRow};
use crate::gui::{fonts, native, theme, widgets};
use crate::io::batch_preview::BatchPreview;
use crate::job::{preview_batch, run_batch};
use crate::ui::progress::{GuiProgress, ProgressReporter};
use crate::ui::report::ProcessReport;
use crate::review::ui::ReviewPanelHost;

/// 主应用 → 评审面板的转换队列上下文。
struct AppReviewHost<'a> {
  queue: &'a [PathBuf],
  output_dir: &'a str,
}

impl ReviewPanelHost for AppReviewHost<'_> {
  fn conversion_queue_paths(&self) -> &[PathBuf] {
    self.queue
  }

  fn output_directory(&self) -> &str {
    self.output_dir
  }
}

enum RunState {
  Idle,
  Running {
    cancelled: Arc<AtomicBool>,
    progress: Arc<dyn ProgressReporter>,
  },
  Done(ProcessReport),
  Failed,
}

enum WorkerMessage {
  Finished(Result<ProcessReport, String>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AppMode {
  Convert,
  Review,
}

/// 主窗口应用。
pub struct ImgforgeApp {
  mode: AppMode,
  review_panel: Option<crate::review::ui::ReviewPanel>,
  review_queue: Vec<PathBuf>,
  review_queue_params: std::collections::HashMap<PathBuf, crate::config::ConvertOverride>,
  burn_review_annotations: bool,
  input_dir: String,
  output_dir: String,
  format_index: usize,
  formats: Vec<ImageFormat>,
  quality: u8,
  recursive: bool,
  preserve_structure: bool,
  overwrite: bool,
  strip_metadata: bool,
  bayer_only: bool,
  rename_template: String,
  use_target_max_bytes: bool,
  target_max_kb: u32,
  gui_prefs: GuiPrefs,
  selected_preset: Option<usize>,
  new_preset_name: String,
  batch_preview: Option<BatchPreview>,
  rename_preview: Vec<(String, String)>,
  rename_preview_error: Option<String>,
  quality_preview_rows: Vec<QualitySizeRow>,
  quality_preview_error: Option<String>,
  quality_preview_worker: Option<QualityPreviewWorker>,
  status: String,
  log_lines: Vec<String>,
  state: RunState,
  worker_rx: Option<Receiver<WorkerMessage>>,
  native_toolbar: Option<native::NativeGlassToolbar>,
}

impl ImgforgeApp {
  pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
    fonts::install_cjk_fonts(&cc.egui_ctx);
    theme::apply(&cc.egui_ctx);

    let formats = ImageFormat::all_supported();
    let review_panel = crate::review::ui::ReviewPanel::new().ok();
    let gui_prefs = GuiPrefs::load();
    Self {
      mode: AppMode::Convert,
      review_panel,
      review_queue: Vec::new(),
      review_queue_params: std::collections::HashMap::new(),
      burn_review_annotations: false,
      input_dir: String::new(),
      output_dir: String::from("./output"),
      format_index: formats
        .iter()
        .position(|f| *f == ImageFormat::WebP)
        .unwrap_or(0),
      formats,
      quality: Quality::DEFAULT.value(),
      recursive: true,
      preserve_structure: true,
      overwrite: false,
      strip_metadata: false,
      bayer_only: false,
      rename_template: String::new(),
      use_target_max_bytes: false,
      target_max_kb: 500,
      gui_prefs,
      selected_preset: None,
      new_preset_name: String::new(),
      batch_preview: None,
      rename_preview: Vec::new(),
      rename_preview_error: None,
      quality_preview_rows: Vec::new(),
      quality_preview_error: None,
      quality_preview_worker: None,
      status: String::from("选择输入文件夹，然后点击「开始转换」"),
      log_lines: Vec::new(),
      state: RunState::Idle,
      worker_rx: None,
      native_toolbar: None,
    }
  }

  fn is_running(&self) -> bool {
    matches!(self.state, RunState::Running { .. })
  }

  fn push_log(&mut self, line: impl Into<String>) {
    self.log_lines.push(line.into());
    if self.log_lines.len() > 200 {
      let drain = self.log_lines.len() - 200;
      self.log_lines.drain(0..drain);
    }
  }

  fn build_config(&self) -> Result<AppConfig, String> {
    let input = PathBuf::from(&self.input_dir);
    if self.input_dir.trim().is_empty() {
      return Err("请选择输入文件夹".into());
    }
    if !input.exists() {
      return Err(format!("输入文件夹不存在：{}", input.display()));
    }

    let output = PathBuf::from(&self.output_dir);
    if self.output_dir.trim().is_empty() {
      return Err("请指定输出文件夹".into());
    }

    let target_format = self.formats[self.format_index];
    let quality = if self.use_target_max_bytes {
      Quality::DEFAULT
    } else {
      Quality::new(self.quality).map_err(|e| e.to_string())?
    };

    let mut config = AppConfig::default();
    config.input_dir = input;
    config.output_dir = output;
    config.target_format = target_format;
    config.quality = quality;
    config.recursive = self.recursive;
    config.preserve_structure = self.preserve_structure;
    config.overwrite = self.overwrite;
    config.metadata_policy = if self.strip_metadata {
      MetadataPolicy::Strip
    } else {
      MetadataPolicy::Preserve
    };
    if !self.rename_template.trim().is_empty() {
      config.rename_template = Some(self.rename_template.trim().to_string());
    }
    if self.use_target_max_bytes {
      config.target_max_bytes = Some(self.target_max_kb as u64 * 1024);
    }
    if !self.review_queue.is_empty() {
      config.explicit_inputs = self.review_queue.clone();
      if let Some(parent) = self.review_queue[0].parent() {
        config.input_dir = parent.to_path_buf();
      }
      config.per_input_params = self.review_queue_params.clone();
    }
    config.burn_review_annotations = self.burn_review_annotations;
    config.bayer_only = self.bayer_only;
    config.validate().map_err(|e| e.to_string())?;
    Ok(config)
  }

  fn snapshot_from_ui(&self) -> ConvertPresetSnapshot {
    ConvertPresetSnapshot {
      format: self.formats[self.format_index],
      quality: self.quality,
      resize: ResizeOptions {
        width: None,
        height: None,
        mode: crate::core::types::ResizeMode::Fit,
      },
      recursive: self.recursive,
      preserve_structure: self.preserve_structure,
      overwrite: self.overwrite,
      strip_metadata: self.strip_metadata,
      bayer_only: self.bayer_only,
      rename_template: self.rename_template.clone(),
      target_max_bytes: if self.use_target_max_bytes {
        Some(self.target_max_kb as u64 * 1024)
      } else {
        None
      },
      use_target_max_bytes: self.use_target_max_bytes,
    }
  }

  fn apply_snapshot(&mut self, snapshot: &ConvertPresetSnapshot) {
    if let Some(idx) = self
      .formats
      .iter()
      .position(|f| *f == snapshot.format)
    {
      self.format_index = idx;
    }
    self.quality = snapshot.quality;
    self.recursive = snapshot.recursive;
    self.preserve_structure = snapshot.preserve_structure;
    self.overwrite = snapshot.overwrite;
    self.strip_metadata = snapshot.strip_metadata;
    self.bayer_only = snapshot.bayer_only;
    self.rename_template = snapshot.rename_template.clone();
    self.use_target_max_bytes = snapshot.use_target_max_bytes;
    if let Some(bytes) = snapshot.target_max_bytes {
      self.target_max_kb = (bytes / 1024).max(1) as u32;
    }
    self.refresh_previews();
  }

  fn refresh_previews(&mut self) {
    self.batch_preview = None;
    self.rename_preview.clear();
    self.rename_preview_error = None;

    if let Ok(config) = self.build_config() {
      if let Ok(preview) = preview_batch(&config) {
        self.batch_preview = Some(preview);
      }
    }

    if !self.rename_template.trim().is_empty() && !self.input_dir.trim().is_empty() {
      let input = PathBuf::from(&self.input_dir);
      let output = PathBuf::from(&self.output_dir);
      if input.exists() {
        match crate::io::batch_preview::rename_preview_samples(
          &input,
          &output,
          self.rename_template.trim(),
          self.formats[self.format_index],
          self.preserve_structure,
          self.recursive,
          5,
        ) {
          Ok(samples) => {
            self.rename_preview = samples
              .into_iter()
              .map(|(path, name)| {
                let stem = path
                  .file_name()
                  .and_then(|n| n.to_str())
                  .unwrap_or("?")
                  .to_string();
                let out = name.unwrap_or_else(|e| format!("错误: {e}"));
                (stem, out)
              })
              .collect();
          }
          Err(e) => self.rename_preview_error = Some(e.to_string()),
        }
      }
    }
  }

  fn request_quality_preview(&mut self) {
    self.quality_preview_rows.clear();
    self.quality_preview_error = None;
    self.quality_preview_worker = None;

    if self.input_dir.trim().is_empty() {
      return;
    }
    let input = PathBuf::from(&self.input_dir);
    if !input.is_dir() {
      return;
    }

    let sample = std::fs::read_dir(&input)
      .ok()
      .into_iter()
      .flatten()
      .filter_map(|e| e.ok())
      .map(|e| e.path())
      .find(|p| {
        p.is_file()
          && p.extension()
            .and_then(|e| e.to_str())
            .and_then(ImageFormat::from_extension)
            .is_some()
      });

    let Some(sample) = sample else {
      self.quality_preview_error = Some("输入文件夹中未找到可预览的图片".into());
      return;
    };

    self.quality_preview_worker = Some(QualityPreviewWorker::spawn(
      sample,
      self.formats[self.format_index],
    ));
  }

  fn poll_quality_preview(&mut self) {
    let Some(worker) = &self.quality_preview_worker else {
      return;
    };
    if let Some(msg) = worker.poll() {
      self.quality_preview_worker = None;
      match msg {
        quality_preview::QualityPreviewMsg::Done(rows) => {
          self.quality_preview_rows = rows;
        }
        quality_preview::QualityPreviewMsg::Failed(e) => {
          self.quality_preview_error = Some(e);
        }
      }
    }
  }

  fn record_history(&mut self, report: &ProcessReport) {
    let entry = TaskHistoryEntry {
      finished_at_unix: prefs::now_unix(),
      input_dir: self.input_dir.clone(),
      output_dir: self.output_dir.clone(),
      successes: report.successes,
      failures: report.failures.len(),
      total: report.total,
      elapsed_ms: report.elapsed.as_millis() as u64,
      snapshot: self.snapshot_from_ui(),
    };
    self.gui_prefs.push_history(entry);
    let _ = self.gui_prefs.save();
  }

  fn start_conversion(&mut self) {
    if self.is_running() {
      return;
    }

    self.refresh_previews();

    let config = match self.build_config() {
      Ok(c) => c,
      Err(e) => {
        self.status = e.clone();
        self.push_log(format!("错误：{e}"));
        return;
      }
    };

    if let Some(ref preview) = self.batch_preview {
      if preview.output_conflicts > 0 {
        self.status = format!(
          "存在 {} 处输出路径冲突，请调整重命名模板",
          preview.output_conflicts
        );
        self.push_log(self.status.clone());
        return;
      }
      if preview.to_convert == 0 {
        self.status = "没有需要转换的文件（可能均已存在且未勾选覆盖）".into();
        self.push_log(self.status.clone());
        return;
      }
    }

    let cancelled = Arc::new(AtomicBool::new(false));
    let progress: Arc<dyn ProgressReporter> = Arc::new(GuiProgress::new());
    let (tx, rx) = mpsc::channel();

    self.worker_rx = Some(rx);
    self.state = RunState::Running {
      cancelled: Arc::clone(&cancelled),
      progress: Arc::clone(&progress),
    };
    self.status = "正在扫描并转换图片…".to_string();
    self.push_log(format!(
      "开始：{} → {} ({})",
      config.input_dir.display(),
      config.output_dir.display(),
      config.target_format
    ));

    thread::spawn(move || {
      let rt = match tokio::runtime::Runtime::new() {
        Ok(rt) => rt,
        Err(e) => {
          let _ = tx.send(WorkerMessage::Finished(Err(e.to_string())));
          return;
        }
      };

      let progress_reporter = Arc::clone(&progress);
      let result = rt.block_on(run_batch(config, cancelled, Some(progress_reporter)));
      let msg = match result {
        Ok(report) => WorkerMessage::Finished(Ok(report)),
        Err(e) => WorkerMessage::Finished(Err(e.to_string())),
      };
      let _ = tx.send(msg);
    });
  }

  fn cancel_conversion(&mut self) {
    if let RunState::Running { cancelled, .. } = &self.state {
      cancelled.store(true, Ordering::Relaxed);
      self.status = "正在取消…".to_string();
      self.push_log("用户请求取消");
    }
  }

  fn poll_worker(&mut self) {
    let Some(rx) = self.worker_rx.as_ref() else {
      return;
    };

    let Ok(msg) = rx.try_recv() else {
      return;
    };

    self.worker_rx = None;
    match msg {
      WorkerMessage::Finished(Ok(report)) => {
        let summary = format!(
          "完成：成功 {} / {}，失败 {}，耗时 {}",
          report.successes,
          report.total,
          report.failures.len(),
          humantime::format_duration(report.elapsed)
        );
        if report.cancelled {
          self.status = format!("已取消（{summary}）");
        } else {
          self.status = summary.clone();
        }
        self.push_log(summary);
        for failure in &report.failures {
          self.push_log(format!("失败：{} — {}", failure.path.display(), failure.error));
        }
        self.record_history(&report);
        self.state = RunState::Done(report);
      }
      WorkerMessage::Finished(Err(e)) => {
        self.status = format!("转换失败：{e}");
        self.push_log(format!("错误：{e}"));
        self.state = RunState::Failed;
      }
    }
  }

  fn open_output_folder(&self) {
    if self.output_dir.trim().is_empty() {
      return;
    }
    let path = PathBuf::from(&self.output_dir);
    if path.exists() {
      let _ = open::that(&path);
    }
  }

  fn settings_checkboxes(&mut self, ui: &mut egui::Ui, enabled: bool) {
    let mut file_options = vec![
      (&mut self.recursive, "包含子文件夹"),
      (&mut self.preserve_structure, "保留目录结构"),
      (&mut self.overwrite, "覆盖已有文件"),
      (&mut self.strip_metadata, "移除 EXIF 元数据"),
    ];

    widgets::settings_subheading(ui, "文件选项");
    ui.add_space(4.0);
    widgets::checkbox_grid(ui, &mut file_options, enabled, 2);

    #[cfg(feature = "bayer")]
    {
      widgets::inset_separator(ui);
      widgets::settings_subheading(ui, "RAW 处理");
      ui.add_space(4.0);
      let mut raw_options = [(&mut self.bayer_only, "仅解 Bayer/RAW（不做缩放锐化）")];
      widgets::checkbox_grid(ui, &mut raw_options, enabled, 1);
    }
  }
}

impl eframe::App for ImgforgeApp {
  fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
    self.poll_worker();
    self.poll_quality_preview();

    let running = self.is_running();
    let enabled = !running;

    if let RunState::Running { progress, .. } = &self.state {
      if let Some(label) = progress.status_label() {
        self.status = label;
      }
      ctx.request_repaint_after(std::time::Duration::from_millis(100));
    }

    ctx.input(|input| {
      if enabled && !input.raw.dropped_files.is_empty() {
        for file in &input.raw.dropped_files {
          if let Some(path) = &file.path {
            if path.is_dir() {
              self.input_dir = path.display().to_string();
              self.status = format!("已拖入文件夹：{}", path.display());
              self.refresh_previews();
              break;
            }
          }
        }
      }
    });

    let dark = ctx.style().visuals.dark_mode;

    if self.native_toolbar.is_none() {
      self.native_toolbar = native::NativeGlassToolbar::try_install(frame);
    }

    let native_toolbar_active = self
      .native_toolbar
      .as_ref()
      .is_some_and(|toolbar| toolbar.is_active());

    if let Some(toolbar) = &mut self.native_toolbar {
      toolbar.sync(enabled, running);
      for action in toolbar.drain_actions() {
        match action {
          native::ToolbarAction::Start => self.start_conversion(),
          native::ToolbarAction::Cancel => self.cancel_conversion(),
          native::ToolbarAction::OpenOutput => self.open_output_folder(),
        }
      }
    }

    if self.mode == AppMode::Convert && !native_toolbar_active {
      egui::TopBottomPanel::bottom("action_toolbar")
        .frame(widgets::glass_toolbar_frame(dark))
        .show(ctx, |ui| {
          if let Some(click) = widgets::action_toolbar_row(ui, enabled, running) {
            match click {
              widgets::ToolbarClick::Start => self.start_conversion(),
              widgets::ToolbarClick::Cancel => self.cancel_conversion(),
              widgets::ToolbarClick::OpenOutput => self.open_output_folder(),
            }
          }
        });
    }

    let viewport = theme::viewport_size(ctx);
    let content_w = theme::content_width(viewport.x);
    let bottom_reserve = if native_toolbar_active {
      native::TOOLBAR_HEIGHT + 14.0
    } else {
      88.0
    };
    let log_height = theme::log_panel_height(viewport.y, bottom_reserve);

    egui::CentralPanel::default()
      .frame(egui::Frame::NONE.fill(theme::window_fill(dark)))
      .show(ctx, |ui| {
        ui.add_space(theme::macos_titlebar_inset(ctx));

        ui.horizontal(|ui| {
          if self.review_panel.is_some() {
            widgets::mode_tab_bar(
              ui,
              &mut self.mode,
              &[
                (AppMode::Convert, "格式转换"),
                (AppMode::Review, "图片评审"),
              ],
            );
          }
        });
        ui.add_space(8.0);

        if self.mode == AppMode::Review {
          if let Some(panel) = &mut self.review_panel {
            let host = AppReviewHost {
              queue: &self.review_queue,
              output_dir: &self.output_dir,
            };
            panel.ui(ctx, ui, &host);
            let output = panel.take_output();
            if !output.enqueue_approved.is_empty() {
              self.review_queue = output.enqueue_approved;
              self.review_queue_params = output
                .enqueue_params
                .iter()
                .filter(|i| !i.params.is_empty())
                .map(|i| {
                  (
                    i.path.clone(),
                    crate::config::ConvertOverride {
                      format: i.params.format,
                      quality: i.params.quality.and_then(|q| crate::core::types::Quality::new(q).ok()),
                      width: i.params.width,
                    },
                  )
                })
                .collect();
              self.mode = AppMode::Convert;
              self.status = format!(
                "已从评审导入 {} 张「通过」图片，可开始转换",
                self.review_queue.len()
              );
              self.push_log(self.status.clone());
            } else if output.switch_to_convert {
              self.mode = AppMode::Convert;
              self.status = output.status_message.clone();
            } else if !output.status_message.is_empty() {
              self.status = output.status_message;
            }
          } else {
            ui.label("评审模块初始化失败");
          }
          return;
        }

        ScrollArea::vertical()
          .auto_shrink([false, false])
          .show(ui, |ui| {
            ui.vertical_centered(|ui| {
              ui.set_width(content_w);

              widgets::navigation_header(ui, "批量图片格式转换");
              ui.add_space(20.0);

              if !self.review_queue.is_empty() {
                widgets::grouped_section(ui, "评审队列", |ui| {
                  ui.label(format!(
                    "已从评审导入 {} 张「通过」图片，将仅转换这些文件",
                    self.review_queue.len()
                  ));
                  ui.horizontal_wrapped(|ui| {
                    for path in self.review_queue.iter().take(8) {
                      let label = if let Some(panel) = &self.review_panel {
                        panel
                          .status_for_path(path)
                          .map(|s| format!("[{}] {}", s.label(), path.file_name().and_then(|n| n.to_str()).unwrap_or("")))
                          .unwrap_or_else(|| path.file_name().and_then(|n| n.to_str()).unwrap_or("").to_string())
                      } else {
                        path.file_name().and_then(|n| n.to_str()).unwrap_or("").to_string()
                      };
                      ui.label(RichText::new(label).size(12.0).color(theme::secondary_label(dark)));
                    }
                    if self.review_queue.len() > 8 {
                      ui.label(format!("…等 {} 张", self.review_queue.len()));
                    }
                  });
                  ui.horizontal(|ui| {
                    ui.add_enabled(enabled, egui::Checkbox::new(&mut self.burn_review_annotations, "导出时叠加标注"));
                    if widgets::secondary_button(ui, "清空评审队列", enabled).clicked() {
                      self.review_queue.clear();
                      self.review_queue_params.clear();
                      self.status = "已清空评审导入队列".into();
                    }
                    if widgets::compact_primary_button(ui, "发送到评审", enabled && !self.review_queue.is_empty())
                      .clicked()
                    {
                      if let Some(panel) = &mut self.review_panel {
                        panel.schedule_import_from_queue(
                          self.review_queue.clone(),
                          "转换队列",
                        );
                        self.mode = AppMode::Review;
                        self.status = format!(
                          "已将 {} 张图片发送到评审模块",
                          self.review_queue.len()
                        );
                      }
                    }
                  });
                });
                ui.add_space(16.0);
              }

              widgets::grouped_section(ui, "文件夹", |ui| {
                let prev_input = self.input_dir.clone();
                widgets::folder_field(ui, "输入", &mut self.input_dir, enabled);
                widgets::folder_field(ui, "输出", &mut self.output_dir, enabled);
                if prev_input != self.input_dir {
                  self.refresh_previews();
                }
                if self.input_dir.trim().is_empty() {
                  widgets::drop_hint(ui);
                }
              });

              ui.add_space(16.0);

              widgets::grouped_section(ui, "预设与历史", |ui| {
                ui.horizontal_wrapped(|ui| {
                  if !self.gui_prefs.presets.is_empty() {
                    let labels: Vec<String> = self
                      .gui_prefs
                      .presets
                      .iter()
                      .map(|p| p.name.clone())
                      .collect();
                    let mut selected = self.selected_preset.unwrap_or(0).min(labels.len().saturating_sub(1));
                    egui::ComboBox::from_id_salt("user_preset")
                      .selected_text(labels.get(selected).cloned().unwrap_or_else(|| "选择预设".into()))
                      .show_ui(ui, |ui| {
                        for (i, name) in labels.iter().enumerate() {
                          ui.selectable_value(&mut selected, i, name);
                        }
                      });
                    self.selected_preset = Some(selected);
                    if widgets::compact_secondary_button(ui, "套用", enabled).clicked() {
                      if let Some(p) = self.gui_prefs.presets.get(selected).cloned() {
                        let name = p.name.clone();
                        self.apply_snapshot(&p.snapshot);
                        self.status = format!("已套用预设「{name}」");
                      }
                    }
                    if widgets::compact_secondary_button(ui, "删除", enabled).clicked() {
                      if let Some(name) = labels.get(selected).cloned() {
                        self.gui_prefs.delete_preset(&name);
                        let _ = self.gui_prefs.save();
                        self.selected_preset = None;
                      }
                    }
                  } else {
                    ui.label(
                      RichText::new("暂无自定义预设")
                        .size(12.0)
                        .color(theme::secondary_label(dark)),
                    );
                  }
                });
                ui.horizontal(|ui| {
                  ui.add_enabled_ui(enabled, |ui| {
                    ui.add(
                      egui::TextEdit::singleline(&mut self.new_preset_name)
                        .desired_width(120.0)
                        .hint_text("预设名称"),
                    );
                  });
                  if widgets::compact_primary_button(ui, "保存当前为预设", enabled).clicked() {
                    let name = self.new_preset_name.trim().to_string();
                    if name.is_empty() {
                      self.status = "请输入预设名称".into();
                    } else {
                      self
                        .gui_prefs
                        .upsert_preset(name.clone(), self.snapshot_from_ui());
                      let _ = self.gui_prefs.save();
                      self.status = format!("已保存预设「{name}」");
                      self.new_preset_name.clear();
                    }
                  }
                });

                if !self.gui_prefs.history.is_empty() {
                  widgets::inset_separator(ui);
                  widgets::settings_subheading(ui, "最近任务");
                  ui.add_space(4.0);
                  let recent: Vec<_> = self.gui_prefs.history.iter().take(5).cloned().collect();
                  for (i, entry) in recent.into_iter().enumerate() {
                    ui.horizontal_wrapped(|ui| {
                      ui.label(
                        RichText::new(format!(
                          "{} → {} · {}/{} 成功",
                          entry.input_dir, entry.output_dir, entry.successes, entry.total
                        ))
                        .size(12.0)
                        .color(theme::secondary_label(dark)),
                      );
                      if widgets::compact_secondary_button(ui, "重跑", enabled).clicked() {
                        self.input_dir = entry.input_dir.clone();
                        self.output_dir = entry.output_dir.clone();
                        self.apply_snapshot(&entry.snapshot);
                        self.status = format!("已载入历史任务 #{i}");
                      }
                      if widgets::compact_secondary_button(ui, "打开输出", true).clicked() {
                        let path = PathBuf::from(&entry.output_dir);
                        if path.exists() {
                          let _ = open::that(&path);
                        }
                      }
                    });
                  }
                }
              });

              ui.add_space(16.0);

              widgets::grouped_section(ui, "转换设置", |ui| {
                widgets::settings_labeled_row(ui, "目标格式", |ui| {
                  let combo_w = f32::min(140.0, ui.available_width());
                  egui::ComboBox::from_id_salt("format")
                    .width(combo_w)
                    .selected_text(self.formats[self.format_index].extension().to_uppercase())
                    .show_ui(ui, |ui| {
                      for (idx, format) in self.formats.iter().enumerate() {
                        ui.selectable_value(
                          &mut self.format_index,
                          idx,
                          format.extension().to_uppercase(),
                        );
                      }
                    });
                });

                ui.add_space(6.0);
                widgets::quality_slider_row(ui, &mut self.quality, enabled && !self.use_target_max_bytes);

                ui.add_space(6.0);
                widgets::quality_presets_row(ui, &mut self.quality, enabled && !self.use_target_max_bytes);

                ui.add_space(6.0);
                widgets::settings_labeled_row(ui, "目标体积", |ui| {
                  ui.checkbox(&mut self.use_target_max_bytes, "限制单文件 ≤");
                  ui.add_enabled_ui(enabled && self.use_target_max_bytes, |ui| {
                    ui.add(
                      egui::DragValue::new(&mut self.target_max_kb)
                        .range(16..=20_480)
                        .suffix(" KB"),
                    );
                  });
                });
                if self.use_target_max_bytes {
                  ui.label(
                    RichText::new("启用后将对 JPEG/WebP 等自动二分搜索质量以控制体积")
                      .size(11.0)
                      .color(theme::secondary_label(dark)),
                  );
                }

                ui.add_space(6.0);
                widgets::settings_labeled_row(ui, "重命名", |ui| {
                  let response = ui.add_enabled_ui(enabled, |ui| {
                    ui.add(
                      egui::TextEdit::singleline(&mut self.rename_template)
                        .desired_width(ui.available_width().min(280.0))
                        .hint_text("{dir}_{stem}_{index}"),
                    )
                  });
                  if response.response.changed() {
                    self.refresh_previews();
                  }
                });
                if let Some(err) = &self.rename_preview_error {
                  ui.colored_label(theme::error_color(dark), err);
                } else if !self.rename_preview.is_empty() {
                  ui.label(
                    RichText::new("预览输出名")
                      .size(11.0)
                      .color(theme::secondary_label(dark)),
                  );
                  for (src, out) in &self.rename_preview {
                    ui.label(
                      RichText::new(format!("{src} → {out}"))
                        .size(11.0)
                        .family(egui::FontFamily::Monospace),
                    );
                  }
                }

                widgets::inset_separator(ui);
                self.settings_checkboxes(ui, enabled);
              });

              ui.add_space(16.0);

              widgets::grouped_section(ui, "转换前摘要", |ui| {
                ui.horizontal(|ui| {
                  if widgets::compact_secondary_button(ui, "刷新预估", enabled).clicked() {
                    self.refresh_previews();
                  }
                  if widgets::compact_secondary_button(ui, "质量体积预览", enabled).clicked() {
                    self.request_quality_preview();
                  }
                });
                if let Some(ref preview) = self.batch_preview {
                  for line in preview.summary_lines(self.formats[self.format_index].extension()) {
                    ui.label(RichText::new(line).size(12.0));
                  }
                  if !preview.samples.is_empty() {
                    ui.add_space(4.0);
                    ui.label(
                      RichText::new("样例路径")
                        .size(11.0)
                        .color(theme::secondary_label(dark)),
                    );
                    for s in &preview.samples {
                      ui.label(
                        RichText::new(format!(
                          "{} → {}",
                          s.input.file_name().and_then(|n| n.to_str()).unwrap_or("?"),
                          s.output.file_name().and_then(|n| n.to_str()).unwrap_or("?")
                        ))
                        .size(11.0)
                        .family(egui::FontFamily::Monospace),
                      );
                    }
                  }
                } else if !self.input_dir.trim().is_empty() {
                  ui.label(
                    RichText::new("点击「刷新预估」查看将转换的文件数")
                      .size(12.0)
                      .color(theme::secondary_label(dark)),
                  );
                }

                if let Some(err) = &self.quality_preview_error {
                  ui.colored_label(theme::error_color(dark), err);
                } else if !self.quality_preview_rows.is_empty() {
                  ui.add_space(6.0);
                  ui.label(
                    RichText::new("单图质量对比（首图采样）")
                      .size(11.0)
                      .color(theme::secondary_label(dark)),
                  );
                  for row in &self.quality_preview_rows {
                    ui.label(format!(
                      "质量 {} → {}",
                      row.quality,
                      quality_preview::format_bytes(row.bytes)
                    ));
                  }
                } else if self.quality_preview_worker.is_some() {
                  ui.label("正在计算质量体积预览…");
                }
              });

              ui.add_space(16.0);

              if running {
                let bar_h = ui.spacing().interact_size.y;
                ui.add_sized(
                  egui::vec2(ui.available_width(), bar_h),
                  egui::ProgressBar::new(
                    match &self.state {
                      RunState::Running { progress, .. } => progress.fraction(),
                      _ => 0.0,
                    },
                  )
                  .text("处理中…")
                  .show_percentage()
                  .animate(running),
                );
                ui.add_space(8.0);
              } else if let RunState::Done(report) = &self.state {
                let ratio = report.compression_ratio() * 100.0;
                ui.label(
                  RichText::new(format!("压缩率约 {ratio:.1}%"))
                    .size(13.0)
                    .color(theme::secondary_label(dark)),
                );
                ui.add_space(8.0);
              }

              widgets::status_banner(ui, &self.status, running);
              ui.add_space(16.0);
              widgets::log_panel(ui, &self.log_lines, log_height);
              ui.add_space(bottom_reserve);
            });
          });
      });
  }

  fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
    let _ = self.gui_prefs.save();
    if let Some(toolbar) = &mut self.native_toolbar {
      toolbar.teardown();
    }
  }
}
