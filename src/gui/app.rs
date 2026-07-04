//! ImgForge 图形界面：文件夹选择、格式设置、进度与结果展示。

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::sync::Arc;
use std::thread;

use eframe::egui;

use crate::config::AppConfig;
use crate::core::types::{ImageFormat, MetadataPolicy, Quality};
use crate::gui::fonts;
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
  Failed(String),
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
}

impl ImgforgeApp {
  pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
    fonts::install_cjk_fonts(&cc.egui_ctx);

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
        self.state = RunState::Failed(e);
      }
    }
  }

  fn folder_row(ui: &mut egui::Ui, label: &str, path: &mut String, enabled: bool) {
    ui.horizontal(|ui| {
      ui.label(label);
      ui.add_enabled(
        enabled,
        egui::TextEdit::singleline(path).desired_width(ui.available_width() - 90.0),
      );
      if ui.add_enabled(enabled, egui::Button::new("浏览…")).clicked() {
        if let Some(folder) = rfd::FileDialog::new().pick_folder() {
          *path = folder.display().to_string();
        }
      }
    });
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
}

impl eframe::App for ImgforgeApp {
  fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
    self.poll_worker();

    let enabled = !self.is_running();

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

    egui::CentralPanel::default().show(ctx, |ui| {
      ui.heading("ImgForge");
      ui.label("批量图片格式转换");
      ui.add_space(8.0);

      let enabled = !self.is_running();

      ui.group(|ui| {
        ui.label(egui::RichText::new("文件夹").strong());
        ui.add_space(4.0);
        Self::folder_row(ui, "输入", &mut self.input_dir, enabled);
        Self::folder_row(ui, "输出", &mut self.output_dir, enabled);
      });

      ui.add_space(8.0);

      ui.group(|ui| {
        ui.label(egui::RichText::new("转换设置").strong());
        ui.add_space(4.0);

        ui.horizontal(|ui| {
          ui.label("目标格式");
          egui::ComboBox::from_id_salt("format")
            .selected_text(self.formats[self.format_index].extension())
            .show_ui(ui, |ui| {
              for (idx, format) in self.formats.iter().enumerate() {
                ui.selectable_value(&mut self.format_index, idx, format.extension());
              }
            });
        });

        ui.horizontal(|ui| {
          ui.label(format!("质量：{}", self.quality));
          ui.add_enabled(enabled, egui::Slider::new(&mut self.quality, 1..=100));
        });

        ui.add_enabled(enabled, egui::Checkbox::new(&mut self.recursive, "包含子文件夹"));
        ui.add_enabled(
          enabled,
          egui::Checkbox::new(&mut self.preserve_structure, "保留目录结构"),
        );
        ui.add_enabled(enabled, egui::Checkbox::new(&mut self.overwrite, "覆盖已有文件"));
        ui.add_enabled(
          enabled,
          egui::Checkbox::new(&mut self.strip_metadata, "移除 EXIF 元数据"),
        );
      });

      ui.add_space(8.0);

      if let RunState::Running { progress, .. } = &self.state {
        ui.add(egui::ProgressBar::new(progress.fraction()).show_percentage());
      } else if let RunState::Done(report) = &self.state {
        let ratio = report.compression_ratio() * 100.0;
        ui.label(format!("压缩率约 {ratio:.1}%"));
      }

      ui.label(&self.status);
      ui.add_space(8.0);

      ui.horizontal(|ui| {
        if ui
          .add_enabled(enabled, egui::Button::new("开始转换"))
          .clicked()
        {
          self.start_conversion();
        }
        if ui
          .add_enabled(self.is_running(), egui::Button::new("取消"))
          .clicked()
        {
          self.cancel_conversion();
        }
        if ui.button("打开输出文件夹").clicked() {
          self.open_output_folder();
        }
      });

      ui.add_space(8.0);
      ui.label(egui::RichText::new("日志").strong());
      egui::ScrollArea::vertical()
        .max_height(160.0)
        .stick_to_bottom(true)
        .show(ui, |ui| {
          for line in &self.log_lines {
            ui.label(line);
          }
        });
    });
  }
}
