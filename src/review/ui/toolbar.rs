//! 标注工具栏。

use eframe::egui::{self, Ui};

use crate::review::domain::AnnotationStyle;
use crate::review::ui::canvas::DrawTool;

pub fn annotation_toolbar(ui: &mut Ui, tool: &mut DrawTool, style: &mut AnnotationStyle, compare: &mut bool) {
  ui.horizontal(|ui| {
    ui.selectable_value(tool, DrawTool::Pan, "平移");
    ui.selectable_value(tool, DrawTool::Rectangle, "矩形");
    ui.selectable_value(tool, DrawTool::Arrow, "箭头");
    ui.selectable_value(tool, DrawTool::Text, "文字");
    ui.separator();
    ui.checkbox(compare, "对比视图");
    ui.separator();
    let mut rgba = [
      style.color[0] as f32 / 255.0,
      style.color[1] as f32 / 255.0,
      style.color[2] as f32 / 255.0,
      1.0,
    ];
    if ui.color_edit_button_rgba_unmultiplied(&mut rgba).changed() {
      style.color = [
        (rgba[0] * 255.0) as u8,
        (rgba[1] * 255.0) as u8,
        (rgba[2] * 255.0) as u8,
        255,
      ];
    }
    ui.add(egui::Slider::new(&mut style.line_width, 1.0..=8.0).text("线宽"));
  });
}
