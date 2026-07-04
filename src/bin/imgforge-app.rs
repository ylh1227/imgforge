//! ImgForge 图形界面入口（双击运行，无需命令行）。

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() -> eframe::Result<()> {
  let options = eframe::NativeOptions {
    viewport: app_viewport(),
    centered: true,
    ..Default::default()
  };

  eframe::run_native(
    "ImgForge",
    options,
    Box::new(|cc| Ok(Box::new(imgforge::gui::ImgforgeApp::new(cc)))),
  )
}

fn app_viewport() -> egui::ViewportBuilder {
  let builder = egui::ViewportBuilder::default()
    .with_inner_size([820.0, 720.0])
    .with_min_inner_size([680.0, 560.0])
    .with_title("ImgForge")
    .with_app_id("com.imgforge.app");

  #[cfg(target_os = "macos")]
  {
    return builder
      .with_fullsize_content_view(true)
      .with_titlebar_shown(true)
      .with_titlebar_buttons_shown(true);
  }

  #[cfg(not(target_os = "macos"))]
  {
    builder
  }
}
