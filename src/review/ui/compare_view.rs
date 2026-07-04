//! 双图对比视图：左右/上下分屏，同步缩放平移，基于 AnnotationCanvas。

use std::path::Path;

use eframe::egui::{self, Context, Ui, Vec2};

use crate::review::domain::coords::ViewportTransform;
use crate::review::ui::annotation_canvas::{
  Annotation, AnnotationCanvas, AnnotationCanvasEvent, CanvasUiOptions,
};
use crate::review::ui::texture_cache::ImageTextureCache;

const SPLITTER_SIZE: f32 = 6.0;
const MIN_PANE: f32 = 120.0;

/// 显示模式：单图 / 分屏对比。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CompareDisplayMode {
  #[default]
  Single,
  Split,
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
    }
  }

  /// 1:1 原始比例：同时作用于可见画布。
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
    }
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
    self.header_ui(ui);

    let original_entry = self
      .textures
      .load(ctx, original_path, thumb_path)
      .cloned();
    let Some(original) = original_entry else {
      ui.colored_label(egui::Color32::LIGHT_RED, "无法加载原图");
      return Vec::new();
    };
    self.left.set_image_size(original.size);

    let converted_entry = converted_path
      .and_then(|p| self.textures.load(ctx, p, None).cloned());
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
    };

    // 状态变更：根据本帧视口变化执行同步
    self.apply_viewport_sync();
    events
  }

  fn header_ui(&mut self, ui: &mut Ui) {
    ui.horizontal(|ui| {
      ui.label("显示");
      ui.selectable_value(&mut self.mode, CompareDisplayMode::Single, "单图");
      ui.selectable_value(&mut self.mode, CompareDisplayMode::Split, "对比");
      ui.separator();
      ui.checkbox(&mut self.config.sync_viewport, "同步缩放平移");
      if self.mode == CompareDisplayMode::Split {
        ui.checkbox(&mut self.config.reverse_sync, "右侧反向同步");
        ui.separator();
        ui.label("布局");
        ui.selectable_value(
          &mut self.config.layout,
          SplitLayout::Horizontal,
          "左右",
        );
        ui.selectable_value(
          &mut self.config.layout,
          SplitLayout::Vertical,
          "上下",
        );
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
}
