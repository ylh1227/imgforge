//! 快捷键自定义面板：可视化编辑与导入导出。

use eframe::egui::{self, RichText, Ui};

use crate::gui::{theme, widgets};
use crate::review::service::{ShortcutAction, ShortcutConfig};

pub struct ShortcutPanelState {
  pub draft: ShortcutConfig,
  pub import_buf: String,
  pub message: String,
}

impl ShortcutPanelState {
  pub fn new(config: &ShortcutConfig) -> Self {
    Self {
      draft: config.clone(),
      import_buf: String::new(),
      message: String::new(),
    }
  }
}

pub fn shortcut_panel_ui(ui: &mut Ui, state: &mut ShortcutPanelState) -> bool {
  let dark = ui.style().visuals.dark_mode;
  let mut saved = false;
  widgets::settings_subheading(ui, "快捷键绑定（逗号分隔多键）");
  for action in [
    ShortcutAction::PrevImage,
    ShortcutAction::NextImage,
    ShortcutAction::StatusPending,
    ShortcutAction::StatusApproved,
    ShortcutAction::StatusNeedsFix,
    ShortcutAction::StatusRejected,
    ShortcutAction::FitWindow,
    ShortcutAction::ActualSize,
    ShortcutAction::UndoAnnotation,
  ] {
    ui.horizontal(|ui| {
      ui.allocate_ui_with_layout(
        egui::vec2(88.0, ui.spacing().interact_size.y),
        egui::Layout::left_to_right(egui::Align::Center),
        |ui| {
          ui.label(
            RichText::new(action.label())
              .size(13.0)
              .color(theme::primary_label(dark)),
          );
        },
      );
      let binding = state
        .draft
        .bindings
        .entry(action)
        .or_insert_with(|| ShortcutAction::default_bindings().get(&action).cloned().unwrap_or_default());
      ui.add(egui::TextEdit::singleline(binding).desired_width(ui.available_width()));
    });
  }

  ui.add_space(8.0);
  ui.horizontal(|ui| {
    if widgets::compact_primary_button(ui, "保存", true).clicked() {
      if let Err(e) = state.draft.save() {
        state.message = e.to_string();
      } else {
        state.message = "已保存".into();
        saved = true;
      }
    }
    if widgets::compact_secondary_button(ui, "恢复默认", true).clicked() {
      state.draft = ShortcutConfig::default();
      state.message = "已恢复默认（需保存生效）".into();
    }
    if widgets::compact_secondary_button(ui, "导出 JSON", true).clicked() {
      state.import_buf = serde_json::to_string_pretty(&state.draft).unwrap_or_default();
    }
  });

  ui.add_space(6.0);
  ui.label(
    RichText::new("导入 JSON")
      .size(12.0)
      .color(theme::secondary_label(dark)),
  );
  ui.add(
    egui::TextEdit::multiline(&mut state.import_buf)
      .desired_rows(4)
      .desired_width(f32::INFINITY),
  );
  if widgets::compact_secondary_button(ui, "应用导入", true).clicked() {
    match serde_json::from_str::<ShortcutConfig>(&state.import_buf) {
      Ok(cfg) => {
        state.draft = cfg;
        state.message = "导入成功（需保存生效）".into();
      }
      Err(e) => state.message = format!("导入失败：{e}"),
    }
  }

  if !state.message.is_empty() {
    ui.label(
      RichText::new(&state.message)
        .size(12.0)
        .color(theme::secondary_label(dark)),
    );
  }
  saved
}
