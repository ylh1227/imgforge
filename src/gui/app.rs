//! ImgForge 图形界面：文件夹选择、格式设置、进度与结果展示。

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::sync::Arc;
use std::thread;

use eframe::egui::{self, RichText, ScrollArea};

use crate::config::AppConfig;
use crate::core::types::{ImageFormat, MetadataPolicy, Quality};
use crate::gui::{fonts, native, theme, widgets};
use crate::job::run_batch;
use crate::ui::progress::{GuiProgress, ProgressReporter};
use crate::ui::report::ProcessReport;

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

/// 主窗口应用。
pub struct ImgforgeApp {
  input_dir: String,
  output_dir: String,
  format_index: usize,
  formats: Vec<ImageFormat>,
  quality: u8,
  recursive: bool,
  preserve_structure: bool,
  overwrite: bool,
  strip_metadata: bool,
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
    Self {
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
    let quality = Quality::new(self.quality).map_err(|e| e.to_string())?;

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
    config.validate().map_err(|e| e.to_string())?;
    Ok(config)
  }

  fn start_conversion(&mut self) {
    if self.is_running() {
      return;
    }

    let config = match self.build_config() {
      Ok(c) => c,
      Err(e) => {
        self.status = e.clone();
        self.push_log(format!("错误：{e}"));
        return;
      }
    };

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
    let two_col = ui.available_width() > theme::NARROW_BREAKPOINT;
    let options = [
      (&mut self.recursive, "包含子文件夹"),
      (&mut self.preserve_structure, "保留目录结构"),
      (&mut self.overwrite, "覆盖已有文件"),
      (&mut self.strip_metadata, "移除 EXIF 元数据"),
    ];

    if two_col {
      ui.columns(2, |cols| {
        for (idx, (value, label)) in options.into_iter().enumerate() {
          cols[idx % 2].add_enabled(enabled, egui::Checkbox::new(value, label));
        }
      });
    } else {
      for (value, label) in options {
        ui.add_enabled(enabled, egui::Checkbox::new(value, label));
      }
    }
  }
}

impl eframe::App for ImgforgeApp {
  fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
    self.poll_worker();

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

    if !native_toolbar_active {
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
        ScrollArea::vertical()
          .auto_shrink([false, false])
          .show(ui, |ui| {
            ui.vertical_centered(|ui| {
              ui.set_width(content_w);

              widgets::navigation_header(ui, "批量图片格式转换");
              ui.add_space(20.0);

              widgets::grouped_section(ui, "文件夹", |ui| {
                widgets::folder_field(ui, "输入", &mut self.input_dir, enabled);
                widgets::folder_field(ui, "输出", &mut self.output_dir, enabled);
                if self.input_dir.trim().is_empty() {
                  widgets::drop_hint(ui);
                }
              });

              ui.add_space(16.0);

              widgets::grouped_section(ui, "转换设置", |ui| {
                ui.horizontal(|ui| {
                  ui.label(
                    RichText::new("目标格式")
                      .font(theme::section_font())
                      .color(theme::primary_label(dark)),
                  );
                  ui.add_space(8.0);
                  egui::ComboBox::from_id_salt("format")
                    .width(f32::min(128.0, ui.available_width() * 0.35))
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

                ui.add_space(10.0);
                widgets::quality_slider_row(ui, &mut self.quality, enabled);

                ui.add_space(8.0);
                widgets::quality_presets_row(ui, &mut self.quality, enabled);

                ui.add_space(10.0);
                self.settings_checkboxes(ui, enabled);
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
}
