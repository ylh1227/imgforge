//! 双图对比视图：左右/上下分屏，同步缩放平移，基于 AnnotationCanvas。

use std::path::Path;

use eframe::egui::{self, Context, RichText, Ui, Vec2};

use crate::gui::{theme, widgets};

use crate::review::domain::coords::ViewportTransform;
use crate::review::ui::annotation_canvas::{
  Annotation, AnnotationCanvas, AnnotationCanvasEvent, CanvasUiOptions,
};
use crate::review::service::ImageLoadTier;
use crate::review::ui::texture_cache::ImageTextureCache;

const SPLITTER_SIZE: f32 = 6.0;
const MIN_PANE: f32 = 120.0;

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
  textures: ImageTextureCache,
  last_viewport_source: ViewportSource,
  last_left_viewport: ViewportTransform,
  last_right_viewport: ViewportTransform,
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
    }
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

  /// 切换对比：手动翻转显示原图/转换后。
  pub fn toggle_flip(&mut self) {
    if self.mode == CompareDisplayMode::Toggle {
      self.toggle_show_converted = !self.toggle_show_converted;
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
      _ => {
        self.left.set_zoom_100(canvas_size);
      }
    }
  }

  pub fn prefetch_neighbors(
    &self,
    paths: &[std::path::PathBuf],
    center: usize,
    radius: usize,
    thumb_paths: &[Option<std::path::PathBuf>],
  ) {
    self
      .textures
      .loader()
      .prefetch_neighbors(paths, center, radius, thumb_paths);
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
    self.header_ui(ui, ctx);

    self.textures
      .request(original_path, thumb_path, ImageLoadTier::Thumb);
    if let Some(p) = converted_path {
      self.textures.request(p, None, ImageLoadTier::Thumb);
    }
    self.textures.poll(ctx);

    let original_entry = self.textures.get(original_path).cloned();
    let Some(original) = original_entry else {
      ui.colored_label(theme::error_color(ui.style().visuals.dark_mode), "正在加载原图…");
      ctx.request_repaint();
      return Vec::new();
    };
    self.left.set_image_size(original.size);
    let zoom = self.left.viewport().zoom;
    self.textures
      .maybe_upgrade(original_path, thumb_path, zoom);

    let converted_entry = converted_path
      .and_then(|p| self.textures.get(p).cloned());
    if converted_entry.is_none() {
      if let Some(p) = converted_path {
        self.textures.request(p, None, ImageLoadTier::Preview);
        self.textures.poll(ctx);
      }
    }
    let converted_entry = converted_path.and_then(|p| self.textures.get(p).cloned());
    if let Some(ref c) = converted_entry {
      self.right.set_image_size(c.size);
    }

    let events = match self.mode {
      CompareDisplayMode::Single => {
        self.render_single(ui, &original.texture, annotations)
      }
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
    };

    // 状态变更：根据本帧视口变化执行同步
    self.apply_viewport_sync();
    events
  }

  fn header_ui(&mut self, ui: &mut Ui, ctx: &Context) {
    let dark = ui.style().visuals.dark_mode;
    let canvas_size = ctx.input(|i| {
      i.viewport()
        .inner_rect
        .map(|r| r.size())
        .unwrap_or_else(|| ctx.screen_rect().size())
    });

    ui.horizontal_wrapped(|ui| {
      widgets::section_label(ui, "显示");
      if widgets::toggle_chip(
        ui,
        "单图",
        self.mode == CompareDisplayMode::Single,
        true,
      ) {
        self.mode = CompareDisplayMode::Single;
      }
      if widgets::toggle_chip(
        ui,
        "对比",
        self.mode == CompareDisplayMode::Split,
        true,
      ) {
        self.mode = CompareDisplayMode::Split;
      }
      if widgets::toggle_chip(
        ui,
        "卷帘",
        self.mode == CompareDisplayMode::Wipe,
        true,
      ) {
        self.mode = CompareDisplayMode::Wipe;
      }
      if widgets::toggle_chip(
        ui,
        "叠加",
        self.mode == CompareDisplayMode::Overlay,
        true,
      ) {
        self.mode = CompareDisplayMode::Overlay;
      }
      if widgets::toggle_chip(
        ui,
        "差异",
        self.mode == CompareDisplayMode::Diff,
        true,
      ) {
        self.mode = CompareDisplayMode::Diff;
      }
      if widgets::toggle_chip(
        ui,
        "切换",
        self.mode == CompareDisplayMode::Toggle,
        true,
      ) {
        self.mode = CompareDisplayMode::Toggle;
      }

      if self.mode == CompareDisplayMode::Toggle {
        ui.separator();
        if widgets::compact_secondary_button(ui, "翻转 (空格)", true).clicked() {
          self.toggle_show_converted = !self.toggle_show_converted;
        }
        ui.checkbox(&mut self.toggle_auto, "自动切换");
        if self.toggle_auto {
          ui.add(
            egui::Slider::new(&mut self.toggle_interval, 0.2..=2.0)
              .suffix("s")
              .text("间隔"),
          );
        }
      }

      if matches!(
        self.mode,
        CompareDisplayMode::Overlay | CompareDisplayMode::Wipe
      ) {
        ui.separator();
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
      }
      ui.checkbox(&mut self.magnifier_enabled, "放大镜");

      ui.separator();
      ui.checkbox(&mut self.config.sync_viewport, "同步缩放平移");
      if self.mode == CompareDisplayMode::Split {
        ui.checkbox(&mut self.config.reverse_sync, "右侧反向同步");
        ui.separator();
        ui.label(
          RichText::new("布局")
            .font(theme::section_font())
            .color(theme::primary_label(dark)),
        );
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
      }

      ui.separator();
      if widgets::compact_secondary_button(ui, "适应窗口", true).clicked() {
        self.fit_to_window(canvas_size);
      }
      if widgets::compact_secondary_button(ui, "100%", true).clicked() {
        self.set_zoom_100(canvas_size);
      }
    });
    ui.add_space(4.0);
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
      ViewportSource::None => {}
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
      self.render_right_pane(ui, Some(right), Vec2::new(ui.available_width(), available.y));
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
    let label = if show_converted { "转换后" } else { "原图" };
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
