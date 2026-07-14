//! 双图/多图对比视图：分屏、卷帘、叠加等，基于 AnnotationCanvas。

use std::path::{Path, PathBuf};

use eframe::egui::{self, Context, RichText, Ui, Vec2};

use crate::gui::{theme, widgets};

use crate::review::domain::coords::ViewportTransform;
use crate::review::service::ImageLoadTier;
use crate::review::ui::annotation_canvas::{
    Annotation, AnnotationCanvas, AnnotationCanvasEvent, CanvasUiOptions,
};
use crate::review::ui::texture_cache::{CachedImage, ImageTextureCache};

const SPLITTER_SIZE: f32 = 6.0;
const MIN_PANE: f32 = 120.0;
const GRID_GAP: f32 = 6.0;
const CELL_LABEL_HEIGHT: f32 = 18.0;

/// 多图并排对比上限（原图之间对照，不含转换预览）。
pub const MAX_MULTI_COMPARE_PANES: usize = 6;

/// 显示模式：单图 / 分屏 / 卷帘 / 叠加 / 差异。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CompareDisplayMode {
    #[default]
    Single,
    Split,
    Wipe,
    Overlay,
    Diff,
    /// 切换对比：同位置叠加，快速在原图/转换后之间切换以发现像素级差异。
    Toggle,
    /// 多张原图横向并排（侧边栏多选或批量对比）。
    MultiSplit,
}

impl CompareDisplayMode {
    pub const ALL: [Self; 7] = [
        Self::Single,
        Self::Split,
        Self::Wipe,
        Self::Overlay,
        Self::Diff,
        Self::Toggle,
        Self::MultiSplit,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Single => "单图",
            Self::Split => "并排",
            Self::Wipe => "卷帘",
            Self::Overlay => "叠加",
            Self::Diff => "差异",
            Self::Toggle => "切换",
            Self::MultiSplit => "多图对比",
        }
    }

    pub fn is_compare(self) -> bool {
        !matches!(self, Self::Single)
    }

    pub fn needs_selection(self) -> bool {
        matches!(self, Self::MultiSplit)
    }
}

/// 分屏布局方向。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SplitLayout {
    #[default]
    Horizontal,
    Vertical,
}

/// 同步联动配置。
#[derive(Debug, Clone)]
pub struct CompareViewConfig {
    /// 左侧操作同步到右侧（默认开启）。
    pub sync_viewport: bool,
    /// 右侧独立操作时反向同步到左侧（默认关闭）。
    pub reverse_sync: bool,
    pub layout: SplitLayout,
}

impl Default for CompareViewConfig {
    fn default() -> Self {
        Self {
            sync_viewport: true,
            reverse_sync: false,
            layout: SplitLayout::Horizontal,
        }
    }
}

/// 视口同步来源（用于判断本帧哪侧主导）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum ViewportSource {
    #[default]
    None,
    Left,
    Right,
    Extra(usize),
}

/// 双图对比组件：对外统一 `ui()`，内部管理双 AnnotationCanvas。
pub struct CompareView {
    pub mode: CompareDisplayMode,
    pub config: CompareViewConfig,
    /// 分屏比例 0.2~0.8（左/上侧占比）。
    pub split_ratio: f32,
    /// 卷帘分割线 0~1。
    pub wipe_ratio: f32,
    /// 叠加模式透明度 0~1。
    pub overlay_alpha: f32,
    /// 局部放大镜开关。
    pub magnifier_enabled: bool,
    /// 切换对比：当前是否显示转换后图（false=原图）。
    pub toggle_show_converted: bool,
    /// 切换对比：自动切换开关。
    pub toggle_auto: bool,
    /// 切换对比：自动切换间隔（秒，0.2~2.0）。
    pub toggle_interval: f32,
    /// 上次自动切换时间戳（秒）。
    toggle_last_flip: f64,
    left: AnnotationCanvas,
    right: AnnotationCanvas,
    extra_panes: Vec<AnnotationCanvas>,
    /// 多图并排模式下的 pane 数量（含主 pane）。
    multi_pane_count: usize,
    textures: ImageTextureCache,
    last_viewport_source: ViewportSource,
    last_left_viewport: ViewportTransform,
    last_right_viewport: ViewportTransform,
    /// 窗口缩放期间暂缓纹理升级，避免最大化时主线程卡顿。
    defer_texture_load: bool,
}

impl Default for CompareView {
    fn default() -> Self {
        Self::new()
    }
}

impl CompareView {
    pub fn new() -> Self {
        Self {
            mode: CompareDisplayMode::Single,
            config: CompareViewConfig::default(),
            split_ratio: 0.5,
            wipe_ratio: 0.5,
            overlay_alpha: 0.5,
            magnifier_enabled: false,
            toggle_show_converted: false,
            toggle_auto: false,
            toggle_interval: 0.5,
            toggle_last_flip: 0.0,
            left: AnnotationCanvas::new(),
            right: AnnotationCanvas::new(),
            extra_panes: Vec::new(),
            multi_pane_count: 0,
            textures: ImageTextureCache::default(),
            last_viewport_source: ViewportSource::None,
            last_left_viewport: ViewportTransform::fit_image(
                crate::review::domain::coords::Vec2::new(1.0, 1.0),
                (1, 1),
            ),
            last_right_viewport: ViewportTransform::fit_image(
                crate::review::domain::coords::Vec2::new(1.0, 1.0),
                (1, 1),
            ),
            defer_texture_load: false,
        }
    }

    pub fn set_defer_texture_load(&mut self, defer: bool) {
        self.defer_texture_load = defer;
    }

    /// 循环切换对比模式：并排 → 卷帘 → 切换 → 并排。
    pub fn cycle_compare_mode(&mut self) {
        self.mode = match self.mode {
            CompareDisplayMode::Split => CompareDisplayMode::Wipe,
            CompareDisplayMode::Wipe => CompareDisplayMode::Toggle,
            CompareDisplayMode::Toggle => CompareDisplayMode::Split,
            _ => CompareDisplayMode::Split,
        };
    }

    pub fn mode_label(&self) -> &'static str {
        self.mode.label()
    }

    pub fn is_compare_active(&self) -> bool {
        self.mode.is_compare()
    }

    /// 主工具栏：下拉选择对比模式。
    pub fn mode_selector_ui(&mut self, ui: &mut Ui) {
        widgets::toolbar_combo_box(ui, "review_compare_mode", self.mode.label(), 120.0, |ui| {
            for mode in CompareDisplayMode::ALL {
                ui.selectable_value(&mut self.mode, mode, mode.label());
            }
        });
    }

    /// 当前窗口宽度下最多可并排的张数（每张至少 [`MIN_PANE`] 宽）。
    pub fn max_panes_for_width(width: f32) -> usize {
        if width < MIN_PANE {
            return 1;
        }
        ((width + SPLITTER_SIZE) / (MIN_PANE + SPLITTER_SIZE))
            .floor()
            .clamp(1.0, MAX_MULTI_COMPARE_PANES as f32) as usize
    }

    /// 对比模式下的状态条（画布上方醒目提示）。
    pub fn mode_status_strip(&self, ui: &mut Ui) {
        let dark = ui.style().visuals.dark_mode;
        let accent = theme::accent(dark);
        let detail = match self.mode {
            CompareDisplayMode::Toggle => {
                let showing = if self.toggle_show_converted {
                    "转换后"
                } else {
                    "原图"
                };
                format!("{} · 当前显示 {showing} · 空格键翻转", self.mode.label())
            }
            CompareDisplayMode::Split => format!(
                "{} · {}分屏",
                self.mode.label(),
                if self.config.layout == SplitLayout::Horizontal {
                    "左右"
                } else {
                    "上下"
                }
            ),
            CompareDisplayMode::MultiSplit => {
                if self.multi_pane_count <= 2 {
                    format!(
                        "{} · {} 张 · 左右并排",
                        self.mode.label(),
                        self.multi_pane_count
                    )
                } else {
                    format!(
                        "{} · {} 张 · 宫格布局",
                        self.mode.label(),
                        self.multi_pane_count
                    )
                }
            }
            _ => format!("{} · 原图与转换后对照", self.mode.label()),
        };

        egui::Frame::new()
            .fill(accent.linear_multiply(0.16))
            .stroke(egui::Stroke::new(1.0, accent.linear_multiply(0.55)))
            .corner_radius(egui::CornerRadius::same(theme::CONTROL_RADIUS))
            .inner_margin(egui::Margin::symmetric(12, 6))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("对比中").size(13.0).strong().color(accent));
                    ui.separator();
                    ui.label(
                        RichText::new(detail)
                            .size(13.0)
                            .color(theme::primary_label(dark)),
                    );
                });
            });
        ui.add_space(6.0);
    }

    /// 切换对比：手动翻转显示原图/转换后。
    pub fn toggle_flip(&mut self) {
        if self.mode == CompareDisplayMode::Toggle {
            self.toggle_show_converted = !self.toggle_show_converted;
        }
    }

    /// 在左侧画布定位到指定标注（同步视口可选）。
    pub fn focus_annotation(&mut self, ann: &Annotation) {
        self.left.focus_on_annotation(ann);
        if self.config.sync_viewport {
            self.sync_right_from_left();
        }
    }

    /// 左侧可编辑画布（工具栏样式等）。
    pub fn left_canvas(&self) -> &AnnotationCanvas {
        &self.left
    }

    pub fn left_canvas_mut(&mut self) -> &mut AnnotationCanvas {
        &mut self.left
    }

    /// 渲染标注工具栏（仅作用于左侧可编辑画布）。
    pub fn tools_ui(&mut self, ui: &mut Ui) -> Vec<AnnotationCanvasEvent> {
        let mut events = Vec::new();
        self.left.toolbar_ui(ui, &mut events);
        ui.add_space(4.0);
        events
    }

    /// 适配窗口：同时作用于可见画布。
    pub fn fit_to_window(&mut self, canvas_size: Vec2) {
        match self.mode {
            CompareDisplayMode::Single => {
                self.left.fit_to_window(canvas_size);
                self.sync_right_from_left();
            }
            CompareDisplayMode::Split => {
                let (left_size, right_size) = self.pane_sizes(canvas_size);
                self.left.fit_to_window(left_size);
                self.right.fit_to_window(right_size);
                if self.config.sync_viewport {
                    self.sync_right_from_left();
                }
            }
            CompareDisplayMode::Wipe
            | CompareDisplayMode::Overlay
            | CompareDisplayMode::Diff
            | CompareDisplayMode::Toggle => {
                self.left.fit_to_window(canvas_size);
            }
            CompareDisplayMode::MultiSplit => {
                self.fit_multi_panes(canvas_size, |pane, size| pane.fit_to_window(size));
            }
        }
    }

    pub fn set_zoom_100(&mut self, canvas_size: Vec2) {
        match self.mode {
            CompareDisplayMode::Single => {
                self.left.set_zoom_100(canvas_size);
                self.sync_right_from_left();
            }
            CompareDisplayMode::Split => {
                let (left_size, right_size) = self.pane_sizes(canvas_size);
                self.left.set_zoom_100(left_size);
                self.right.set_zoom_100(right_size);
                if self.config.sync_viewport {
                    self.sync_right_from_left();
                }
            }
            CompareDisplayMode::MultiSplit => {
                self.fit_multi_panes(canvas_size, |pane, size| pane.set_zoom_100(size));
            }
            _ => {
                self.left.set_zoom_100(canvas_size);
            }
        }
    }

    fn fit_multi_panes(&mut self, canvas_size: Vec2, apply: impl Fn(&mut AnnotationCanvas, Vec2)) {
        let n = self.multi_pane_count.max(2);
        let Some(pane_size) = multi_pane_size(n, canvas_size) else {
            return;
        };
        apply(&mut self.left, pane_size);
        for pane in self.extra_panes.iter_mut().take(n.saturating_sub(1)) {
            apply(pane, pane_size);
        }
        if self.config.sync_viewport {
            self.sync_extra_panes_from_left();
        }
    }

    pub fn prefetch_neighbors(
        &self,
        paths: &[std::path::PathBuf],
        center: usize,
        radius: usize,
        thumb_paths: &[Option<std::path::PathBuf>],
    ) {
        self.textures
            .loader()
            .prefetch_neighbors(paths, center, radius, thumb_paths);
    }

    pub fn prefetch_multi_thumbs(&mut self, items: &[(PathBuf, Option<PathBuf>)]) {
        self.textures.prefetch_thumbs(items);
    }

    /// 统一入口：模式切换栏 + 画布区域，返回左侧标注事件。
    pub fn ui(
        &mut self,
        ui: &mut Ui,
        ctx: &Context,
        original_path: &Path,
        converted_path: Option<&Path>,
        thumb_path: Option<&Path>,
        annotations: &[Annotation],
    ) -> Vec<AnnotationCanvasEvent> {
        let canvas_size = ui.available_size();
        if self.mode.is_compare() {
            self.mode_status_strip(ui);
        }
        self.context_controls_ui(ui, canvas_size);

        self.textures
            .request(original_path, thumb_path, ImageLoadTier::Thumb);
        if let Some(p) = converted_path {
            self.textures.request(p, None, ImageLoadTier::Thumb);
        }
        let mut textures_pending = self.textures.poll(ctx);

        let original_entry = self.textures.get(original_path).cloned();
        let Some(original) = original_entry else {
            if crate::review::service::is_non_filesystem_path(original_path) {
                // 尝试先显示本地缩略图
                if let Some(thumb) = thumb_path.filter(|p| p.exists()) {
                    self.textures.request(thumb, None, ImageLoadTier::Thumb);
                    let _ = self.textures.poll(ctx);
                    if let Some(preview) = self.textures.get(thumb).cloned() {
                        self.left.set_image_size(preview.size);
                        ui.colored_label(
                            theme::warning_color(ui.style().visuals.dark_mode),
                            "原图下载中…（当前显示缩略图）",
                        );
                        ctx.request_repaint_after(std::time::Duration::from_millis(200));
                        // 继续用缩略图路径渲染会很复杂；这里先提示并等待
                        return Vec::new();
                    }
                }
                ui.colored_label(
                    theme::warning_color(ui.style().visuals.dark_mode),
                    "正在下载原图…",
                );
            } else if let Some(err) = self.textures.load_error(original_path) {
                ui.colored_label(
                    theme::error_color(ui.style().visuals.dark_mode),
                    format!("无法打开图片：{err}"),
                );
                ui.label(
                    RichText::new(original_path.display().to_string())
                        .weak()
                        .size(11.0),
                );
            } else {
                ui.colored_label(
                    theme::error_color(ui.style().visuals.dark_mode),
                    "正在加载原图…",
                );
                ctx.request_repaint_after(std::time::Duration::from_millis(100));
            }
            return Vec::new();
        };
        self.left.set_image_size(original.size);
        let max_tier = if self.mode == CompareDisplayMode::Single {
            ImageLoadTier::Full
        } else {
            ImageLoadTier::Preview
        };
        if !self.defer_texture_load {
            self.textures.maybe_upgrade(
                original_path,
                thumb_path,
                self.left.viewport(),
                original.size,
                max_tier,
            );
        }

        let converted_entry = converted_path.and_then(|p| self.textures.get(p).cloned());
        if converted_entry.is_none() {
            if let Some(p) = converted_path {
                self.textures.request(p, None, ImageLoadTier::Preview);
                textures_pending |= self.textures.poll(ctx);
            }
        }
        let converted_entry = converted_path.and_then(|p| self.textures.get(p).cloned());
        if let Some(ref c) = converted_entry {
            self.right.set_image_size(c.size);
        }

        let events = match self.mode {
            CompareDisplayMode::Single => self.render_single(ui, &original.texture, annotations),
            CompareDisplayMode::Split => self.render_split(
                ui,
                &original.texture,
                converted_entry.as_ref().map(|c| &c.texture),
                annotations,
            ),
            CompareDisplayMode::Wipe => self.render_wipe(
                ui,
                &original.texture,
                converted_entry.as_ref().map(|c| &c.texture),
                annotations,
            ),
            CompareDisplayMode::Overlay => self.render_overlay(
                ui,
                &original.texture,
                converted_entry.as_ref().map(|c| &c.texture),
                annotations,
            ),
            CompareDisplayMode::Diff => self.render_diff(
                ui,
                &original.texture,
                converted_entry.as_ref().map(|c| &c.texture),
                annotations,
            ),
            CompareDisplayMode::Toggle => self.render_toggle(
                ui,
                ctx,
                &original.texture,
                converted_entry.as_ref().map(|c| &c.texture),
                annotations,
            ),
            CompareDisplayMode::MultiSplit => Vec::new(),
        };

        // 状态变更：根据本帧视口变化执行同步
        self.apply_viewport_sync();
        if textures_pending {
            ctx.request_repaint_after(std::time::Duration::from_millis(
                if self.defer_texture_load { 80 } else { 32 },
            ));
        }
        events
    }

    /// 多图原图并排对比（仅原图，不含转换预览）。
    pub fn ui_multi(
        &mut self,
        ui: &mut Ui,
        ctx: &Context,
        sources: &[(PathBuf, Option<PathBuf>, String)],
    ) -> Vec<AnnotationCanvasEvent> {
        self.multi_pane_count = sources.len();
        let canvas_size = ui.available_size();
        if self.mode.is_compare() {
            self.mode_status_strip(ui);
        }
        self.context_controls_ui(ui, canvas_size);

        self.textures.ensure_capacity(sources.len() + 8);
        for (path, thumb, _) in sources {
            self.textures
                .request(path, thumb.as_deref(), ImageLoadTier::Thumb);
        }
        let mut textures_pending = self.textures.poll(ctx);

        let mut loaded: Vec<(CachedImage, String)> = Vec::with_capacity(sources.len());
        let mut missing = 0usize;
        for (path, thumb, label) in sources {
            if let Some(entry) = self.textures.get(path).cloned() {
                loaded.push((entry, label.clone()));
                let _ = (path, thumb);
            } else {
                missing += 1;
            }
        }

        if loaded.len() < 2 {
            ui.colored_label(
                theme::error_color(ui.style().visuals.dark_mode),
                format!("正在加载原图… ({}/{})", loaded.len(), sources.len()),
            );
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
            return Vec::new();
        }

        if missing > 0 {
            ui.label(
                RichText::new(format!(
                    "已显示 {} 张，其余 {missing} 张加载中…",
                    loaded.len()
                ))
                .size(12.0)
                .color(theme::secondary_label(ui.style().visuals.dark_mode)),
            );
            textures_pending = true;
        }

        self.ensure_extra_panes(loaded.len().saturating_sub(1));
        if let Some((entry, _)) = loaded.first() {
            self.left.set_image_size(entry.size);
        }
        for (idx, (entry, _)) in loaded.iter().enumerate().skip(1) {
            self.extra_panes[idx - 1].set_image_size(entry.size);
        }

        let events = self.render_multi_compare(ui, &loaded);
        self.apply_viewport_sync();
        if textures_pending || missing > 0 {
            ctx.request_repaint_after(std::time::Duration::from_millis(
                if self.defer_texture_load { 100 } else { 50 },
            ));
        }
        events
    }

    fn ensure_extra_panes(&mut self, extra: usize) {
        while self.extra_panes.len() < extra {
            self.extra_panes.push(AnnotationCanvas::new());
        }
    }

    fn pane_mut(&mut self, index: usize) -> &mut AnnotationCanvas {
        if index == 0 {
            &mut self.left
        } else {
            &mut self.extra_panes[index - 1]
        }
    }

    fn sync_extra_panes_from_left(&mut self) {
        let vp = self.left.viewport();
        for pane in self
            .extra_panes
            .iter_mut()
            .take(self.multi_pane_count.saturating_sub(1))
        {
            pane.set_viewport(vp);
        }
    }

    /// 当前模式下的画布辅助控件（模式选择在主工具栏）。
    fn context_controls_ui(&mut self, ui: &mut Ui, canvas_size: Vec2) {
        let dark = ui.style().visuals.dark_mode;

        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = egui::vec2(6.0, 6.0);

            if self.mode == CompareDisplayMode::Toggle {
                let showing = if self.toggle_show_converted {
                    "转换后"
                } else {
                    "原图"
                };
                ui.label(
                    RichText::new(format!("当前 {showing}"))
                        .size(13.0)
                        .strong()
                        .color(theme::accent(dark)),
                );
                if widgets::compact_secondary_button(ui, "翻转 (空格)", true).clicked() {
                    self.toggle_flip();
                }
                ui.checkbox(&mut self.toggle_auto, "自动切换");
                if self.toggle_auto {
                    ui.add(
                        egui::Slider::new(&mut self.toggle_interval, 0.2..=2.0)
                            .suffix("s")
                            .text("间隔"),
                    );
                }
                widgets::toolbar_separator(ui);
            }

            if matches!(
                self.mode,
                CompareDisplayMode::Overlay | CompareDisplayMode::Wipe
            ) {
                ui.add(
                    egui::Slider::new(
                        if self.mode == CompareDisplayMode::Wipe {
                            &mut self.wipe_ratio
                        } else {
                            &mut self.overlay_alpha
                        },
                        0.05..=0.95,
                    )
                    .text(if self.mode == CompareDisplayMode::Wipe {
                        "卷帘"
                    } else {
                        "透明度"
                    }),
                );
                widgets::toolbar_separator(ui);
            }

            if self.mode == CompareDisplayMode::Split {
                if widgets::toggle_chip(
                    ui,
                    "左右",
                    self.config.layout == SplitLayout::Horizontal,
                    true,
                ) {
                    self.config.layout = SplitLayout::Horizontal;
                }
                if widgets::toggle_chip(
                    ui,
                    "上下",
                    self.config.layout == SplitLayout::Vertical,
                    true,
                ) {
                    self.config.layout = SplitLayout::Vertical;
                }
                ui.checkbox(&mut self.config.reverse_sync, "右侧反向同步");
                widgets::toolbar_separator(ui);
            }

            if self.mode == CompareDisplayMode::MultiSplit {
                ui.label(
                    RichText::new("2 张左右并排 · 3 张及以上宫格 · 标注请切回单图")
                        .size(12.0)
                        .color(theme::secondary_label(dark)),
                );
                widgets::toolbar_separator(ui);
            }

            ui.checkbox(&mut self.magnifier_enabled, "放大镜");
            ui.checkbox(&mut self.config.sync_viewport, "同步缩放平移");

            if self.mode.is_compare() {
                widgets::toolbar_separator(ui);
                if widgets::compact_secondary_button(ui, "适应窗口", true).clicked() {
                    self.fit_to_window(canvas_size);
                }
                if widgets::compact_secondary_button(ui, "100%", true).clicked() {
                    self.set_zoom_100(canvas_size);
                }
            }
        });

        if self.mode.is_compare()
            || self.magnifier_enabled
            || !self.config.sync_viewport
            || (self.mode == CompareDisplayMode::Toggle && self.toggle_auto)
        {
            ui.add_space(6.0);
        }
    }

    fn render_single(
        &mut self,
        ui: &mut Ui,
        texture: &egui::TextureHandle,
        annotations: &[Annotation],
    ) -> Vec<AnnotationCanvasEvent> {
        ui.label(egui::RichText::new("原图").strong());
        let before = self.left.viewport();
        let events = self.left.ui_with_options(
            ui,
            texture,
            annotations,
            CanvasUiOptions {
                show_toolbar: false,
                ..CanvasUiOptions::default()
            },
        );
        if self.left.viewport() != before {
            self.last_viewport_source = ViewportSource::Left;
        }
        events
    }

    fn render_split(
        &mut self,
        ui: &mut Ui,
        left_texture: &egui::TextureHandle,
        right_texture: Option<&egui::TextureHandle>,
        annotations: &[Annotation],
    ) -> Vec<AnnotationCanvasEvent> {
        let total = ui.available_size();
        let mut left_events = Vec::new();

        let ratio = self.split_ratio.clamp(0.2, 0.8);

        match self.config.layout {
            SplitLayout::Horizontal => {
                let left_w = (total.x * ratio).clamp(MIN_PANE, total.x - MIN_PANE - SPLITTER_SIZE);
                let right_w = total.x - left_w - SPLITTER_SIZE;

                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        ui.set_width(left_w);
                        ui.label(egui::RichText::new("原图").strong());
                        let before = self.left.viewport();
                        left_events.extend(self.left.ui_with_options(
                            ui,
                            left_texture,
                            annotations,
                            CanvasUiOptions {
                                show_toolbar: false,
                                ..CanvasUiOptions::default()
                            },
                        ));
                        if self.left.viewport() != before {
                            self.last_viewport_source = ViewportSource::Left;
                        }
                    });

                    let sep = ui.add_sized(
                        egui::vec2(SPLITTER_SIZE, total.y),
                        egui::Separator::default().spacing(0.0),
                    );
                    if sep.dragged() {
                        self.split_ratio += sep.drag_delta().x / total.x;
                        self.split_ratio = self.split_ratio.clamp(0.2, 0.8);
                    }
                    if sep.hovered() || sep.dragged() {
                        ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
                    }

                    ui.vertical(|ui| {
                        ui.set_width(right_w);
                        ui.label(egui::RichText::new("转换后").strong());
                        self.render_right_pane(ui, right_texture, Vec2::new(right_w, total.y));
                    });
                });
            }
            SplitLayout::Vertical => {
                let top_h = (total.y * ratio).clamp(MIN_PANE, total.y - MIN_PANE - SPLITTER_SIZE);
                let bottom_h = total.y - top_h - SPLITTER_SIZE;

                ui.vertical(|ui| {
                    ui.label(egui::RichText::new("原图").strong());
                    let before = self.left.viewport();
                    left_events.extend(self.left.ui_with_options(
                        ui,
                        left_texture,
                        annotations,
                        CanvasUiOptions {
                            show_toolbar: false,
                            ..CanvasUiOptions::default()
                        },
                    ));
                    if self.left.viewport() != before {
                        self.last_viewport_source = ViewportSource::Left;
                    }

                    let sep = ui.add_sized(
                        egui::vec2(total.x, SPLITTER_SIZE),
                        egui::Separator::default().spacing(0.0),
                    );
                    if sep.dragged() {
                        self.split_ratio += sep.drag_delta().y / total.y;
                        self.split_ratio = self.split_ratio.clamp(0.2, 0.8);
                    }
                    if sep.hovered() || sep.dragged() {
                        ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeVertical);
                    }

                    ui.label(egui::RichText::new("转换后").strong());
                    self.render_right_pane(ui, right_texture, Vec2::new(total.x, bottom_h));
                });
            }
        }

        left_events
    }

    fn render_multi_compare(
        &mut self,
        ui: &mut Ui,
        loaded: &[(CachedImage, String)],
    ) -> Vec<AnnotationCanvasEvent> {
        if loaded.len() <= 2 {
            self.render_multi_horizontal(ui, loaded)
        } else {
            self.render_multi_grid(ui, loaded)
        }
    }

    fn multi_read_only_options(&self) -> CanvasUiOptions {
        if self.config.sync_viewport {
            CanvasUiOptions::READ_ONLY_PREVIEW
        } else {
            CanvasUiOptions::READ_ONLY_PAN
        }
    }

    fn render_multi_pane_cell(
        &mut self,
        ui: &mut Ui,
        index: usize,
        entry: &CachedImage,
        label: &str,
        pane_size: Vec2,
        read_only: CanvasUiOptions,
    ) {
        ui.vertical(|ui| {
            ui.set_width(pane_size.x);
            ui.label(RichText::new(label).strong().size(12.0));
            ui.allocate_ui_with_layout(
                egui::vec2(pane_size.x, pane_size.y),
                egui::Layout::top_down(egui::Align::Center),
                |ui| {
                    let pane = self.pane_mut(index);
                    let before = pane.viewport();
                    pane.ui_with_options(ui, &entry.texture, &[], read_only);
                    if pane.viewport() != before {
                        self.last_viewport_source = if index == 0 {
                            ViewportSource::Left
                        } else {
                            ViewportSource::Extra(index - 1)
                        };
                    }
                },
            );
        });
    }

    fn render_multi_horizontal(
        &mut self,
        ui: &mut Ui,
        loaded: &[(CachedImage, String)],
    ) -> Vec<AnnotationCanvasEvent> {
        let n = loaded.len();
        let total = ui.available_size();
        let splitters = SPLITTER_SIZE * (n - 1) as f32;
        let pane_w = (total.x - splitters) / n as f32;

        if pane_w < MIN_PANE {
            widgets::error_banner(
                ui,
                &format!(
                    "当前宽度不足以并排 {n} 张（每张至少 {}px），请减少选择或加宽窗口",
                    MIN_PANE as i32
                ),
            );
            return Vec::new();
        }

        let pane_size = Vec2::new(pane_w, total.y - CELL_LABEL_HEIGHT);
        let read_only = self.multi_read_only_options();

        ui.horizontal(|ui| {
            for (i, (entry, label)) in loaded.iter().enumerate() {
                if i > 0 {
                    ui.add_sized(
                        egui::vec2(SPLITTER_SIZE, total.y),
                        egui::Separator::default().spacing(0.0),
                    );
                }
                self.render_multi_pane_cell(ui, i, entry, label, pane_size, read_only);
            }
        });
        Vec::new()
    }

    fn render_multi_grid(
        &mut self,
        ui: &mut Ui,
        loaded: &[(CachedImage, String)],
    ) -> Vec<AnnotationCanvasEvent> {
        let n = loaded.len();
        let total = ui.available_size();
        let Some((cols, rows, pane_size)) = multi_grid_layout(n, total) else {
            widgets::error_banner(
                ui,
                &format!(
                    "当前区域不足以宫格显示 {n} 张（每格至少 {}px），请减少选择或放大窗口",
                    MIN_PANE as i32
                ),
            );
            return Vec::new();
        };

        let read_only = self.multi_read_only_options();
        let row_height = pane_size.y + CELL_LABEL_HEIGHT;

        ui.vertical(|ui| {
            for row in 0..rows {
                if row > 0 {
                    ui.add_space(GRID_GAP);
                }
                ui.horizontal(|ui| {
                    for col in 0..cols {
                        let index = row * cols + col;
                        if col > 0 {
                            ui.add_space(GRID_GAP);
                        }
                        if index >= n {
                            ui.allocate_space(egui::vec2(pane_size.x, row_height));
                            continue;
                        }
                        let (entry, label) = &loaded[index];
                        self.render_multi_pane_cell(ui, index, entry, label, pane_size, read_only);
                    }
                });
            }
        });
        Vec::new()
    }

    fn render_right_pane(
        &mut self,
        ui: &mut Ui,
        texture: Option<&egui::TextureHandle>,
        pane_size: Vec2,
    ) {
        let Some(tex) = texture else {
            ui.allocate_ui_with_layout(
                pane_size,
                egui::Layout::centered_and_justified(egui::Direction::TopDown),
                |ui| {
                    ui.label("暂无转换预览（请先完成格式转换）");
                },
            );
            return;
        };

        let right_options = if self.config.sync_viewport {
            CanvasUiOptions::READ_ONLY_PREVIEW
        } else {
            CanvasUiOptions::READ_ONLY_PAN
        };

        let before = self.right.viewport();
        self.right.ui_with_options(ui, tex, &[], right_options);
        if self.right.viewport() != before {
            self.last_viewport_source = ViewportSource::Right;
        }
    }

    fn apply_viewport_sync(&mut self) {
        if self.mode == CompareDisplayMode::MultiSplit {
            if self.config.sync_viewport {
                match self.last_viewport_source {
                    ViewportSource::Left => self.sync_extra_panes_from_left(),
                    _ => {}
                }
            }
        } else {
            match self.last_viewport_source {
                ViewportSource::Left => {
                    if self.config.sync_viewport {
                        self.sync_right_from_left();
                    }
                }
                ViewportSource::Right => {
                    if self.config.reverse_sync {
                        self.left.set_viewport(self.right.viewport());
                    }
                }
                ViewportSource::Extra(_) | ViewportSource::None => {}
            }
        }
        self.last_left_viewport = self.left.viewport();
        self.last_right_viewport = self.right.viewport();
        self.last_viewport_source = ViewportSource::None;
    }

    fn sync_right_from_left(&mut self) {
        self.right.set_viewport(self.left.viewport());
    }

    fn pane_sizes(&self, total: Vec2) -> (Vec2, Vec2) {
        let ratio = self.split_ratio.clamp(0.2, 0.8);
        match self.config.layout {
            SplitLayout::Horizontal => {
                let lw = total.x * ratio;
                let rw = total.x - lw - SPLITTER_SIZE;
                (Vec2::new(lw, total.y), Vec2::new(rw, total.y))
            }
            SplitLayout::Vertical => {
                let th = total.y * ratio;
                let bh = total.y - th - SPLITTER_SIZE;
                (Vec2::new(total.x, th), Vec2::new(total.x, bh))
            }
        }
    }

    fn render_wipe(
        &mut self,
        ui: &mut Ui,
        left_texture: &egui::TextureHandle,
        right_texture: Option<&egui::TextureHandle>,
        annotations: &[Annotation],
    ) -> Vec<AnnotationCanvasEvent> {
        let Some(right) = right_texture else {
            return self.render_single(ui, left_texture, annotations);
        };
        let available = ui.available_size();
        ui.horizontal(|ui| {
            let left_w = available.x * self.wipe_ratio.clamp(0.05, 0.95);
            ui.set_width(left_w);
            self.render_single(ui, left_texture, annotations);
            let sep = ui.add_sized(
                egui::vec2(SPLITTER_SIZE, available.y),
                egui::Separator::default().spacing(0.0),
            );
            if sep.dragged() {
                self.wipe_ratio += sep.drag_delta().x / available.x;
                self.wipe_ratio = self.wipe_ratio.clamp(0.05, 0.95);
            }
            ui.set_width(available.x - left_w - SPLITTER_SIZE);
            self.render_right_pane(
                ui,
                Some(right),
                Vec2::new(ui.available_width(), available.y),
            );
        });
        Vec::new()
    }

    fn render_overlay(
        &mut self,
        ui: &mut Ui,
        left_texture: &egui::TextureHandle,
        right_texture: Option<&egui::TextureHandle>,
        annotations: &[Annotation],
    ) -> Vec<AnnotationCanvasEvent> {
        let events = self.render_single(ui, left_texture, annotations);
        if let Some(right) = right_texture {
            let rect = ui.min_rect();
            let alpha = (self.overlay_alpha * 255.0) as u8;
            ui.painter().image(
                right.id(),
                rect,
                egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                egui::Color32::from_white_alpha(alpha),
            );
        }
        events
    }

    fn render_diff(
        &mut self,
        ui: &mut Ui,
        left_texture: &egui::TextureHandle,
        right_texture: Option<&egui::TextureHandle>,
        annotations: &[Annotation],
    ) -> Vec<AnnotationCanvasEvent> {
        let events = self.render_split(ui, left_texture, right_texture, annotations);
        if right_texture.is_none() {
            widgets::error_banner(ui, "无转换预览，无法高亮差异");
        } else {
            ui.label(
                RichText::new("差异模式：左右对照，缺失预览区域以红色提示")
                    .size(12.0)
                    .color(theme::secondary_label(ui.style().visuals.dark_mode)),
            );
        }
        events
    }

    fn render_toggle(
        &mut self,
        ui: &mut Ui,
        ctx: &Context,
        left_texture: &egui::TextureHandle,
        right_texture: Option<&egui::TextureHandle>,
        annotations: &[Annotation],
    ) -> Vec<AnnotationCanvasEvent> {
        // 自动切换：按间隔翻转，视口保持不变
        if self.toggle_auto && right_texture.is_some() {
            let now = ctx.input(|i| i.time);
            let interval = self.toggle_interval.clamp(0.2, 2.0) as f64;
            if now - self.toggle_last_flip >= interval {
                self.toggle_show_converted = !self.toggle_show_converted;
                self.toggle_last_flip = now;
            }
            ctx.request_repaint_after(std::time::Duration::from_secs_f32(
                self.toggle_interval.clamp(0.2, 2.0),
            ));
        }

        let show_converted = self.toggle_show_converted && right_texture.is_some();
        let label = if show_converted {
            "转换后"
        } else {
            "原图"
        };
        ui.label(egui::RichText::new(label).strong());

        // 始终用左画布的视口渲染，确保切换过程中缩放/平移一致
        let texture = if show_converted {
            right_texture.unwrap_or(left_texture)
        } else {
            left_texture
        };
        let before = self.left.viewport();
        let events = self.left.ui_with_options(
            ui,
            texture,
            annotations,
            CanvasUiOptions {
                show_toolbar: false,
                ..CanvasUiOptions::default()
            },
        );
        if self.left.viewport() != before {
            self.last_viewport_source = ViewportSource::Left;
        }
        if right_texture.is_none() {
            widgets::error_banner(ui, "无转换预览，无法切换对比");
        }
        events
    }
}

fn multi_pane_size(n: usize, canvas_size: Vec2) -> Option<Vec2> {
    if n <= 2 {
        let splitters = SPLITTER_SIZE * (n - 1) as f32;
        let pane_w = (canvas_size.x - splitters) / n as f32;
        if pane_w < MIN_PANE {
            return None;
        }
        Some(Vec2::new(pane_w, canvas_size.y - CELL_LABEL_HEIGHT))
    } else {
        multi_grid_layout(n, canvas_size).map(|(_, _, size)| size)
    }
}

/// 计算宫格列/行与单格画布尺寸（不含标题行）。
fn multi_grid_layout(n: usize, total: Vec2) -> Option<(usize, usize, Vec2)> {
    let mut best: Option<(usize, usize, Vec2)> = None;
    for cols in 2..=3.min(n) {
        let rows = n.div_ceil(cols);
        let cell_w = (total.x - (cols - 1) as f32 * GRID_GAP) / cols as f32;
        let cell_h = (total.y - (rows - 1) as f32 * GRID_GAP) / rows as f32 - CELL_LABEL_HEIGHT;
        if cell_w < MIN_PANE || cell_h < MIN_PANE {
            continue;
        }
        let size = Vec2::new(cell_w, cell_h);
        let score = cell_w.min(cell_h);
        if best
            .as_ref()
            .is_none_or(|(_, _, prev)| score > prev.x.min(prev.y))
        {
            best = Some((cols, rows, size));
        }
    }
    best
}
