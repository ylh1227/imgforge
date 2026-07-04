//! ImgForge 图形界面入口（双击运行，无需命令行）。

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() -> eframe::Result<()> {
  let options = eframe::NativeOptions {
    viewport: egui::ViewportBuilder::default()
      .with_inner_size([760.0, 680.0])
      .with_min_inner_size([640.0, 520.0])
      .with_title("ImgForge"),
    ..Default::default()
  };

  eframe::run_native(
    "ImgForge",
    options,
    Box::new(|cc| Ok(Box::new(imgforge::gui::ImgforgeApp::new(cc)))),
  )
}
