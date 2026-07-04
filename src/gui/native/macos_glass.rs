//! macOS 26+ 原生 Liquid Glass 底部工具栏（`NSGlassEffectView` + `NSButton`）。

use std::cell::RefCell;
use std::sync::mpsc::{self, Receiver, Sender};

use eframe::Frame;
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{define_class, msg_send, sel, DefinedClass, MainThreadOnly};
use objc2_app_kit::{
  NSAutoresizingMaskOptions, NSBezelStyle, NSButton, NSControlSize, NSGlassEffectView,
  NSGlassEffectViewStyle, NSStackView, NSUserInterfaceLayoutOrientation, NSView,
};
use objc2_foundation::{
  ns_string, MainThreadMarker, NSArray, NSOperatingSystemVersion, NSProcessInfo, NSObject,
  NSObjectProtocol, NSPoint, NSRect, NSSize,
};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};

/// 原生工具栏高度（pt），与 Auto Layout 帧一致。
pub const TOOLBAR_HEIGHT: f32 = 74.0;

/// 工具栏按钮动作。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolbarAction {
  Start,
  Cancel,
  OpenOutput,
}

#[derive(Default)]
struct ToolbarTargetIvars {
  tx: RefCell<Option<Sender<ToolbarAction>>>,
}

define_class!(
  #[unsafe(super = NSObject)]
  #[thread_kind = MainThreadOnly]
  #[ivars = ToolbarTargetIvars]
  struct ToolbarTarget;

  unsafe impl NSObjectProtocol for ToolbarTarget {}

  impl ToolbarTarget {
    #[unsafe(method(imgforgeStart:))]
    fn imgforge_start(&self, _sender: &NSButton) {
      if let Some(tx) = self.ivars().tx.borrow().as_ref() {
        let _ = tx.send(ToolbarAction::Start);
      }
    }

    #[unsafe(method(imgforgeCancel:))]
    fn imgforge_cancel(&self, _sender: &NSButton) {
      if let Some(tx) = self.ivars().tx.borrow().as_ref() {
        let _ = tx.send(ToolbarAction::Cancel);
      }
    }

    #[unsafe(method(imgforgeOpenOutput:))]
    fn imgforge_open_output(&self, _sender: &NSButton) {
      if let Some(tx) = self.ivars().tx.borrow().as_ref() {
        let _ = tx.send(ToolbarAction::OpenOutput);
      }
    }
  }
);

impl ToolbarTarget {
  fn new(mtm: MainThreadMarker, tx: Sender<ToolbarAction>) -> Retained<Self> {
    let this = Self::alloc(mtm).set_ivars(ToolbarTargetIvars {
      tx: RefCell::new(Some(tx)),
    });
    unsafe { msg_send![super(this), init] }
  }
}

/// AppKit 原生 Liquid Glass 底部操作栏。
pub struct NativeGlassToolbar {
  active: bool,
  action_rx: Receiver<ToolbarAction>,
  _target: Retained<ToolbarTarget>,
  btn_start: Retained<NSButton>,
  btn_cancel: Retained<NSButton>,
  btn_open: Retained<NSButton>,
  glass: Retained<NSGlassEffectView>,
  _content: Retained<NSView>,
  stack: Retained<NSStackView>,
  parent: Retained<NSView>,
}

impl NativeGlassToolbar {
  /// 在 winit 内容视图上挂载原生玻璃工具栏；不可用时返回 `None`（回退 egui 绘制）。
  pub fn try_install(frame: &Frame) -> Option<Self> {
    let mtm = MainThreadMarker::new()?;
    if !liquid_glass_available(mtm) {
      tracing::debug!("NSGlassEffectView unavailable; using egui toolbar fallback");
      return None;
    }

    let handle = frame.window_handle().ok()?;
    let parent = unsafe {
      match handle.as_raw() {
        RawWindowHandle::AppKit(appkit) => {
          Retained::retain(appkit.ns_view.as_ptr().cast::<NSView>())
        }
        _ => None,
      }
    }?;

    let (action_tx, action_rx) = mpsc::channel();
    let target = ToolbarTarget::new(mtm, action_tx);

    let btn_start = make_button(
      mtm,
      &target,
      ns_string!("开始转换"),
      sel!(imgforgeStart:),
      NSBezelStyle::Push,
    );
    let btn_cancel = make_button(
      mtm,
      &target,
      ns_string!("取消"),
      sel!(imgforgeCancel:),
      NSBezelStyle::Glass,
    );
    let btn_open = make_button(
      mtm,
      &target,
      ns_string!("打开输出"),
      sel!(imgforgeOpenOutput:),
      NSBezelStyle::Glass,
    );

    let button_views = [
      btn_start.as_ref() as &NSView,
      btn_cancel.as_ref() as &NSView,
      btn_open.as_ref() as &NSView,
    ];
    let buttons = NSArray::from_slice(&button_views);
    let stack = NSStackView::stackViewWithViews(&buttons, mtm);
    stack.setOrientation(NSUserInterfaceLayoutOrientation::Horizontal);
    stack.setSpacing(12.0);

    let content = NSView::new(mtm);
    content.addSubview(&stack);
    center_stack_in_toolbar(&stack, &content);

    let glass = NSGlassEffectView::initWithFrame(
      NSGlassEffectView::alloc(mtm),
      NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(100.0, TOOLBAR_HEIGHT as f64)),
    );
    glass.setStyle(NSGlassEffectViewStyle::Regular);
    glass.setCornerRadius(20.0);
    glass.setContentView(Some(&content));
    pin_child_fill(&content, &glass);

    parent.addSubview(&glass);
    layout_glass(&glass, &parent);

    tracing::info!("installed native NSGlassEffectView toolbar");

    Some(Self {
      active: true,
      action_rx,
      _target: target,
      btn_start,
      btn_cancel,
      btn_open,
      glass,
      _content: content,
      stack,
      parent,
    })
  }

  pub fn is_active(&self) -> bool {
    self.active
  }

  pub fn sync(&self, enabled: bool, running: bool) {
    self.btn_start.setEnabled(enabled);
    self.btn_cancel.setEnabled(running);
    self.btn_open.setEnabled(true);
    self.layout();
  }

  pub fn layout(&self) {
    layout_glass(&self.glass, &self.parent);
    center_stack_in_toolbar(&self.stack, &self._content);
  }

  pub fn drain_actions(&mut self) -> Vec<ToolbarAction> {
    self.action_rx.try_iter().collect()
  }
}

fn make_button(
  mtm: MainThreadMarker,
  target: &ToolbarTarget,
  title: &objc2_foundation::NSString,
  action: objc2::runtime::Sel,
  bezel: NSBezelStyle,
) -> Retained<NSButton> {
  let target_obj: &AnyObject = target.as_ref();
  let button = unsafe {
    NSButton::buttonWithTitle_target_action(title, Some(target_obj), Some(action), mtm)
  };
  button.setBezelStyle(bezel);
  button.setControlSize(NSControlSize::Large);
  button
}

fn pin_child_fill(child: &NSView, parent: &NSView) {
  let bounds = parent.bounds();
  child.setFrame(bounds);
  child.setAutoresizingMask(
    NSAutoresizingMaskOptions::ViewWidthSizable | NSAutoresizingMaskOptions::ViewHeightSizable,
  );
}

fn center_stack_in_toolbar(stack: &NSStackView, content: &NSView) {
  let bounds = content.bounds();
  let frame = stack.frame();
  let stack_w = frame.size.width.max(1.0);
  let stack_h = frame.size.height.max(40.0);
  stack.setFrame(NSRect::new(
    NSPoint::new(
      ((bounds.size.width - stack_w) * 0.5).max(12.0),
      ((bounds.size.height - stack_h) * 0.5).max(8.0),
    ),
    NSSize::new(stack_w, stack_h),
  ));
  stack.setAutoresizingMask(
    NSAutoresizingMaskOptions::ViewMinXMargin
      | NSAutoresizingMaskOptions::ViewMaxXMargin
      | NSAutoresizingMaskOptions::ViewMinYMargin
      | NSAutoresizingMaskOptions::ViewMaxYMargin,
  );
}

fn layout_glass(glass: &NSGlassEffectView, parent: &NSView) {
  let bounds = parent.bounds();
  glass.setFrame(NSRect::new(
    NSPoint::new(0.0, 0.0),
    NSSize::new(bounds.size.width, TOOLBAR_HEIGHT as f64),
  ));
  glass.setAutoresizingMask(
    NSAutoresizingMaskOptions::ViewWidthSizable | NSAutoresizingMaskOptions::ViewMinYMargin,
  );
}

fn liquid_glass_available(_mtm: MainThreadMarker) -> bool {
  let info = NSProcessInfo::processInfo();
  let version: NSOperatingSystemVersion = info.operatingSystemVersion();
  if version.majorVersion < 26 {
    return false;
  }

  // 运行时确认 AppKit 已注册 NSGlassEffectView（旧系统链接但新 SDK 构建时兜底）。
  objc2::runtime::AnyClass::get(c"NSGlassEffectView").is_some()
}
