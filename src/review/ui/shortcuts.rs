//! 快捷键匹配（egui 输入层）。

use eframe::egui::{self, Key};

use crate::review::service::{ShortcutAction, ShortcutConfig};

pub fn handle_shortcuts(config: &ShortcutConfig, ctx: &egui::Context) -> Option<ShortcutAction> {
  let input = ctx.input(|i| i.clone());
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
    if matches_action(action, config, &input) {
      return Some(action);
    }
  }
  None
}

fn matches_action(action: ShortcutAction, config: &ShortcutConfig, input: &egui::InputState) -> bool {
  let Some(spec) = config.bindings.get(&action) else {
    return false;
  };
  spec.split(',').any(|part| match_binding(part.trim(), input))
}

fn match_binding(spec: &str, input: &egui::InputState) -> bool {
  let ctrl = spec.contains("Ctrl+") || spec.contains("Command+");
  let key_part = spec
    .replace("Ctrl+", "")
    .replace("Command+", "");
  if ctrl && !(input.modifiers.ctrl || input.modifiers.command) {
    return false;
  }
  match key_part.as_str() {
    "A" => input.key_pressed(Key::A),
    "D" => input.key_pressed(Key::D),
    "Left" => input.key_pressed(Key::ArrowLeft),
    "Right" => input.key_pressed(Key::ArrowRight),
    "0" => input.key_pressed(Key::Num0),
    "1" => input.key_pressed(Key::Num1),
    "2" => input.key_pressed(Key::Num2),
    "3" => input.key_pressed(Key::Num3),
    "Z" => input.key_pressed(Key::Z),
    _ => false,
  }
}
