//! eframe glow 上下文桥：供 libmpv OpenGL render 使用。

use std::ffi::{c_void, CStr};
use std::sync::Arc;

/// eframe 的 get_proc_address 实际只查符号表，可跨线程只读调用。
struct SyncGetProc(&'static (dyn Fn(&CStr) -> *const c_void));

// SAFETY: loader 无可变状态，仅转发到平台 GL 入口。
unsafe impl Send for SyncGetProc {}
unsafe impl Sync for SyncGetProc {}

impl Clone for SyncGetProc {
    fn clone(&self) -> Self {
        Self(self.0)
    }
}

/// 应用生命周期内有效的 OpenGL 加载器 + glow 上下文。
#[derive(Clone)]
pub struct GlowBridge {
    pub gl: Arc<eframe::glow::Context>,
    gpa: SyncGetProc,
}

impl GlowBridge {
    /// 从 eframe CreationContext 捕获（需 `Renderer::Glow`）。
    pub fn from_creation_context(cc: &eframe::CreationContext<'_>) -> Option<Self> {
        let gl = cc.gl.clone()?;
        let gpa_ref = cc.get_proc_address?;
        // GL display / loader 与窗口同寿。
        let gpa_static: &'static (dyn Fn(&CStr) -> *const c_void) =
            unsafe { std::mem::transmute(gpa_ref) };
        Some(Self {
            gl,
            gpa: SyncGetProc(gpa_static),
        })
    }

    pub fn get_proc_address(&self, name: &CStr) -> *const c_void {
        (self.gpa.0)(name)
    }
}

/// C 回调：mpv OpenGL init 用。
pub unsafe extern "C" fn mpv_get_proc_address(
    ctx: *mut c_void,
    name: *const std::os::raw::c_char,
) -> *mut c_void {
    if ctx.is_null() || name.is_null() {
        return std::ptr::null_mut();
    }
    let bridge = unsafe { &*(ctx as *const GlowBridge) };
    let c_name = unsafe { CStr::from_ptr(name) };
    bridge.get_proc_address(c_name) as *mut c_void
}
