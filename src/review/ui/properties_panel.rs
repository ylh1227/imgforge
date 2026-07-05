//! 右侧属性面板：元数据 + 转换参数。

use eframe::egui::{self, RichText, Ui};

use crate::core::types::ImageFormat;
use crate::gui::{theme, widgets};
use crate::review::domain::convert_params::ConvertParams;
use crate::review::domain::metadata::ImageMetadata;
use crate::review::domain::ReviewImageItem;

pub struct PropertiesPanelState {
  pub metadata: Option<ImageMetadata>,
  pub convert_draft: ConvertParams,
  pub format_index: usize,
}

impl Default for PropertiesPanelState {
  fn default() -> Self {
    Self {
      metadata: None,
      convert_draft: ConvertParams::default(),
      format_index: 0,
    }
  }
}

impl PropertiesPanelState {
  pub fn sync_item(&mut self, item: &ReviewImageItem, metadata: Option<ImageMetadata>) {
    self.metadata = metadata;
    self.convert_draft = item.convert_params.clone();
    let formats = ImageFormat::all_supported();
    self.format_index = self
      .convert_draft
      .format
      .and_then(|f| formats.iter().position(|x| *x == f))
      .unwrap_or(0);
  }
}

pub fn properties_panel_ui(
  ui: &mut Ui,
  state: &mut PropertiesPanelState,
  item: Option<&ReviewImageItem>,
) -> bool {
  let mut changed = false;
  let dark = ui.style().visuals.dark_mode;
  let Some(item) = item else {
    ui.label(
      RichText::new("选择图片以查看属性")
        .color(theme::secondary_label(dark)),
    );
    return false;
  };

  widgets::settings_subheading(ui, "文件");
  ui.label(format!("路径：{}", item.file_path.display()));
  if let Some(meta) = &state.metadata {
    ui.label(format!("分辨率：{}", meta.resolution_label()));
    ui.label(format!(
      "位深：{}",
      meta.bit_depth.map(|b| format!("{b} bit")).unwrap_or_else(|| "—".into())
    ));
    ui.label(format!(
      "色彩：{}",
      meta.color_space.as_deref().unwrap_or("—")
    ));
    ui.label(format!("大小：{}", meta.file_size_label()));
    if let Some(exif) = &meta.exif_summary {
      ui.label(format!("EXIF：{exif}"));
    }
  } else {
    ui.label("元数据加载中…");
  }

  widgets::inset_separator(ui);
  widgets::settings_subheading(ui, "转换参数（加入队列时带入）");
  let formats = ImageFormat::all_supported();
  ui.horizontal(|ui| {
    ui.label("格式");
    egui::ComboBox::from_id_salt("review_convert_format")
      .selected_text(
        state
          .convert_draft
          .format
          .map(|f| f.extension().to_uppercase())
          .unwrap_or_else(|| "默认".into()),
      )
      .show_ui(ui, |ui| {
        if ui.selectable_label(state.convert_draft.format.is_none(), "默认").clicked() {
          state.convert_draft.format = None;
          changed = true;
        }
        for (idx, f) in formats.iter().enumerate() {
          if ui
            .selectable_value(&mut state.format_index, idx, f.extension().to_uppercase())
            .clicked()
          {
            state.convert_draft.format = Some(formats[state.format_index]);
            changed = true;
          }
        }
      });
  });
  ui.horizontal(|ui| {
    ui.label("质量");
    let mut q = state.convert_draft.quality.unwrap_or(85) as i32;
    if ui.add(egui::Slider::new(&mut q, 1..=100)).changed() {
      state.convert_draft.quality = Some(q.clamp(1, 100) as u8);
      changed = true;
    }
  });
  ui.horizontal(|ui| {
    ui.label("宽度");
    let mut w = state.convert_draft.width.unwrap_or(0) as i32;
    if ui
      .add(egui::DragValue::new(&mut w).range(0..=100000).suffix("px"))
      .changed()
    {
      state.convert_draft.width = if w <= 0 { None } else { Some(w as u32) };
      changed = true;
    }
  });
  ui.label(
    RichText::new(format!("当前状态：{}", item.status.label()))
      .size(12.0)
      .color(theme::secondary_label(dark)),
  );
  changed
}
