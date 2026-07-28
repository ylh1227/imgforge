//! 多视频同步对比（2–6 路）：画幅装箱、Solo/Wipe/叠化、拖换位与焦点密度。

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Instant;

use eframe::egui::{
    self, Color32, ColorImage, Context, Id, Pos2, Rect, RichText, Sense, Stroke, TextureHandle, Ui,
    Vec2,
};

use crate::gui::theme;
use crate::video_review::domain::VideoItem;
use crate::video_review::playback::{ComparePlayer, PaneTexture};
use crate::video_review::service::VideoReviewService;
use crate::video_review::ui::compare_layout::{
    layout_slots, video_aspect, CompareLayoutPreset, CompareViewMode, PaneSlot,
};

pub const MAX_COMPARE_VIDEOS: usize = 6;
const TITLE_H_FOCUS: f32 = 28.0;
const TITLE_H_COMPACT: f32 = 22.0;
const DIFF_MAX_W: u32 = 640;
const DIFF_THROTTLE_MS: u128 = 100;

/// 对比面板交互。
#[derive(Debug, Clone, Default)]
pub struct CompareUiAction {
    /// `(video_id, frames)`：frames>0 表示该路画面应更早（offset 减小）。
    pub frame_nudges: Vec<(i64, i64)>,
    /// 点选听哪一路
    pub audio_master: Option<i64>,
    /// 拖换位后的完整顺序（同步 selected_ids）
    pub reorder_ids: Option<Vec<i64>>,
    /// 布局预设变更，需写入 GuiPrefs
    pub layout_changed: bool,
}

#[derive(Clone)]
pub struct MultiVideoCompare {
    pub current_time_ms: u64,
    pub compare_ids: Vec<i64>,
    pub layout_preset: CompareLayoutPreset,
    pub view_mode: CompareViewMode,
    pub focused_id: Option<i64>,
    textures: HashMap<String, TextureHandle>,
    diff_tex: Option<TextureHandle>,
    diff_key: Option<(i64, i64, u64)>,
    diff_last: Option<Instant>,
    drag_id: Option<i64>,
    title_rects: Vec<(i64, Rect)>,
}

impl Default for MultiVideoCompare {
    fn default() -> Self {
        Self {
            current_time_ms: 0,
            compare_ids: Vec::new(),
            layout_preset: CompareLayoutPreset::Auto,
            view_mode: CompareViewMode::Grid,
            focused_id: None,
            textures: HashMap::new(),
            diff_tex: None,
            diff_key: None,
            diff_last: None,
            drag_id: None,
            title_rects: Vec::new(),
        }
    }
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
        if let Some(fid) = self.focused_id {
            if !self.compare_ids.contains(&fid) {
                self.focused_id = self.compare_ids.first().copied();
            }
        } else {
            self.focused_id = self.compare_ids.first().copied();
        }
        self.sanitize_view_mode();
    }

    pub fn set_layout_preset(&mut self, preset: CompareLayoutPreset) -> bool {
        if self.layout_preset == preset {
            return false;
        }
        self.layout_preset = preset;
        if !matches!(
            self.view_mode,
            CompareViewMode::Grid | CompareViewMode::Solo { .. }
        ) {
            self.view_mode = CompareViewMode::Grid;
        }
        true
    }

    pub fn enter_wipe(&mut self) {
        if let Some((left, right)) = self.pair_ids() {
            self.view_mode = CompareViewMode::Wipe {
                left,
                right,
                split: 0.5,
            };
        }
    }

    pub fn enter_overlay(&mut self) {
        if let Some((base, top)) = self.pair_ids() {
            self.view_mode = CompareViewMode::Overlay {
                base,
                top,
                opacity: 0.5,
                diff: false,
            };
        }
    }

    pub fn exit_special_mode(&mut self) {
        self.view_mode = CompareViewMode::Grid;
    }

    pub fn toggle_solo_focused(&mut self) {
        match &self.view_mode {
            CompareViewMode::Solo { .. } => {
                self.view_mode = CompareViewMode::Grid;
            }
            _ => {
                if let Some(id) = self
                    .focused_id
                    .or_else(|| self.compare_ids.first().copied())
                {
                    self.view_mode = CompareViewMode::Solo { video_id: id };
                }
            }
        }
    }

    pub fn cycle_focus(&mut self) {
        if self.compare_ids.is_empty() {
            return;
        }
        let cur = self
            .focused_id
            .and_then(|id| self.compare_ids.iter().position(|x| *x == id));
        let next = match cur {
            Some(i) => (i + 1) % self.compare_ids.len(),
            None => 0,
        };
        self.focused_id = Some(self.compare_ids[next]);
    }

    fn pair_ids(&self) -> Option<(i64, i64)> {
        if self.compare_ids.len() < 2 {
            return None;
        }
        if let Some(fid) = self.focused_id {
            if let Some(i) = self.compare_ids.iter().position(|x| *x == fid) {
                let other = self.compare_ids[(i + 1) % self.compare_ids.len()];
                if other != fid {
                    return Some((fid, other));
                }
            }
        }
        Some((self.compare_ids[0], self.compare_ids[1]))
    }

    fn sanitize_view_mode(&mut self) {
        match &self.view_mode {
            CompareViewMode::Solo { video_id } => {
                if !self.compare_ids.contains(video_id) {
                    self.view_mode = CompareViewMode::Grid;
                }
            }
            CompareViewMode::Wipe { left, right, .. } => {
                if !self.compare_ids.contains(left) || !self.compare_ids.contains(right) {
                    self.view_mode = CompareViewMode::Grid;
                }
            }
            CompareViewMode::Overlay { base, top, .. } => {
                if !self.compare_ids.contains(base) || !self.compare_ids.contains(top) {
                    self.view_mode = CompareViewMode::Grid;
                }
            }
            CompareViewMode::Grid => {}
        }
    }

    pub fn ui(
        &mut self,
        ctx: &Context,
        ui: &mut Ui,
        service: &VideoReviewService,
        player: &mut ComparePlayer,
        videos: &[VideoItem],
        area: Vec2,
        scrubbing: bool,
    ) -> CompareUiAction {
        let mut action = CompareUiAction::default();
        let selected: Vec<&VideoItem> = self
            .compare_ids
            .iter()
            .filter_map(|id| videos.iter().find(|v| v.id == *id))
            .collect();

        if selected.is_empty() {
            ui.centered_and_justified(|ui| {
                ui.label(RichText::new("在左侧勾选 2–6 个视频后进入对比").weak());
            });
            return action;
        }

        if self.focused_id.is_none() {
            self.focused_id = selected.first().map(|v| v.id);
        }
        self.title_rects.clear();

        let (response_rect, _) = ui.allocate_exact_size(area, Sense::hover());

        let present_videos: Vec<&VideoItem> = match &self.view_mode {
            CompareViewMode::Solo { video_id } => selected
                .iter()
                .copied()
                .filter(|v| v.id == *video_id)
                .chain(selected.iter().copied().filter(|v| v.id != *video_id))
                .collect(),
            CompareViewMode::Wipe { left, right, .. }
            | CompareViewMode::Overlay {
                base: left,
                top: right,
                ..
            } => selected
                .iter()
                .copied()
                .filter(|v| v.id == *left || v.id == *right)
                .collect(),
            CompareViewMode::Grid => selected.clone(),
        };

        // 先按布局算每路面板像素，再 present，避免统一降到 16:9 小图。
        let mut size_hints = HashMap::new();
        let mut grid_slots: Option<Vec<PaneSlot>> = None;
        match &self.view_mode {
            CompareViewMode::Solo { video_id } => {
                size_hints.insert(
                    *video_id,
                    (
                        response_rect.width().max(160.0) as u32,
                        (response_rect.height() - TITLE_H_FOCUS).max(90.0) as u32,
                    ),
                );
            }
            CompareViewMode::Wipe { left, right, .. }
            | CompareViewMode::Overlay {
                base: left,
                top: right,
                ..
            } => {
                let hw = (response_rect.width() * 0.5).max(160.0) as u32;
                let hh = response_rect.height().max(90.0) as u32;
                size_hints.insert(*left, (hw, hh));
                size_hints.insert(*right, (hw, hh));
            }
            CompareViewMode::Grid => {
                let ids: Vec<i64> = selected.iter().map(|v| v.id).collect();
                let aspects: Vec<f32> = selected
                    .iter()
                    .map(|v| video_aspect(v.width, v.height))
                    .collect();
                let slots = layout_slots(
                    response_rect,
                    self.layout_preset,
                    &ids,
                    &aspects,
                    TITLE_H_FOCUS,
                );
                for s in &slots {
                    size_hints.insert(
                        s.video_id,
                        (
                            s.content.width().max(160.0) as u32,
                            s.content.height().max(90.0) as u32,
                        ),
                    );
                }
                grid_slots = Some(slots);
            }
        }
        player.present_with_sizes(
            ctx,
            &present_videos,
            self.current_time_ms,
            scrubbing,
            area,
            &size_hints,
        );

        match self.view_mode.clone() {
            CompareViewMode::Solo { video_id } => {
                if let Some(video) = selected.iter().find(|v| v.id == video_id).copied() {
                    let slot = PaneSlot {
                        video_id,
                        rect: response_rect,
                        content: Rect::from_min_max(
                            Pos2::new(response_rect.min.x, response_rect.min.y + TITLE_H_FOCUS),
                            response_rect.max,
                        ),
                    };
                    self.draw_pane_at(ctx, ui, service, player, video, slot, true, &mut action);
                }
            }
            CompareViewMode::Wipe { left, right, split } => {
                self.draw_wipe(
                    ctx,
                    ui,
                    service,
                    player,
                    &selected,
                    response_rect,
                    left,
                    right,
                    split,
                );
            }
            CompareViewMode::Overlay {
                base,
                top,
                opacity,
                diff,
            } => {
                self.draw_overlay(
                    ctx,
                    ui,
                    service,
                    player,
                    &selected,
                    response_rect,
                    base,
                    top,
                    opacity,
                    diff,
                );
            }
            CompareViewMode::Grid => {
                let slots = grid_slots.unwrap_or_default();
                self.title_rects = slots
                    .iter()
                    .map(|s| {
                        (
                            s.video_id,
                            Rect::from_min_size(
                                s.rect.min,
                                Vec2::new(s.rect.width(), TITLE_H_FOCUS),
                            ),
                        )
                    })
                    .collect();
                let focused = self.focused_id;
                for slot in slots {
                    let Some(video) = selected.iter().find(|v| v.id == slot.video_id).copied()
                    else {
                        continue;
                    };
                    let is_focus = focused == Some(video.id);
                    self.draw_pane_at(ctx, ui, service, player, video, slot, is_focus, &mut action);
                }
            }
        }

        action
    }

    fn draw_pane_at(
        &mut self,
        ctx: &Context,
        ui: &mut Ui,
        service: &VideoReviewService,
        player: &ComparePlayer,
        video: &VideoItem,
        slot: PaneSlot,
        focused: bool,
        action: &mut CompareUiAction,
    ) {
        let name = file_name(video);
        let short = short_name(&name, 14);
        let effective = video
            .effective_time_ms(self.current_time_ms)
            .min(video.duration_ms);

        let title_h = if focused {
            TITLE_H_FOCUS
        } else {
            TITLE_H_COMPACT
        };
        let title_rect = Rect::from_min_size(slot.rect.min, Vec2::new(slot.rect.width(), title_h));
        let content = Rect::from_min_max(
            Pos2::new(slot.rect.min.x, slot.rect.min.y + title_h),
            slot.rect.max,
        );

        ui.painter().rect_filled(
            slot.rect,
            4.0,
            theme::video_stage_fill(ui.visuals().dark_mode),
        );
        let border = if focused {
            Stroke::new(2.0, ui.visuals().selection.stroke.color)
        } else {
            Stroke::new(1.0, Color32::from_rgb(52, 52, 58))
        };
        ui.painter()
            .rect_stroke(slot.rect, 4.0, border, egui::StrokeKind::Inside);
        ui.painter()
            .rect_filled(title_rect, 0.0, theme::video_pane_title_fill());

        let title_id = Id::new(("compare_title", video.id));
        let title_resp = ui.interact(title_rect, title_id, Sense::click_and_drag());
        if title_resp.clicked() {
            self.focused_id = Some(video.id);
        }
        if title_resp.drag_started() {
            self.drag_id = Some(video.id);
        }
        if title_resp.dragged() {
            if let Some(pos) = title_resp.interact_pointer_pos() {
                ui.painter().text(
                    pos + Vec2::new(8.0, 8.0),
                    egui::Align2::LEFT_TOP,
                    &short,
                    egui::FontId::proportional(12.0),
                    Color32::from_rgba_unmultiplied(255, 255, 255, 200),
                );
            }
        }
        if title_resp.drag_stopped() {
            if let (Some(src), Some(pos)) = (self.drag_id.take(), title_resp.interact_pointer_pos())
            {
                if let Some(ids) = self.try_swap_at(src, pos) {
                    self.compare_ids = ids.clone();
                    action.reorder_ids = Some(ids);
                }
            }
        }

        ui.allocate_new_ui(egui::UiBuilder::new().max_rect(title_rect), |ui| {
            ui.set_clip_rect(title_rect);
            ui.horizontal(|ui| {
                let c = video.status.color_rgba();
                ui.colored_label(Color32::from_rgba_unmultiplied(c[0], c[1], c[2], c[3]), "●");
                let name_c = Color32::from_rgb(230, 230, 235);
                let meta_c = theme::video_timecode_on_stage();
                if focused {
                    ui.label(RichText::new(&name).strong().size(12.0).color(name_c));
                    ui.label(
                        RichText::new(format!("{}ms · {}", video.offset_ms, format_ms(effective)))
                            .monospace()
                            .size(11.0)
                            .color(meta_c),
                    );
                } else {
                    ui.label(RichText::new(&short).size(11.0).color(name_c));
                    ui.label(
                        RichText::new(format_ms_short(effective))
                            .monospace()
                            .size(10.0)
                            .color(meta_c),
                    );
                }
                let is_master = player.audio_master() == Some(video.id);
                let audio_label = if is_master { "🔊" } else { "🔇" };
                if ui
                    .small_button(audio_label)
                    .on_hover_text("听这一路（其它静音）")
                    .clicked()
                {
                    action.audio_master = Some(video.id);
                }
                let show_nudge = focused || title_resp.hovered();
                if show_nudge {
                    if ui
                        .small_button("−1")
                        .on_hover_text("该路画面提前 1 帧")
                        .clicked()
                    {
                        action.frame_nudges.push((video.id, 1));
                    }
                    if ui
                        .small_button("+1")
                        .on_hover_text("该路画面延后 1 帧")
                        .clicked()
                    {
                        action.frame_nudges.push((video.id, -1));
                    }
                }
            });
        });

        let content_id = Id::new(("compare_content", video.id));
        let content_resp = ui.interact(content, content_id, Sense::click());
        if content_resp.clicked() {
            self.focused_id = Some(video.id);
        }
        if content_resp.double_clicked() {
            match &self.view_mode {
                CompareViewMode::Solo { .. } => {
                    self.view_mode = CompareViewMode::Grid;
                }
                _ => {
                    self.view_mode = CompareViewMode::Solo { video_id: video.id };
                    self.focused_id = Some(video.id);
                }
            }
        }

        ui.painter()
            .rect_filled(content, 0.0, Color32::from_rgb(12, 12, 14));
        self.paint_video_frame(ctx, ui, service, player, video, content);
    }

    fn try_swap_at(&self, src: i64, pos: Pos2) -> Option<Vec<i64>> {
        for &(dst, rect) in &self.title_rects {
            if dst == src {
                continue;
            }
            if rect.contains(pos) {
                let mut ids = self.compare_ids.clone();
                let ia = ids.iter().position(|x| *x == src)?;
                let ib = ids.iter().position(|x| *x == dst)?;
                ids.swap(ia, ib);
                return Some(ids);
            }
        }
        None
    }

    fn paint_video_frame(
        &mut self,
        ctx: &Context,
        ui: &mut Ui,
        service: &VideoReviewService,
        player: &ComparePlayer,
        video: &VideoItem,
        rect: Rect,
    ) {
        let painted = if let Some(tex) = player.pane_texture(video.id) {
            paint_pane_tex(ui, rect, &tex, Color32::WHITE, None);
            if player.show_safe_frame() {
                paint_safe_frame(ui, rect);
            }
            true
        } else if let Some(path) = service
            .frame_at(video, self.current_time_ms, 640)
            .ok()
            .flatten()
        {
            if let Some(tex) = self.load_texture(ctx, &path) {
                paint_tex(ui, rect, &tex, Color32::WHITE, None);
                if player.show_safe_frame() {
                    paint_safe_frame(ui, rect);
                }
                true
            } else {
                false
            }
        } else {
            false
        };

        if !painted {
            let msg =
                player
                    .session_error(video.id)
                    .unwrap_or(if player.backend_info().available {
                        "解码中…"
                    } else {
                        "抽帧中…"
                    });
            ui.painter().text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                msg,
                egui::FontId::proportional(12.0),
                ui.visuals().weak_text_color(),
            );
        }
    }

    fn draw_wipe(
        &mut self,
        ctx: &Context,
        ui: &mut Ui,
        service: &VideoReviewService,
        player: &ComparePlayer,
        selected: &[&VideoItem],
        area: Rect,
        left_id: i64,
        right_id: i64,
        split: f32,
    ) {
        let Some(left_v) = selected.iter().find(|v| v.id == left_id).copied() else {
            return;
        };
        let Some(right_v) = selected.iter().find(|v| v.id == right_id).copied() else {
            return;
        };

        let title_h = TITLE_H_FOCUS;
        let title_rect = Rect::from_min_size(area.min, Vec2::new(area.width(), title_h));
        let content = Rect::from_min_max(Pos2::new(area.min.x, area.min.y + title_h), area.max);

        ui.allocate_new_ui(egui::UiBuilder::new().max_rect(title_rect), |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new("Wipe").strong());
                ui.label(
                    RichText::new(format!(
                        "{} | {}",
                        short_name(&file_name(left_v), 18),
                        short_name(&file_name(right_v), 18)
                    ))
                    .weak()
                    .size(11.0),
                );
                if ui.small_button("宫格").clicked() {
                    self.view_mode = CompareViewMode::Grid;
                }
            });
        });

        ui.painter()
            .rect_filled(content, 0.0, Color32::from_rgb(12, 12, 14));

        let split = split.clamp(0.05, 0.95);
        let split_x = content.min.x + content.width() * split;
        let left_clip = Rect::from_min_max(content.min, Pos2::new(split_x, content.max.y));
        let right_clip = Rect::from_min_max(Pos2::new(split_x, content.min.y), content.max);

        self.paint_video_frame_clipped(ctx, ui, service, player, left_v, content, left_clip);
        self.paint_video_frame_clipped(ctx, ui, service, player, right_v, content, right_clip);

        let handle = Rect::from_center_size(
            Pos2::new(split_x, content.center().y),
            Vec2::new(10.0, content.height()),
        );
        let handle_id = Id::new("compare_wipe_handle");
        let resp = ui.interact(handle, handle_id, Sense::drag());
        ui.painter().rect_filled(
            Rect::from_center_size(
                Pos2::new(split_x, content.center().y),
                Vec2::new(3.0, content.height()),
            ),
            0.0,
            Color32::from_rgba_unmultiplied(255, 255, 255, 200),
        );
        if resp.dragged() {
            if let Some(pos) = resp.interact_pointer_pos() {
                let new_split = ((pos.x - content.min.x) / content.width()).clamp(0.05, 0.95);
                self.view_mode = CompareViewMode::Wipe {
                    left: left_id,
                    right: right_id,
                    split: new_split,
                };
            }
        }
    }

    fn paint_video_frame_clipped(
        &mut self,
        ctx: &Context,
        ui: &mut Ui,
        service: &VideoReviewService,
        player: &ComparePlayer,
        video: &VideoItem,
        full_rect: Rect,
        clip: Rect,
    ) {
        if let Some(tex) = player.pane_texture(video.id) {
            paint_pane_tex(ui, full_rect, &tex, Color32::WHITE, Some(clip));
        } else if let Some(path) = service
            .frame_at(video, self.current_time_ms, 640)
            .ok()
            .flatten()
        {
            if let Some(tex) = self.load_texture(ctx, &path) {
                paint_tex(ui, full_rect, &tex, Color32::WHITE, Some(clip));
            }
        }
    }

    fn draw_overlay(
        &mut self,
        ctx: &Context,
        ui: &mut Ui,
        service: &VideoReviewService,
        player: &ComparePlayer,
        selected: &[&VideoItem],
        area: Rect,
        base_id: i64,
        top_id: i64,
        opacity: f32,
        diff: bool,
    ) {
        let Some(base_v) = selected.iter().find(|v| v.id == base_id).copied() else {
            return;
        };
        let Some(top_v) = selected.iter().find(|v| v.id == top_id).copied() else {
            return;
        };

        let title_h = TITLE_H_FOCUS;
        let title_rect = Rect::from_min_size(area.min, Vec2::new(area.width(), title_h));
        let content = Rect::from_min_max(Pos2::new(area.min.x, area.min.y + title_h), area.max);

        let mut opacity = opacity;
        let mut diff = diff;
        let mut back_grid = false;
        ui.allocate_new_ui(egui::UiBuilder::new().max_rect(title_rect), |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new("叠化").strong());
                ui.add(
                    egui::Slider::new(&mut opacity, 0.0..=1.0)
                        .text("透明")
                        .show_value(false),
                );
                ui.checkbox(&mut diff, "差分");
                if ui.small_button("宫格").clicked() {
                    back_grid = true;
                }
            });
        });
        if back_grid {
            self.view_mode = CompareViewMode::Grid;
            return;
        }
        self.view_mode = CompareViewMode::Overlay {
            base: base_id,
            top: top_id,
            opacity,
            diff,
        };

        ui.painter()
            .rect_filled(content, 0.0, Color32::from_rgb(12, 12, 14));

        if diff && !player.playing() {
            let key = (base_id, top_id, self.current_time_ms);
            let need = self.diff_key != Some(key)
                && self
                    .diff_last
                    .map(|t| t.elapsed().as_millis() >= DIFF_THROTTLE_MS)
                    .unwrap_or(true);
            if need || self.diff_tex.is_none() {
                if let Some(tex) = self.build_diff_texture(ctx, service, base_v, top_v) {
                    self.diff_tex = Some(tex);
                    self.diff_key = Some(key);
                    self.diff_last = Some(Instant::now());
                }
            }
            if let Some(tex) = &self.diff_tex {
                paint_tex(ui, content, tex, Color32::WHITE, None);
            }
        } else {
            self.paint_video_frame(ctx, ui, service, player, base_v, content);
            let tint = Color32::from_rgba_unmultiplied(
                255,
                255,
                255,
                (opacity.clamp(0.0, 1.0) * 255.0) as u8,
            );
            if let Some(tex) = player.pane_texture(top_id) {
                paint_pane_tex(ui, content, &tex, tint, None);
            } else if let Some(path) = service
                .frame_at(top_v, self.current_time_ms, 640)
                .ok()
                .flatten()
            {
                if let Some(tex) = self.load_texture(ctx, &path) {
                    paint_tex(ui, content, &tex, tint, None);
                }
            }
            if diff && player.playing() {
                let flash = (ctx.input(|i| i.time) * 2.0) as u64 % 2 == 0;
                if flash {
                    ui.painter().rect_filled(
                        content,
                        0.0,
                        Color32::from_rgba_unmultiplied(0, 0, 0, 100),
                    );
                    self.paint_video_frame(ctx, ui, service, player, top_v, content);
                }
                ctx.request_repaint();
            }
        }
    }

    fn build_diff_texture(
        &mut self,
        ctx: &Context,
        service: &VideoReviewService,
        a: &VideoItem,
        b: &VideoItem,
    ) -> Option<TextureHandle> {
        let pa = service
            .frame_at(a, self.current_time_ms, DIFF_MAX_W)
            .ok()??;
        let pb = service
            .frame_at(b, self.current_time_ms, DIFF_MAX_W)
            .ok()??;
        let ia = image::open(&pa).ok()?.to_rgba8();
        let ib = image::open(&pb).ok()?.to_rgba8();
        let w = ia.width().min(ib.width());
        let h = ia.height().min(ib.height());
        if w == 0 || h == 0 {
            return None;
        }
        let mut out = vec![0u8; (w * h * 4) as usize];
        for y in 0..h {
            for x in 0..w {
                let pa = ia.get_pixel(x, y).0;
                let pb = ib.get_pixel(x, y).0;
                let dr = pa[0].abs_diff(pb[0]);
                let dg = pa[1].abs_diff(pb[1]);
                let db = pa[2].abs_diff(pb[2]);
                let mag = ((dr as u16 + dg as u16 + db as u16) / 3).min(255) as u8;
                let i = ((y * w + x) * 4) as usize;
                out[i] = mag;
                out[i + 1] = mag / 2;
                out[i + 2] = 255u8.saturating_sub(mag);
                out[i + 3] = 255;
            }
        }
        let handle = ctx.load_texture(
            format!("compare_diff_{}_{}", a.id, b.id),
            ColorImage::from_rgba_unmultiplied([w as usize, h as usize], &out),
            egui::TextureOptions::LINEAR,
        );
        Some(handle)
    }

    fn load_texture(&mut self, ctx: &Context, path: &PathBuf) -> Option<TextureHandle> {
        let key = path.to_string_lossy().to_string();
        if let Some(t) = self.textures.get(&key) {
            return Some(t.clone());
        }
        let img = image::open(path).ok()?;
        let rgba = img.to_rgba8();
        let size = [rgba.width() as usize, rgba.height() as usize];
        let handle = ctx.load_texture(
            format!("video_frame_{key}"),
            ColorImage::from_rgba_unmultiplied(size, &rgba),
            egui::TextureOptions::LINEAR,
        );
        self.textures.insert(key, handle.clone());
        Some(handle)
    }
}

fn file_name(video: &VideoItem) -> String {
    video
        .file_path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| format!("视频 #{}", video.id))
}

fn short_name(name: &str, max: usize) -> String {
    let chars: Vec<char> = name.chars().collect();
    if chars.len() <= max {
        name.to_string()
    } else {
        let take = max.saturating_sub(1);
        format!("{}…", chars.into_iter().take(take).collect::<String>())
    }
}

fn paint_pane_tex(ui: &Ui, rect: Rect, tex: &PaneTexture<'_>, tint: Color32, clip: Option<Rect>) {
    match tex {
        PaneTexture::Cpu(handle) => paint_tex(ui, rect, handle, tint, clip),
        PaneTexture::Gpu(g) => {
            let scale = (rect.width() / g.size.x).min(rect.height() / g.size.y);
            let display = g.size * scale;
            let offset = rect.center() - display * 0.5;
            let img_rect = Rect::from_min_size(offset, display);
            let uv = if g.flip_y {
                Rect::from_min_max(egui::pos2(0.0, 1.0), egui::pos2(1.0, 0.0))
            } else {
                Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0))
            };
            let painter = match clip {
                Some(c) => ui.painter().with_clip_rect(c.intersect(ui.clip_rect())),
                None => ui.painter().clone(),
            };
            painter.image(g.id, img_rect, uv, tint);
        }
    }
}

fn paint_tex(ui: &Ui, rect: Rect, tex: &TextureHandle, tint: Color32, clip: Option<Rect>) {
    let tex_size = tex.size_vec2();
    let scale = (rect.width() / tex_size.x).min(rect.height() / tex_size.y);
    let display = tex_size * scale;
    let offset = rect.center() - display * 0.5;
    let img_rect = Rect::from_min_size(offset, display);
    let painter = match clip {
        Some(c) => ui.painter().with_clip_rect(c.intersect(ui.clip_rect())),
        None => ui.painter().clone(),
    };
    painter.image(
        tex.id(),
        img_rect,
        Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
        tint,
    );
}

fn paint_safe_frame(ui: &Ui, rect: Rect) {
    let stroke = Stroke::new(1.0, Color32::from_rgba_unmultiplied(255, 220, 80, 180));
    let stroke_inner = Stroke::new(1.0, Color32::from_rgba_unmultiplied(80, 200, 255, 140));
    let r90 = shrink_centered(rect, 0.9);
    let r80 = shrink_centered(rect, 0.8);
    ui.painter()
        .rect_stroke(r90, 0.0, stroke, egui::StrokeKind::Outside);
    ui.painter()
        .rect_stroke(r80, 0.0, stroke_inner, egui::StrokeKind::Outside);
    let c = rect.center();
    let arm = 12.0;
    ui.painter().line_segment(
        [egui::pos2(c.x - arm, c.y), egui::pos2(c.x + arm, c.y)],
        stroke,
    );
    ui.painter().line_segment(
        [egui::pos2(c.x, c.y - arm), egui::pos2(c.x, c.y + arm)],
        stroke,
    );
}

fn shrink_centered(rect: Rect, factor: f32) -> Rect {
    let size = rect.size() * factor;
    Rect::from_center_size(rect.center(), size)
}

pub fn format_ms(ms: u64) -> String {
    let total_secs = ms / 1000;
    let mins = total_secs / 60;
    let secs = total_secs % 60;
    let millis = ms % 1000;
    format!("{mins:02}:{secs:02}.{millis:03}")
}

fn format_ms_short(ms: u64) -> String {
    let total_secs = ms / 1000;
    let mins = total_secs / 60;
    let secs = total_secs % 60;
    format!("{mins:02}:{secs:02}")
}
