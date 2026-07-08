//! 列表 hover 预览状态与纹理缓存。

use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use eframe::egui::{self, Color32, Context, RichText, TextureHandle, Ui};

use crate::video_review::domain::VideoItem;
use crate::video_review::service::VideoReviewService;
use crate::video_review::ui::multi_compare::format_ms;

const HOVER_DEBOUNCE: Duration = Duration::from_millis(200);
const PREVIEW_FRAME_WIDTH: u32 = 320;
const MAX_TEXTURES: usize = 64;
const MAX_UPLOADS_PER_FRAME: usize = 2;

#[derive(Debug, Clone)]
pub struct HoverPreviewState {
    pub video_id: i64,
    pub started_at: Instant,
    pub time_ms: u64,
}

impl HoverPreviewState {
    pub fn new(video_id: i64, time_ms: u64) -> Self {
        Self {
            video_id,
            started_at: Instant::now(),
            time_ms,
        }
    }

    pub fn ready(&self) -> bool {
        self.started_at.elapsed() >= HOVER_DEBOUNCE
    }
}

pub struct PreviewTextureCache {
    textures: HashMap<String, TextureHandle>,
    order: VecDeque<String>,
}

impl Default for PreviewTextureCache {
    fn default() -> Self {
        Self {
            textures: HashMap::new(),
            order: VecDeque::new(),
        }
    }
}

impl PreviewTextureCache {
    pub fn get(&self, key: &str) -> Option<TextureHandle> {
        self.textures.get(key).cloned()
    }

    pub fn insert(&mut self, ctx: &Context, key: String, path: &PathBuf) -> Option<TextureHandle> {
        if let Some(tex) = self.textures.get(&key) {
            return Some(tex.clone());
        }
        let img = image::open(path).ok()?;
        let rgba = img.to_rgba8();
        let size = [rgba.width() as usize, rgba.height() as usize];
        let handle = ctx.load_texture(
            format!("vpreview_{key}"),
            egui::ColorImage::from_rgba_unmultiplied(size, &rgba),
            egui::TextureOptions::LINEAR,
        );
        self.textures.insert(key.clone(), handle.clone());
        self.order.push_back(key);
        while self.order.len() > MAX_TEXTURES {
            if let Some(old) = self.order.pop_front() {
                self.textures.remove(&old);
            }
        }
        Some(handle)
    }
}

pub struct HoverPreviewController {
    pub hover: Option<HoverPreviewState>,
    pub textures: PreviewTextureCache,
    uploads_this_frame: usize,
}

impl Default for HoverPreviewController {
    fn default() -> Self {
        Self {
            hover: None,
            textures: PreviewTextureCache::default(),
            uploads_this_frame: 0,
        }
    }
}

impl HoverPreviewController {
    pub fn begin_frame(&mut self) {
        self.uploads_this_frame = 0;
    }

    pub fn set_hover(&mut self, video_id: i64, time_ms: u64) {
        let changed = self
            .hover
            .as_ref()
            .is_none_or(|h| h.video_id != video_id || h.time_ms != time_ms);
        if changed {
            self.hover = Some(HoverPreviewState::new(video_id, time_ms));
        }
    }

    pub fn clear_hover(&mut self) {
        self.hover = None;
    }

    pub fn poll_frame(
        &mut self,
        ctx: &Context,
        service: &VideoReviewService,
        video: &VideoItem,
    ) -> Option<PathBuf> {
        let hover = self.hover.as_ref()?;
        if hover.video_id != video.id || !hover.ready() {
            return None;
        }
        if self.uploads_this_frame >= MAX_UPLOADS_PER_FRAME {
            return None;
        }
        let path = service
            .frame_at(video, hover.time_ms, PREVIEW_FRAME_WIDTH)
            .ok()
            .flatten()?;
        self.uploads_this_frame += 1;
        Some(path)
    }

    pub fn show_popup(
        &mut self,
        ctx: &Context,
        ui: &Ui,
        video: &VideoItem,
        frame_path: Option<&PathBuf>,
    ) {
        let Some(hover) = self.hover.as_ref() else {
            return;
        };
        if hover.video_id != video.id || !hover.ready() {
            return;
        }

        let name = video
            .file_path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();

        let popup_id = egui::Id::new("video_hover_preview");
        let anchor = ui.ctx().pointer_hover_pos().unwrap_or(egui::pos2(8.0, 8.0));
        let offset = egui::vec2(16.0, 16.0);
        let pos = anchor + offset;

        egui::Area::new(popup_id)
            .order(egui::Order::Tooltip)
            .fixed_pos(pos)
            .show(ctx, |ui| {
                egui::Frame::popup(ui.style()).show(ui, |ui| {
                    ui.set_max_width(280.0);
                    ui.label(RichText::new(&name).strong().size(12.0));
                    ui.label(
                        RichText::new(format!(
                            "{} · {} · {:.1}fps",
                            video.metadata().duration_label(),
                            video.metadata().resolution_label(),
                            video.fps
                        ))
                        .weak()
                        .size(11.0),
                    );
                    ui.label(
                        RichText::new(format!(
                            "时间 {}",
                            format_ms(
                                video
                                    .effective_time_ms(hover.time_ms)
                                    .min(video.duration_ms)
                            )
                        ))
                        .weak()
                        .size(11.0),
                    );
                    if let Some(path) = frame_path {
                        let key = path.to_string_lossy().to_string();
                        if let Some(tex) = self.textures.insert(ctx, key, path) {
                            ui.image((tex.id(), egui::vec2(256.0, 144.0)));
                        }
                    } else {
                        ui.label(RichText::new("预览加载中…").weak());
                    }
                    let c = video.status.color_rgba();
                    ui.horizontal(|ui| {
                        ui.colored_label(
                            Color32::from_rgba_unmultiplied(c[0], c[1], c[2], c[3]),
                            "●",
                        );
                        ui.label(video.status.label());
                    });
                });
            });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hover_debounce_delay() {
        let h = HoverPreviewState::new(1, 1000);
        assert!(!h.ready());
        std::thread::sleep(Duration::from_millis(210));
        assert!(h.ready());
    }

    #[test]
    fn preview_limits() {
        assert_eq!(HOVER_DEBOUNCE.as_millis(), 200);
        assert_eq!(PREVIEW_FRAME_WIDTH, 320);
        assert_eq!(MAX_TEXTURES, 64);
        assert_eq!(MAX_UPLOADS_PER_FRAME, 2);
    }
}
