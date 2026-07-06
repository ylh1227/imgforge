//! 多视频同步抽帧对比（2–6 路）。

use std::collections::HashMap;
use std::path::PathBuf;

use eframe::egui::{self, Color32, Context, RichText, TextureHandle, Ui, Vec2};

use crate::video_review::domain::VideoItem;
use crate::video_review::service::VideoReviewService;

pub const MAX_COMPARE_VIDEOS: usize = 6;

#[derive(Clone, Default)]
pub struct MultiVideoCompare {
  pub current_time_ms: u64,
  pub compare_ids: Vec<i64>,
  textures: HashMap<String, TextureHandle>,
}

impl MultiVideoCompare {
  pub fn with_time(current_time_ms: u64) -> Self {
    Self {
      current_time_ms,
      ..Default::default()
    }
  }

  pub fn set_compare_ids(&mut self, ids: Vec<i64>) {
    self.compare_ids = ids.into_iter().take(MAX_COMPARE_VIDEOS).collect();
  }

  pub fn ui(
    &mut self,
    ctx: &Context,
    ui: &mut Ui,
    service: &VideoReviewService,
    videos: &[VideoItem],
    area: Vec2,
  ) {
    let selected: Vec<&VideoItem> = self
      .compare_ids
      .iter()
      .filter_map(|id| videos.iter().find(|v| v.id == *id))
      .collect();

    if selected.is_empty() {
      ui.centered_and_justified(|ui| {
        ui.label(RichText::new("在左侧勾选 2–6 个视频后进入对比").weak());
      });
      return;
    }

    if selected.len() == 1 {
      self.draw_single_pane(ctx, ui, service, selected[0], area);
      return;
    }

    if selected.len() == 2 {
      ui.horizontal(|ui| {
        let half = (area.x - 6.0) / 2.0;
        let pane = Vec2::new(half.max(120.0), area.y);
        for video in &selected {
          ui.vertical(|ui| {
            self.draw_video_pane(ctx, ui, service, video, pane);
          });
          ui.add_space(6.0);
        }
      });
      return;
    }

    let cols = if selected.len() <= 4 { 2 } else { 3 };
    let gap = 6.0;
    let cell_w = (area.x - gap * (cols as f32 - 1.0)) / cols as f32;
    let rows = (selected.len() + cols - 1) / cols;
    let cell_h = (area.y - gap * (rows as f32 - 1.0)) / rows as f32;
    let cell = Vec2::new(cell_w.max(100.0), cell_h.max(80.0));

    egui::Grid::new("video_multi_grid")
      .spacing([gap, gap])
      .show(ui, |ui| {
        for (i, video) in selected.iter().enumerate() {
          ui.vertical(|ui| {
            self.draw_video_pane(ctx, ui, service, video, cell);
          });
          if (i + 1) % cols == 0 {
            ui.end_row();
          }
        }
      });
  }

  fn draw_single_pane(
    &mut self,
    ctx: &Context,
    ui: &mut Ui,
    service: &VideoReviewService,
    video: &VideoItem,
    area: Vec2,
  ) {
    self.draw_video_pane(ctx, ui, service, video, area);
  }

  fn draw_video_pane(
    &mut self,
    ctx: &Context,
    ui: &mut Ui,
    service: &VideoReviewService,
    video: &VideoItem,
    area: Vec2,
  ) {
    let name = video
      .file_path
      .file_name()
      .map(|n| n.to_string_lossy().to_string())
      .unwrap_or_else(|| format!("视频 #{}", video.id));

    ui.horizontal(|ui| {
      let c = video.status.color_rgba();
      ui.colored_label(
        Color32::from_rgba_unmultiplied(c[0], c[1], c[2], c[3]),
        "●",
      );
      ui.label(RichText::new(&name).strong().size(12.0));
      if video.offset_ms != 0 {
        ui.label(
          RichText::new(format!("偏移 {}ms", video.offset_ms))
            .weak()
            .size(11.0),
        );
      }
    });

    let frame_area = Vec2::new(area.x, (area.y - 22.0).max(60.0));
    let (rect, _) = ui.allocate_exact_size(frame_area, egui::Sense::hover());

    let effective = video.effective_time_ms(self.current_time_ms).min(video.duration_ms);
    let frame_path = service
      .frame_at(video, self.current_time_ms, 640)
      .ok()
      .flatten();

    if let Some(path) = frame_path {
      if let Some(tex) = self.load_texture(ctx, &path) {
        let tex_size = tex.size_vec2();
        let scale = (rect.width() / tex_size.x).min(rect.height() / tex_size.y);
        let display = tex_size * scale;
        let offset = rect.center() - display * 0.5;
        let img_rect = egui::Rect::from_min_size(offset, display);
        ui.painter().image(
          tex.id(),
          img_rect,
          egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
          Color32::WHITE,
        );
      } else {
        ui.painter().rect_filled(rect, 4.0, ui.visuals().extreme_bg_color);
        ui.painter().text(
          rect.center(),
          egui::Align2::CENTER_CENTER,
          "加载帧…",
          egui::FontId::proportional(12.0),
          ui.visuals().weak_text_color(),
        );
      }
    } else {
      ui.painter().rect_filled(rect, 4.0, ui.visuals().extreme_bg_color);
      ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        "抽帧中…",
        egui::FontId::proportional(12.0),
        ui.visuals().weak_text_color(),
      );
    }

    ui.label(
      RichText::new(format!(
        "{} / {}",
        format_ms(effective),
        format_ms(video.duration_ms)
      ))
      .weak()
      .size(11.0),
    );
  }

  fn load_texture(&mut self, ctx: &Context, path: &PathBuf) -> Option<TextureHandle> {
    let key = path.to_string_lossy().to_string();
    if self.textures.contains_key(&key) {
      return self.textures.get(&key).cloned();
    }
    let img = image::open(path).ok()?;
    let rgba = img.to_rgba8();
    let size = [rgba.width() as usize, rgba.height() as usize];
    let handle = ctx.load_texture(
      format!("video_frame_{key}"),
      egui::ColorImage::from_rgba_unmultiplied(size, &rgba),
      egui::TextureOptions::LINEAR,
    );
    self.textures.insert(key, handle.clone());
    Some(handle)
  }
}

pub fn format_ms(ms: u64) -> String {
  let total = ms / 1000;
  let m = total / 60;
  let s = total % 60;
  let frac = ms % 1000;
  format!("{m:02}:{s:02}.{frac:03}")
}
