//! 可复用 egui 图片标注画布：纯 UI、事件驱动、归一化坐标。

mod coords;
mod draw;
mod events;
mod history;
mod types;

pub use coords::{
  image_to_normalized, normalized_to_screen, screen_to_image, screen_to_normalized, zoom_at,
};
pub use events::AnnotationCanvasEvent;
pub use types::{
  Annotation, AnnotationDraft, AnnotationKind, AnnotationPosition, AnnotationStyle,
  AnnotationType, CanvasTool, NormalizedRect, NormRect,
};

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use eframe::egui::{
  self, Color32, Context, Key, Pos2, Rect, Sense, TextureHandle, Ui, Vec2,
};

use crate::review::domain::annotation::{ArrowPosition, RectanglePosition, TextPosition};
use crate::review::domain::coords::egui_bridge;
use crate::review::domain::coords::ViewportTransform;

use coords::{norm_to_screen_pos2, screen_rect_to_norm, screen_to_norm, NormPoint, ScreenPoint};
use draw::{hit_test, paint_annotation, paint_arrow_head, stroke_from_style};
use events::AnnotationCanvasEvent as Event;
use history::HistoryStack;
use types::AnnotationFingerprint;

const MIN_ZOOM: f32 = 0.1;
const MAX_ZOOM: f32 = 5.0;

/// 画布 UI 行为选项（嵌入 CompareView 等复合组件时使用）。
#[derive(Debug, Clone, Copy)]
pub struct CanvasUiOptions {
  pub show_toolbar: bool,
  /// 只读：禁止创建/编辑标注。
  pub read_only: bool,
  /// 是否允许平移与滚轮缩放。
  pub allow_pan_zoom: bool,
  /// 是否绘制标注层。
  pub draw_annotations: bool,
}

impl Default for CanvasUiOptions {
  fn default() -> Self {
    Self {
      show_toolbar: true,
      read_only: false,
      allow_pan_zoom: true,
      draw_annotations: true,
    }
  }
}

impl CanvasUiOptions {
  pub const READ_ONLY_PREVIEW: Self = Self {
    show_toolbar: false,
    read_only: true,
    allow_pan_zoom: false,
    draw_annotations: false,
  };

  pub const READ_ONLY_PAN: Self = Self {
    show_toolbar: false,
    read_only: true,
    allow_pan_zoom: true,
    draw_annotations: false,
  };
}

/// 拖拽交互状态（组件内部，不向外暴露）。
#[derive(Debug, Clone)]
enum DragState {
  None,
  Panning,
  Creating {
    tool: CanvasTool,
    start: Pos2,
  },
  Moving {
    id: i64,
    start_position: AnnotationPosition,
    grab_norm: NormPoint,
  },
  ResizingRect {
    id: i64,
    start_position: AnnotationPosition,
    handle: RectHandle,
  },
  MovingArrowEnd {
    id: i64,
    start_position: AnnotationPosition,
    end_index: u8,
  },
}

#[derive(Debug, Clone, Copy)]
enum RectHandle {
  TopLeft,
  TopRight,
  BottomRight,
  BottomLeft,
}

/// 文字输入弹层状态。
#[derive(Debug, Clone)]
struct TextPrompt {
  norm: NormPoint,
  buffer: String,
}

/// 可复用图片标注画布（无副作用，事件向上传递）。
pub struct AnnotationCanvas {
  pub tool: CanvasTool,
  pub style: AnnotationStyle,
  image_size: (u32, u32),
  transform: ViewportTransform,
  selected_id: Option<i64>,
  drag: DragState,
  /// 拖拽中的预览位置（释放时才向外部发 Update 事件）。
  preview: Option<(i64, AnnotationPosition)>,
  draft_pointer: Option<Pos2>,
  text_prompt: Option<TextPrompt>,
  history: HistoryStack,
  cache_key: u64,
  last_texture_id: Option<egui::TextureId>,
  hovered_id: Option<i64>,
  /// 创建标注后等待上层写回，再自动选中。
  pending_select: Option<AnnotationFingerprint>,
}

impl Default for AnnotationCanvas {
  fn default() -> Self {
    Self::new()
  }
}

impl AnnotationCanvas {
  pub fn new() -> Self {
    Self {
      tool: CanvasTool::Select,
      style: AnnotationStyle::default(),
      image_size: (1, 1),
      transform: ViewportTransform::fit_image(coords::Vec2::new(400.0, 300.0), (1, 1)),
      selected_id: None,
      drag: DragState::None,
      preview: None,
      draft_pointer: None,
      text_prompt: None,
      history: HistoryStack::default(),
      cache_key: 0,
      last_texture_id: None,
      hovered_id: None,
      pending_select: None,
    }
  }

  /// 设置原图尺寸（加载新纹理后由上层调用）。
  pub fn set_image_size(&mut self, size: (u32, u32)) {
    if self.image_size != size {
      self.image_size = size;
      self.cache_key = 0;
    }
  }

  /// 适配窗口：图片按比例居中填满画布。
  pub fn fit_to_window(&mut self, canvas_size: Vec2) {
    self.transform =
      ViewportTransform::fit_image(egui_bridge::vec2(canvas_size), self.image_size);
    self.transform.zoom = self.transform.zoom.clamp(MIN_ZOOM, MAX_ZOOM);
  }

  /// 原始比例 1:1 显示。
  pub fn set_zoom_100(&mut self, canvas_size: Vec2) {
    self.transform =
      ViewportTransform::one_to_one(egui_bridge::vec2(canvas_size), self.image_size);
    self.transform.zoom = 1.0;
  }

  pub fn selected_id(&self) -> Option<i64> {
    self.selected_id
  }

  pub fn set_selected_id(&mut self, id: Option<i64>) {
    self.selected_id = id;
  }

  pub fn viewport(&self) -> ViewportTransform {
    self.transform
  }

  pub fn set_viewport(&mut self, transform: ViewportTransform) {
    self.transform = transform;
  }

  pub fn image_size(&self) -> (u32, u32) {
    self.image_size
  }

  /// 主渲染入口（默认显示工具栏、可编辑）。
  pub fn ui(
    &mut self,
    ui: &mut Ui,
    image: &TextureHandle,
    annotations: &[Annotation],
  ) -> Vec<Event> {
    self.ui_with_options(ui, image, annotations, CanvasUiOptions::default())
  }

  /// 可配置的画布渲染入口。
  pub fn ui_with_options(
    &mut self,
    ui: &mut Ui,
    image: &TextureHandle,
    annotations: &[Annotation],
    options: CanvasUiOptions,
  ) -> Vec<Event> {
    let mut events = Vec::new();

    if let Some(fp) = &self.pending_select {
      if let Some(ann) = annotations.iter().find(|a| fp.matches(a)) {
        self.selected_id = Some(ann.id);
        self.pending_select = None;
        events.push(Event::SelectionChanged {
          id: Some(ann.id),
        });
      }
    }

    if options.show_toolbar {
      self.toolbar_ui(ui, &mut events);
      if !options.read_only {
        events.extend(self.handle_keyboard(ui, annotations));
      }
    } else if !options.read_only {
      events.extend(self.handle_keyboard(ui, annotations));
    }

    let available = ui.available_size();
    let (rect, response) = ui.allocate_exact_size(available, Sense::click_and_drag());
    let painter = ui.painter_at(rect);

    let new_key = cache_key(image.id(), annotations);
    if self.last_texture_id != Some(image.id()) || self.cache_key != new_key {
      self.last_texture_id = Some(image.id());
      self.cache_key = new_key;
    }
    if self.image_size != (1, 1) && self.transform.image_rect.size.x < 2.0 {
      self.fit_to_window(rect.size());
    }

    let pan_mode = options.allow_pan_zoom
      && (ui.input(|i| i.pointer.middle_down() || (i.key_down(Key::Space) && i.pointer.primary_down()))
        || matches!(self.drag, DragState::Panning));

    if options.allow_pan_zoom && response.hovered() {
      let scroll = ui.input(|i| i.raw_scroll_delta.y);
      if scroll != 0.0 {
        let factor = if scroll > 0.0 { 1.1 } else { 0.9 };
        if let Some(pos) = ui.input(|i| i.pointer.hover_pos()) {
          if rect.contains(pos) {
            coords::zoom_at(
              &mut self.transform,
              factor,
              ScreenPoint { x: pos.x, y: pos.y },
              MIN_ZOOM,
              MAX_ZOOM,
            );
          }
        }
      }
    }

    if options.allow_pan_zoom
      && response.dragged()
      && (pan_mode || matches!(self.drag, DragState::Panning))
    {
      let delta = response.drag_delta();
      self.transform.pan.x += delta.x;
      self.transform.pan.y += delta.y;
    }

    let img_rect = egui_bridge::to_egui_rect(self.transform.displayed_image_rect());
    painter.image(
      image.id(),
      img_rect,
      Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
      Color32::WHITE,
    );

    let draw_list = if options.draw_annotations {
      annotations
    } else {
      &[]
    };

    for ann in draw_list {
      let selected = self.selected_id == Some(ann.id);
      if let Some((pid, ref pos)) = self.preview {
        if pid == ann.id {
          let mut preview_ann = ann.clone();
          preview_ann.position = pos.clone();
          paint_annotation(&painter, &preview_ann, &self.transform, selected);
          continue;
        }
      }
      paint_annotation(&painter, ann, &self.transform, selected);
    }

    if !options.read_only {
      self.paint_draft(&painter, ui);
    }

    if options.draw_annotations {
      if let Some(pos) = response.hover_pos() {
        self.hovered_id = hit_test(annotations, &self.transform, pos);
        if self.hovered_id.is_some()
          && self.tool == CanvasTool::Select
          && !pan_mode
          && !options.read_only
        {
          ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
      }
    }
    if pan_mode || matches!(self.drag, DragState::Panning) {
      ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
    }

    if !options.read_only && !pan_mode {
      events.extend(self.handle_pointer(ui, &response, annotations));
    } else if options.read_only && options.allow_pan_zoom && response.dragged() {
      let delta = response.drag_delta();
      self.transform.pan.x += delta.x;
      self.transform.pan.y += delta.y;
    }

    if !options.read_only {
      events.extend(self.text_prompt_ui(ui.ctx(), annotations));
    }
    events
  }

  /// 渲染标注工具栏（颜色、线宽、工具切换），供 CompareView 外置调用。
  pub fn toolbar_ui(&mut self, ui: &mut Ui, events: &mut Vec<Event>) {
    ui.horizontal(|ui| {
      for tool in [
        CanvasTool::Select,
        CanvasTool::Rectangle,
        CanvasTool::Arrow,
        CanvasTool::Text,
      ] {
        if ui
          .selectable_label(self.tool == tool, tool.label())
          .clicked()
        {
          self.tool = tool;
          events.push(Event::ToolChanged { tool });
        }
      }
      ui.separator();
      ui.color_edit_button_srgba_unmultiplied(&mut self.style.color);
      ui.add(
        egui::DragValue::new(&mut self.style.line_width)
          .range(1.0..=12.0)
          .speed(0.1)
          .prefix("线宽 "),
      );
    });
    ui.add_space(4.0);
  }

  fn handle_keyboard(&mut self, ui: &Ui, annotations: &[Annotation]) -> Vec<Event> {
    let mut events = Vec::new();
    let input = ui.input(|i| i.clone());

    if input.key_pressed(Key::Z) && (input.modifiers.ctrl || input.modifiers.command) {
      if let Some(ev) = self.history.undo(annotations) {
        events.push(ev);
      }
    }
    if input.key_pressed(Key::Y) && (input.modifiers.ctrl || input.modifiers.command) {
      if let Some(ev) = self.history.redo(annotations) {
        events.push(ev);
      }
    }
    if input.key_pressed(Key::Delete) || input.key_pressed(Key::Backspace) {
      if let Some(id) = self.selected_id {
        if let Some(ann) = annotations.iter().find(|a| a.id == id) {
          self.history.record_delete(ann.clone());
        }
        events.push(Event::DeleteAnnotation { id });
        self.selected_id = None;
        events.push(Event::SelectionChanged { id: None });
      }
    }
    events
  }

  fn handle_pointer(
    &mut self,
    ui: &Ui,
    response: &egui::Response,
    annotations: &[Annotation],
  ) -> Vec<Event> {
    let mut events = Vec::new();

    if response.drag_started() {
      if let Some(pos) = response.interact_pointer_pos() {
        events.extend(self.on_pointer_down(pos, annotations));
      }
    }
    if response.dragged() {
      if let Some(pos) = response.interact_pointer_pos() {
        self.on_pointer_drag(pos);
      }
    }
    if response.drag_stopped() {
      events.extend(self.on_pointer_up(annotations));
    }
    if response.clicked() {
      if let Some(pos) = response.interact_pointer_pos() {
        events.extend(self.on_click(pos));
      }
    }
    let _ = ui;
    events
  }

  fn on_pointer_down(&mut self, pos: Pos2, annotations: &[Annotation]) -> Vec<Event> {
    let mut events = Vec::new();
    match self.tool {
      CanvasTool::Select => {
        if let Some(id) = hit_test(annotations, &self.transform, pos) {
          self.selected_id = Some(id);
          events.push(Event::SelectionChanged { id: Some(id) });
          if let Some(ann) = annotations.iter().find(|a| a.id == id) {
            let grab_norm = screen_to_norm(ScreenPoint { x: pos.x, y: pos.y }, &self.transform);
            if let Some(handle) = rect_handle_hit(ann, &self.transform, pos) {
              self.drag = DragState::ResizingRect {
                id,
                start_position: ann.position.clone(),
                handle,
              };
            } else if let Some(end) = arrow_end_hit(ann, &self.transform, pos) {
              self.drag = DragState::MovingArrowEnd {
                id,
                start_position: ann.position.clone(),
                end_index: end,
              };
            } else {
              self.drag = DragState::Moving {
                id,
                start_position: ann.position.clone(),
                grab_norm,
              };
            }
          }
        } else {
          self.selected_id = None;
          events.push(Event::SelectionChanged { id: None });
          self.drag = DragState::Panning;
        }
      }
      CanvasTool::Rectangle | CanvasTool::Arrow => {
        self.draft_pointer = Some(pos);
        self.drag = DragState::Creating {
          tool: self.tool,
          start: pos,
        };
      }
      CanvasTool::Text => {}
    }
    events
  }

  fn on_pointer_drag(&mut self, pos: Pos2) {
    let norm = screen_to_norm(ScreenPoint { x: pos.x, y: pos.y }, &self.transform);
    match &self.drag {
      DragState::Creating { start, tool } => {
        self.draft_pointer = Some(pos);
        let _ = (start, tool);
      }
      DragState::Moving {
        id,
        start_position,
        grab_norm,
      } => {
        let dx = norm.x - grab_norm.x;
        let dy = norm.y - grab_norm.y;
        let moved = translate_position(start_position, dx, dy);
        self.preview = Some((*id, moved));
      }
      DragState::ResizingRect {
        id,
        start_position,
        handle,
      } => {
        if let AnnotationPosition::Rectangle(r) = start_position {
          let mut nr = *r;
          match handle {
            RectHandle::TopLeft => {
              nr.x0 = norm.x;
              nr.y0 = norm.y;
            }
            RectHandle::TopRight => {
              nr.x1 = norm.x;
              nr.y0 = norm.y;
            }
            RectHandle::BottomRight => {
              nr.x1 = norm.x;
              nr.y1 = norm.y;
            }
            RectHandle::BottomLeft => {
              nr.x0 = norm.x;
              nr.y1 = norm.y;
            }
          }
          self.preview = Some((*id, AnnotationPosition::Rectangle(nr)));
        }
      }
      DragState::MovingArrowEnd {
        id,
        start_position,
        end_index,
      } => {
        if let AnnotationPosition::Arrow(a) = start_position {
          let mut na = *a;
          if *end_index == 0 {
            na.x0 = norm.x;
            na.y0 = norm.y;
          } else {
            na.x1 = norm.x;
            na.y1 = norm.y;
          }
          self.preview = Some((*id, AnnotationPosition::Arrow(na)));
        }
      }
      _ => {}
    }
  }

  fn on_pointer_up(&mut self, _annotations: &[Annotation]) -> Vec<Event> {
    let mut events = Vec::new();

    match self.drag.clone() {
      DragState::Creating { tool, start } => {
        if let Some(end) = self.draft_pointer.take() {
          let norm = screen_rect_to_norm(
            ScreenPoint { x: start.x, y: start.y },
            ScreenPoint { x: end.x, y: end.y },
            &self.transform,
          );
          if (norm.x1 - norm.x0).abs() > 0.005 || (norm.y1 - norm.y0).abs() > 0.005 {
            let (kind, position) = match tool {
              CanvasTool::Rectangle => (
                AnnotationKind::Rectangle,
                AnnotationPosition::Rectangle(RectanglePosition {
                  x0: norm.x0,
                  y0: norm.y0,
                  x1: norm.x1,
                  y1: norm.y1,
                }),
              ),
              CanvasTool::Arrow => (
                AnnotationKind::Arrow,
                AnnotationPosition::Arrow(ArrowPosition {
                  x0: norm.x0,
                  y0: norm.y0,
                  x1: norm.x1,
                  y1: norm.y1,
                }),
              ),
              _ => return events,
            };
            self.history.record_create(
              kind,
              position.clone(),
              self.style.clone(),
              String::new(),
            );
            events.push(Event::CreateAnnotation {
              kind,
              position: position.clone(),
              style: self.style.clone(),
              content: String::new(),
            });
            self.tool = CanvasTool::Select;
            events.push(Event::ToolChanged {
              tool: CanvasTool::Select,
            });
            self.pending_select = Some(AnnotationFingerprint::from_parts(kind, &position));
          }
        }
      }
      DragState::Moving { id, start_position, .. }
      | DragState::ResizingRect { id, start_position, .. }
      | DragState::MovingArrowEnd { id, start_position, .. } => {
        if let Some((pid, after)) = self.preview.take() {
          if pid == id && after != start_position {
            self
              .history
              .record_update(id, start_position, after.clone());
            events.push(Event::UpdateAnnotation { id, position: after });
          }
        }
      }
      _ => {}
    }

    self.drag = DragState::None;
    self.draft_pointer = None;
    events
  }

  fn on_click(&mut self, pos: Pos2) -> Vec<Event> {
    if self.tool == CanvasTool::Text {
      let norm = screen_to_norm(ScreenPoint { x: pos.x, y: pos.y }, &self.transform);
      self.text_prompt = Some(TextPrompt {
        norm,
        buffer: String::new(),
      });
    }
    Vec::new()
  }

  fn paint_draft(&self, painter: &egui::Painter, ui: &Ui) {
    let DragState::Creating { start, tool } = &self.drag else {
      return;
    };
    let Some(pointer) = self.draft_pointer.or_else(|| ui.input(|i| i.pointer.latest_pos())) else {
      return;
    };
    if !pointer.is_finite() {
      return;
    }
    let stroke = stroke_from_style(&self.style);
    match tool {
      CanvasTool::Rectangle => {
        painter.rect_stroke(
          Rect::from_two_pos(*start, pointer),
          0.0,
          stroke,
          egui::StrokeKind::Outside,
        );
      }
      CanvasTool::Arrow => {
        painter.line_segment([*start, pointer], stroke);
        paint_arrow_head(painter, *start, pointer, stroke);
      }
      _ => {}
    }
  }

  fn text_prompt_ui(&mut self, ctx: &Context, _annotations: &[Annotation]) -> Vec<Event> {
    let Some(prompt) = self.text_prompt.clone() else {
      return Vec::new();
    };
    let mut events = Vec::new();
    let mut open = true;
    let mut buffer = prompt.buffer;

    egui::Window::new("文字备注")
      .collapsible(false)
      .resizable(false)
      .open(&mut open)
      .show(ctx, |ui| {
        ui.text_edit_singleline(&mut buffer);
        ui.horizontal(|ui| {
          if ui.button("确认").clicked() && !buffer.trim().is_empty() {
            let position = AnnotationPosition::Text(TextPosition {
              x: prompt.norm.x,
              y: prompt.norm.y,
            });
            self.history.record_create(
              AnnotationKind::Text,
              position.clone(),
              self.style.clone(),
              buffer.clone(),
            );
            events.push(Event::CreateAnnotation {
              kind: AnnotationKind::Text,
              position: position.clone(),
              style: self.style.clone(),
              content: buffer.clone(),
            });
            self.tool = CanvasTool::Select;
            events.push(Event::ToolChanged {
              tool: CanvasTool::Select,
            });
            self.pending_select =
              Some(AnnotationFingerprint::from_parts(AnnotationKind::Text, &position));
            self.text_prompt = None;
          }
          if ui.button("取消").clicked() {
            self.text_prompt = None;
          }
        });
      });

    if !open {
      self.text_prompt = None;
    } else if self.text_prompt.is_some() {
      self.text_prompt.as_mut().unwrap().buffer = buffer;
    }
    events
  }
}

fn cache_key(texture_id: egui::TextureId, annotations: &[Annotation]) -> u64 {
  let mut h = DefaultHasher::new();
  texture_id.hash(&mut h);
  for a in annotations {
    a.id.hash(&mut h);
    (a.kind.db_value() as u64).hash(&mut h);
  }
  h.finish()
}

fn translate_position(pos: &AnnotationPosition, dx: f32, dy: f32) -> AnnotationPosition {
  match pos {
    AnnotationPosition::Rectangle(r) => AnnotationPosition::Rectangle(RectanglePosition {
      x0: (r.x0 + dx).clamp(0.0, 1.0),
      y0: (r.y0 + dy).clamp(0.0, 1.0),
      x1: (r.x1 + dx).clamp(0.0, 1.0),
      y1: (r.y1 + dy).clamp(0.0, 1.0),
    }),
    AnnotationPosition::Arrow(a) => AnnotationPosition::Arrow(ArrowPosition {
      x0: (a.x0 + dx).clamp(0.0, 1.0),
      y0: (a.y0 + dy).clamp(0.0, 1.0),
      x1: (a.x1 + dx).clamp(0.0, 1.0),
      y1: (a.y1 + dy).clamp(0.0, 1.0),
    }),
    AnnotationPosition::Text(t) => AnnotationPosition::Text(TextPosition {
      x: (t.x + dx).clamp(0.0, 1.0),
      y: (t.y + dy).clamp(0.0, 1.0),
    }),
  }
}

fn rect_handle_hit(ann: &Annotation, transform: &ViewportTransform, point: Pos2) -> Option<RectHandle> {
  let AnnotationPosition::Rectangle(r) = &ann.position else {
    return None;
  };
  let rect = egui_bridge::to_egui_rect(coords::norm_rect_to_screen(
    coords::NormRect {
      x0: r.x0,
      y0: r.y0,
      x1: r.x1,
      y1: r.y1,
    },
    transform,
  ));
  let corners = [
    (rect.left_top(), RectHandle::TopLeft),
    (rect.right_top(), RectHandle::TopRight),
    (rect.right_bottom(), RectHandle::BottomRight),
    (rect.left_bottom(), RectHandle::BottomLeft),
  ];
  for (p, h) in corners {
    if point.distance(p) <= 8.0 {
      return Some(h);
    }
  }
  None
}

fn arrow_end_hit(ann: &Annotation, transform: &ViewportTransform, point: Pos2) -> Option<u8> {
  let AnnotationPosition::Arrow(a) = &ann.position else {
    return None;
  };
  let p0 = egui_bridge::to_egui_pos(norm_to_screen_pos2(
    NormPoint { x: a.x0, y: a.y0 },
    transform,
  ));
  let p1 = egui_bridge::to_egui_pos(norm_to_screen_pos2(
    NormPoint { x: a.x1, y: a.y1 },
    transform,
  ));
  if point.distance(p0) <= 8.0 {
    return Some(0);
  }
  if point.distance(p1) <= 8.0 {
    return Some(1);
  }
  None
}
