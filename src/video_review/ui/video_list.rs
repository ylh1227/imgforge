//! 视频列表：列表模式 / 卡片模式、筛选、选择与 hover 预览。

use std::collections::HashMap;
use std::path::PathBuf;

use eframe::egui::{self, Color32, Context, RichText, ScrollArea, TextureHandle, Ui};

use crate::gui::widgets;
use crate::review::domain::image_item::ReviewStatus;
use crate::video_review::domain::{VideoFilter, VideoItem, VideoTag};
use crate::video_review::service::VideoReviewService;
use crate::video_review::ui::hover_preview::HoverPreviewController;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum VideoListMode {
    #[default]
    List,
    Card,
}

impl VideoListMode {
    pub fn label(self) -> &'static str {
        match self {
            Self::List => "列表",
            Self::Card => "卡片",
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct VideoListState {
    pub mode: VideoListMode,
    pub search_buf: String,
    pub filter: VideoFilter,
}

#[derive(Debug, Clone, Default)]
pub struct VideoListAction {
    pub reload_videos: bool,
    pub select_video: Option<i64>,
    pub enter_compare: bool,
    pub clear_selection: bool,
    pub toggle_compare_id: Option<(i64, bool)>,
}

pub fn video_list_toolbar_ui(
    ui: &mut Ui,
    state: &mut VideoListState,
    selected_count: usize,
    action: &mut VideoListAction,
) {
    ui.set_width(ui.available_width());

    widgets::mode_tab_bar(
        ui,
        &mut state.mode,
        &[
            (VideoListMode::List, "列表"),
            (VideoListMode::Card, "卡片"),
        ],
    );

    ui.add_space(8.0);

    let gap = 6.0;
    let reset_w = 64.0_f32.min((ui.available_width() * 0.28).max(56.0));
    let field_w = (ui.available_width() - reset_w - gap).max(80.0);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = gap;
        if widgets::toolbar_search_edit(ui, &mut state.search_buf, "文件名…", field_w).changed() {
            state.filter.search = state.search_buf.clone();
            action.reload_videos = true;
        }
        if widgets::full_width_secondary_button_in(
            ui,
            "重置",
            !state.search_buf.is_empty(),
            reset_w,
        )
        .clicked()
        {
            state.search_buf.clear();
            state.filter.search.clear();
            action.reload_videos = true;
        }
    });

    ui.add_space(8.0);

    let cell = ((ui.available_width() - gap * 2.0) / 3.0).max(56.0);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = gap;
        let selected = state.filter.status.map(|s| s.label()).unwrap_or("全部");
        widgets::toolbar_combo_box(ui, "video_status_filter", selected, cell, |ui| {
            if ui
                .selectable_label(state.filter.status.is_none(), "全部")
                .clicked()
            {
                state.filter.status = None;
                action.reload_videos = true;
            }
            for s in ReviewStatus::all() {
                if ui
                    .selectable_label(state.filter.status == Some(s), s.label())
                    .clicked()
                {
                    state.filter.status = Some(s);
                    action.reload_videos = true;
                }
            }
        });

        let compare_label = format!("对比 ({selected_count})");
        let compare_clicked = if selected_count >= 2 {
            widgets::full_width_primary_button_in(ui, &compare_label, true, cell).clicked()
        } else {
            widgets::full_width_secondary_button_in(ui, &compare_label, false, cell).clicked()
        };
        if compare_clicked {
            action.enter_compare = true;
        }
        if widgets::full_width_secondary_button_in(ui, "清空选择", selected_count > 0, cell)
            .clicked()
        {
            action.clear_selection = true;
        }
    });
}

pub fn load_thumb_texture(
    ctx: &Context,
    cache: &mut HashMap<String, TextureHandle>,
    path: &PathBuf,
) -> Option<TextureHandle> {
    let key = path.to_string_lossy().to_string();
    if let Some(tex) = cache.get(&key) {
        return Some(tex.clone());
    }
    let img = image::open(path).ok()?;
    let rgba = img.to_rgba8();
    let size = [rgba.width() as usize, rgba.height() as usize];
    let handle = ctx.load_texture(
        format!("vthumb_{key}"),
        egui::ColorImage::from_rgba_unmultiplied(size, &rgba),
        egui::TextureOptions::LINEAR,
    );
    cache.insert(key, handle.clone());
    Some(handle)
}

pub fn video_list_body_ui(
    ctx: &Context,
    ui: &mut Ui,
    state: &VideoListMode,
    videos: &[VideoItem],
    current_video: Option<i64>,
    selected_ids: &[i64],
    all_tags: &[VideoTag],
    video_tag_map: &HashMap<i64, Vec<i64>>,
    current_time_ms: u64,
    service: &VideoReviewService,
    hover: &mut HoverPreviewController,
    thumb_textures: &mut HashMap<String, TextureHandle>,
    action: &mut VideoListAction,
) {
    hover.begin_frame();

    ScrollArea::vertical()
        .id_salt(match state {
            VideoListMode::List => "video_review_video_list",
            VideoListMode::Card => "video_review_video_cards",
        })
        .max_height(400.0)
        .show(ui, |ui| match state {
            VideoListMode::List => list_mode_ui(
                ctx,
                ui,
                videos,
                current_video,
                selected_ids,
                current_time_ms,
                service,
                hover,
                thumb_textures,
                action,
            ),
            VideoListMode::Card => card_mode_ui(
                ctx,
                ui,
                videos,
                current_video,
                selected_ids,
                all_tags,
                video_tag_map,
                thumb_textures,
                action,
            ),
        });

    if let Some(video_id) = hover.hover.as_ref().map(|h| h.video_id) {
        if let Some(video) = videos.iter().find(|v| v.id == video_id) {
            let frame_path = hover.poll_frame(ctx, service, video);
            let path_ref = frame_path.as_ref();
            hover.show_popup(ctx, ui, video, path_ref);
        }
    }
}

fn list_mode_ui(
    ctx: &Context,
    ui: &mut Ui,
    videos: &[VideoItem],
    current_video: Option<i64>,
    selected_ids: &[i64],
    current_time_ms: u64,
    _service: &VideoReviewService,
    hover: &mut HoverPreviewController,
    thumb_textures: &mut HashMap<String, TextureHandle>,
    action: &mut VideoListAction,
) {
    let mut hovered_id = None;
    for video in videos {
        let id = video.id;
        let name = video
            .file_path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        let checked = selected_ids.contains(&id);
        let selected = current_video == Some(id);

        let row = ui.horizontal(|ui| {
            let mut c = checked;
            ui.checkbox(&mut c, "");
            if c != checked {
                action.toggle_compare_id = Some((id, c));
            }
            let c = video.status.color_rgba();
            ui.colored_label(Color32::from_rgba_unmultiplied(c[0], c[1], c[2], c[3]), "●");
            if let Some(ref thumb) = video.thumbnail_path {
                if let Some(tex) = load_thumb_texture(ctx, thumb_textures, thumb) {
                    ui.image((tex.id(), egui::vec2(36.0, 20.0)));
                }
            }
            let resp = ui.selectable_label(selected, &name);
            if resp.clicked() {
                action.select_video = Some(id);
            }
            if resp.hovered() {
                hovered_id = Some(id);
            }
            resp
        });
        if row.inner.hovered() {
            hovered_id = Some(id);
        }
        ui.label(
            RichText::new(format!(
                "{} · {} · {}",
                video.metadata().duration_label(),
                video.metadata().resolution_label(),
                video.video_codec
            ))
            .weak()
            .size(11.0),
        );
    }

    if let Some(id) = hovered_id {
        hover.set_hover(id, current_time_ms);
    } else {
        hover.clear_hover();
    }
}

fn card_mode_ui(
    ctx: &Context,
    ui: &mut Ui,
    videos: &[VideoItem],
    current_video: Option<i64>,
    selected_ids: &[i64],
    all_tags: &[VideoTag],
    video_tag_map: &HashMap<i64, Vec<i64>>,
    thumb_textures: &mut HashMap<String, TextureHandle>,
    action: &mut VideoListAction,
) {
    let avail_w = ui.available_width().max(120.0);
    let cols = if avail_w >= 360.0 { 2 } else { 1 };
    let gap = 6.0;
    let card_w = (avail_w - gap * (cols as f32 - 1.0)) / cols as f32;

    egui::Grid::new("video_card_grid")
        .spacing([gap, gap])
        .show(ui, |ui| {
            for (i, video) in videos.iter().enumerate() {
                let id = video.id;
                let selected = current_video == Some(id);
                let checked = selected_ids.contains(&id);
                let name = video
                    .file_path
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_default();

                ui.vertical(|ui| {
                    ui.set_width(card_w);
                    let frame = egui::Frame::group(ui.style())
                        .fill(if selected {
                            ui.visuals().selection.bg_fill
                        } else {
                            ui.visuals().extreme_bg_color
                        })
                        .corner_radius(6.0);
                    frame.show(ui, |ui| {
                        ui.set_width(card_w - 12.0);
                        if let Some(ref thumb) = video.thumbnail_path {
                            if let Some(tex) = load_thumb_texture(ctx, thumb_textures, thumb) {
                                let resp = ui.add(
                                    egui::ImageButton::new((
                                        tex.id(),
                                        egui::vec2(card_w - 16.0, 72.0),
                                    ))
                                    .selected(selected),
                                );
                                if resp.clicked() {
                                    action.select_video = Some(id);
                                }
                                if resp.double_clicked() {
                                    action.enter_compare = true;
                                    action.select_video = Some(id);
                                }
                            }
                        }
                        ui.horizontal(|ui| {
                            let mut c = checked;
                            ui.checkbox(&mut c, "");
                            if c != checked {
                                action.toggle_compare_id = Some((id, c));
                            }
                            let st = video.status.color_rgba();
                            ui.colored_label(
                                Color32::from_rgba_unmultiplied(st[0], st[1], st[2], st[3]),
                                "●",
                            );
                            ui.label(RichText::new(&name).strong().size(11.0));
                        });
                        ui.label(
                            RichText::new(format!(
                                "{} · {} · {:.1}fps",
                                video.metadata().duration_label(),
                                video.metadata().resolution_label(),
                                video.fps
                            ))
                            .weak()
                            .size(10.0),
                        );
                        ui.label(
                            RichText::new(format!("编码 {}", video.video_codec))
                                .weak()
                                .size(10.0),
                        );
                        if video.offset_ms != 0 {
                            ui.label(
                                RichText::new(format!("偏移 {}ms", video.offset_ms))
                                    .weak()
                                    .size(10.0),
                            );
                        }
                        if let Some(tag_ids) = video_tag_map.get(&id) {
                            ui.horizontal_wrapped(|ui| {
                                for tid in tag_ids {
                                    if let Some(tag) = all_tags.iter().find(|t| t.id == *tid) {
                                        let c = tag.color;
                                        ui.colored_label(
                                            Color32::from_rgba_unmultiplied(c[0], c[1], c[2], c[3]),
                                            "■",
                                        );
                                        ui.label(RichText::new(&tag.name).size(10.0));
                                    }
                                }
                            });
                        }
                    });
                });
                if (i + 1) % cols == 0 {
                    ui.end_row();
                }
            }
        });
}

pub fn format_card_meta(video: &VideoItem) -> String {
    format!(
        "{} · {} · {:.1}fps · {}",
        video.metadata().duration_label(),
        video.metadata().resolution_label(),
        video.fps,
        video.video_codec
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::video_review::domain::VideoItem;
    use chrono::Utc;

    fn sample() -> VideoItem {
        VideoItem {
            id: 1,
            batch_id: 1,
            file_path: "/tmp/demo.mp4".into(),
            status: ReviewStatus::Pending,
            remark: None,
            thumbnail_path: None,
            duration_ms: 125_000,
            fps: 24.0,
            width: 1920,
            height: 1080,
            video_codec: "h264".into(),
            audio_codec: None,
            bitrate_kbps: None,
            device_model: None,
            offset_ms: 0,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            deleted_at: None,
        }
    }

    #[test]
    fn card_meta_format() {
        let s = format_card_meta(&sample());
        assert!(s.contains("h264"));
        assert!(s.contains("24.0fps"));
    }

    #[test]
    fn list_mode_labels() {
        assert_eq!(VideoListMode::List.label(), "列表");
        assert_eq!(VideoListMode::Card.label(), "卡片");
        assert_ne!(VideoListMode::List, VideoListMode::Card);
    }
}
