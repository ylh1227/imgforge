//! 文件夹批量截图：参考图上框选归一化 ROI。

use std::path::PathBuf;

use eframe::egui::{self, Color32, CornerRadius, Pos2, Rect, RichText, Sense, Vec2};

use crate::gui::theme;
use crate::gui::widgets;
use crate::review::domain::coords::{egui_bridge, Vec2 as DomVec2};
use crate::review::domain::{
    norm_rect_to_screen, screen_rect_to_norm, NormRect, ScreenPoint, ViewportTransform,
};
use crate::review::service::{format_roi_label, is_meaningful_roi};

/// 文件夹 ROI 对话框状态。
pub struct FolderRoiDialogState {
    pub paths: Vec<PathBuf>,
    pub source_label: String,
    pub preview_index: usize,
    pub crop: Option<NormRect>,
    drag_start: Option<Pos2>,
    drag_current: Option<Pos2>,
    texture: Option<egui::TextureHandle>,
    texture_path: Option<PathBuf>,
    image_size: (u32, u32),
    load_error: Option<String>,
}

pub enum FolderRoiDialogAction {
    None,
    Cancel,
    /// 用户确认；`None` = 整图。
    Confirm { crop: Option<NormRect> },
}

impl FolderRoiDialogState {
    pub fn new(paths: Vec<PathBuf>, source_label: String) -> Self {
        Self {
            paths,
            source_label,
            preview_index: 0,
            crop: None,
            drag_start: None,
            drag_current: None,
            texture: None,
            texture_path: None,
            image_size: (0, 0),
            load_error: None,
        }
    }

    pub fn preview_path(&self) -> Option<&PathBuf> {
        self.paths.get(self.preview_index)
    }

    fn ensure_texture(&mut self, ctx: &egui::Context) {
        let Some(path) = self.preview_path().cloned() else {
            return;
        };
        if self.texture_path.as_ref() == Some(&path) && self.texture.is_some() {
            return;
        }
        self.texture = None;
        self.texture_path = Some(path.clone());
        self.load_error = None;
        match image::open(&path) {
            Ok(img) => {
                let rgba = img.to_rgba8();
                self.image_size = (rgba.width(), rgba.height());
                let size = [rgba.width() as usize, rgba.height() as usize];
                let pixels = rgba.into_raw();
                let color_image = egui::ColorImage::from_rgba_unmultiplied(size, &pixels);
                self.texture =
                    Some(ctx.load_texture("folder_roi_preview", color_image, Default::default()));
            }
            Err(e) => {
                self.image_size = (0, 0);
                self.load_error = Some(format!("无法打开预览：{e}"));
            }
        }
    }

    pub fn show(&mut self, ctx: &egui::Context) -> FolderRoiDialogAction {
        self.ensure_texture(ctx);
        let mut action = FolderRoiDialogAction::None;
        let dark = ctx.style().visuals.dark_mode;

        egui::Window::new("框选导出区域")
            .collapsible(false)
            .resizable(true)
            .default_size([720.0, 560.0])
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                ui.label(
                    RichText::new(format!(
                        "源：{} · 共 {} 张 · 归一化 ROI 将套用到整批",
                        self.source_label,
                        self.paths.len()
                    ))
                    .size(12.0)
                    .color(theme::secondary_label(dark)),
                );
                ui.add_space(8.0);

                ui.horizontal(|ui| {
                    ui.label("预览图");
                    let label = self
                        .preview_path()
                        .and_then(|p| p.file_name())
                        .map(|n| n.to_string_lossy().to_string())
                        .unwrap_or_else(|| "(无)".into());
                    egui::ComboBox::from_id_salt("folder_roi_preview_pick")
                        .selected_text(label)
                        .width(280.0)
                        .show_ui(ui, |ui| {
                            for (i, path) in self.paths.iter().enumerate() {
                                let name = path
                                    .file_name()
                                    .map(|n| n.to_string_lossy().to_string())
                                    .unwrap_or_else(|| path.display().to_string());
                                if ui.selectable_label(self.preview_index == i, name).clicked() {
                                    self.preview_index = i;
                                    self.texture = None;
                                    self.texture_path = None;
                                }
                            }
                        });
                    if widgets::compact_secondary_button(ui, "整图", true).clicked() {
                        self.crop = None;
                        self.drag_start = None;
                        self.drag_current = None;
                    }
                });

                if self.image_size.0 > 0 {
                    ui.label(
                        RichText::new(format!(
                            "参考尺寸 {}×{} · {}",
                            self.image_size.0,
                            self.image_size.1,
                            format_roi_label(self.live_or_stored_roi())
                        ))
                        .size(12.0)
                        .color(theme::secondary_label(dark)),
                    );
                }
                if let Some(err) = &self.load_error {
                    ui.label(RichText::new(err).color(theme::error_color(dark)));
                }
                ui.label(
                    RichText::new("在预览上拖拽框选；不选或点「整图」则导出完整画面")
                        .weak()
                        .size(11.0),
                );

                ui.add_space(6.0);
                let available = ui.available_size();
                let preview_h = (available.y - 56.0).clamp(240.0, 480.0);
                let preview_w = available.x.max(320.0);
                self.draw_preview(ui, Vec2::new(preview_w, preview_h), dark);

                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    if widgets::secondary_button(ui, "取消", true).clicked() {
                        action = FolderRoiDialogAction::Cancel;
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if widgets::primary_button(ui, "开始导出…", !self.paths.is_empty()).clicked()
                        {
                            action = FolderRoiDialogAction::Confirm {
                                crop: self.crop.filter(is_meaningful_roi),
                            };
                        }
                    });
                });
            });

        action
    }

    fn live_or_stored_roi(&self) -> Option<NormRect> {
        self.crop
    }

    fn draw_preview(&mut self, ui: &mut egui::Ui, size: Vec2, dark: bool) {
        let (response, painter) = ui.allocate_painter(size, Sense::click_and_drag());
        let rect = response.rect;
        painter.rect_filled(
            rect,
            CornerRadius::same(8),
            if dark {
                Color32::from_rgb(28, 28, 30)
            } else {
                Color32::from_rgb(235, 235, 240)
            },
        );

        let Some(texture) = self.texture.clone() else {
            painter.text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                self.load_error.as_deref().unwrap_or("加载预览中…"),
                egui::FontId::proportional(14.0),
                theme::secondary_label(dark),
            );
            return;
        };

        if self.image_size.0 == 0 || self.image_size.1 == 0 {
            return;
        }

        let mut transform = ViewportTransform::fit_image(
            DomVec2 {
                x: rect.width(),
                y: rect.height(),
            },
            self.image_size,
        );
        transform.image_rect.min.x += rect.min.x;
        transform.image_rect.min.y += rect.min.y;

        let image_screen = egui_bridge::to_egui_rect(transform.displayed_image_rect());
        painter.image(
            texture.id(),
            image_screen,
            Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
            Color32::WHITE,
        );

        if response.drag_started() {
            if let Some(pos) = response.interact_pointer_pos() {
                self.drag_start = Some(pos);
                self.drag_current = Some(pos);
            }
        }
        if response.dragged() {
            if let Some(pos) = response.interact_pointer_pos() {
                self.drag_current = Some(pos);
                if let Some(a) = self.drag_start {
                    let norm = screen_rect_to_norm(
                        ScreenPoint { x: a.x, y: a.y },
                        ScreenPoint { x: pos.x, y: pos.y },
                        &transform,
                    );
                    self.crop = if is_meaningful_roi(&norm) {
                        Some(norm)
                    } else {
                        None
                    };
                }
            }
        }
        if response.drag_stopped() {
            self.drag_start = None;
            self.drag_current = None;
        }

        if let Some(roi) = self.crop.filter(is_meaningful_roi) {
            draw_roi_overlay(&painter, &transform, roi, image_screen);
        }
    }
}

fn draw_roi_overlay(
    painter: &egui::Painter,
    transform: &ViewportTransform,
    roi: NormRect,
    image_screen: Rect,
) {
    let sel = egui_bridge::to_egui_rect(norm_rect_to_screen(roi, transform));
    let dim = Color32::from_black_alpha(110);
    let top = Rect::from_min_max(
        image_screen.min,
        Pos2::new(
            image_screen.max.x,
            sel.min.y.clamp(image_screen.min.y, image_screen.max.y),
        ),
    );
    let bottom = Rect::from_min_max(
        Pos2::new(
            image_screen.min.x,
            sel.max.y.clamp(image_screen.min.y, image_screen.max.y),
        ),
        image_screen.max,
    );
    let left = Rect::from_min_max(
        Pos2::new(image_screen.min.x, sel.min.y),
        Pos2::new(
            sel.min.x.clamp(image_screen.min.x, image_screen.max.x),
            sel.max.y,
        ),
    );
    let right = Rect::from_min_max(
        Pos2::new(
            sel.max.x.clamp(image_screen.min.x, image_screen.max.x),
            sel.min.y,
        ),
        Pos2::new(image_screen.max.x, sel.max.y),
    );
    for r in [top, bottom, left, right] {
        if r.width() > 0.5 && r.height() > 0.5 {
            painter.rect_filled(r, 0.0, dim);
        }
    }
    painter.rect_stroke(
        sel,
        0.0,
        egui::Stroke::new(2.0, Color32::from_rgb(0, 122, 255)),
        egui::StrokeKind::Outside,
    );
}
