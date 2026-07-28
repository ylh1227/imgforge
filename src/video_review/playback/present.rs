//! libmpv 出帧：OpenGL FBO（主路径）+ SW RGBA 回退。

use std::ffi::{c_void, CString};
use std::os::raw::c_int;
use std::ptr;
use std::sync::Arc;

use eframe::egui;
use eframe::glow::{self, HasContext};
use libmpv2::Mpv;
use libmpv2_sys as sys;

use super::glow_bridge::{mpv_get_proc_address, GlowBridge};

const MPV_RENDER_PARAM_API_TYPE: u32 = sys::mpv_render_param_type_MPV_RENDER_PARAM_API_TYPE;
const MPV_RENDER_PARAM_OPENGL_INIT_PARAMS: u32 =
    sys::mpv_render_param_type_MPV_RENDER_PARAM_OPENGL_INIT_PARAMS;
const MPV_RENDER_PARAM_OPENGL_FBO: u32 = sys::mpv_render_param_type_MPV_RENDER_PARAM_OPENGL_FBO;
const MPV_RENDER_PARAM_FLIP_Y: u32 = sys::mpv_render_param_type_MPV_RENDER_PARAM_FLIP_Y;
const MPV_RENDER_PARAM_BLOCK_FOR_TARGET_TIME: u32 =
    sys::mpv_render_param_type_MPV_RENDER_PARAM_BLOCK_FOR_TARGET_TIME;
const MPV_RENDER_PARAM_SW_SIZE: u32 = sys::mpv_render_param_type_MPV_RENDER_PARAM_SW_SIZE;
const MPV_RENDER_PARAM_SW_FORMAT: u32 = sys::mpv_render_param_type_MPV_RENDER_PARAM_SW_FORMAT;
const MPV_RENDER_PARAM_SW_STRIDE: u32 = sys::mpv_render_param_type_MPV_RENDER_PARAM_SW_STRIDE;
const MPV_RENDER_PARAM_SW_POINTER: u32 = sys::mpv_render_param_type_MPV_RENDER_PARAM_SW_POINTER;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PresentMode {
    Sw,
    Gl,
}

impl PresentMode {
    pub fn label(self) -> &'static str {
        match self {
            PresentMode::Sw => "SW",
            PresentMode::Gl => "GL",
        }
    }
}

pub struct RgbaFrame {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

/// GPU 帧：已渲染到 FBO 颜色附件，可供 egui 注册为原生纹理。
pub struct GlFrame {
    #[allow(dead_code)]
    pub texture: glow::Texture,
    pub width: u32,
    pub height: u32,
    /// 垂直翻转 UV（OpenGL vs egui）
    pub flip_y: bool,
}

pub enum PresentFrame {
    Cpu(RgbaFrame),
    Gpu(GlFrame),
}

// --- SW -----------------------------------------------------------------

pub struct SwPresenter {
    ctx: *mut sys::mpv_render_context,
}

impl SwPresenter {
    pub fn new(mpv: &Mpv) -> Result<Self, String> {
        let mut params = [
            sys::mpv_render_param {
                type_: MPV_RENDER_PARAM_API_TYPE,
                data: sys::MPV_RENDER_API_TYPE_SW.as_ptr() as *mut c_void,
            },
            sys::mpv_render_param {
                type_: 0,
                data: ptr::null_mut(),
            },
        ];
        let mut ctx: *mut sys::mpv_render_context = ptr::null_mut();
        let err = unsafe {
            sys::mpv_render_context_create(&mut ctx, mpv.ctx.as_ptr(), params.as_mut_ptr())
        };
        if err < 0 {
            return Err(format!("mpv SW render 初始化失败: {}", mpv_err_msg(err)));
        }
        Ok(Self { ctx })
    }

    pub fn render(&mut self, width: u32, height: u32) -> Result<RgbaFrame, String> {
        render_sw(self.ctx, width, height)
    }
}

impl Drop for SwPresenter {
    fn drop(&mut self) {
        if !self.ctx.is_null() {
            unsafe { sys::mpv_render_context_free(self.ctx) };
            self.ctx = ptr::null_mut();
        }
    }
}

unsafe impl Send for SwPresenter {}

// --- GL -----------------------------------------------------------------

pub struct GlPresenter {
    ctx: *mut sys::mpv_render_context,
    bridge: Arc<GlowBridge>,
    /// 保持 bridge 在 mpv 回调存活（raw 指针传给 init params）
    _bridge_keep: Box<GlowBridge>,
    fbo: glow::Framebuffer,
    color_tex: glow::Texture,
    depth_rbo: glow::Renderbuffer,
    width: u32,
    height: u32,
    egui_tex_id: Option<egui::TextureId>,
}

impl GlPresenter {
    pub fn new(mpv: &Mpv, bridge: Arc<GlowBridge>) -> Result<Self, String> {
        let bridge_box = Box::new((*bridge).clone());
        let bridge_ptr = bridge_box.as_ref() as *const GlowBridge as *mut c_void;

        let mut init = sys::mpv_opengl_init_params {
            get_proc_address: Some(mpv_get_proc_address),
            get_proc_address_ctx: bridge_ptr,
        };

        let mut params = [
            sys::mpv_render_param {
                type_: MPV_RENDER_PARAM_API_TYPE,
                data: sys::MPV_RENDER_API_TYPE_OPENGL.as_ptr() as *mut c_void,
            },
            sys::mpv_render_param {
                type_: MPV_RENDER_PARAM_OPENGL_INIT_PARAMS,
                data: &mut init as *mut _ as *mut c_void,
            },
            sys::mpv_render_param {
                type_: 0,
                data: ptr::null_mut(),
            },
        ];

        let mut ctx: *mut sys::mpv_render_context = ptr::null_mut();
        let err = unsafe {
            sys::mpv_render_context_create(&mut ctx, mpv.ctx.as_ptr(), params.as_mut_ptr())
        };
        if err < 0 {
            return Err(format!("mpv GL render 初始化失败: {}", mpv_err_msg(err)));
        }

        let gl = &bridge.gl;
        let (fbo, color_tex, depth_rbo) =
            unsafe { create_fbo(gl, 64, 64) }.map_err(|e| e.to_string())?;

        Ok(Self {
            ctx,
            bridge,
            _bridge_keep: bridge_box,
            fbo,
            color_tex,
            depth_rbo,
            width: 64,
            height: 64,
            egui_tex_id: None,
        })
    }

    pub fn egui_texture_id(&self) -> Option<egui::TextureId> {
        self.egui_tex_id
    }

    pub fn take_texture_for_register(&mut self) -> Option<(glow::Texture, u32, u32)> {
        if self.egui_tex_id.is_some() {
            return None;
        }
        Some((self.color_tex, self.width, self.height))
    }

    pub fn mark_registered(&mut self, id: egui::TextureId) {
        self.egui_tex_id = Some(id);
    }

    pub fn size(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    pub fn render(&mut self, width: u32, height: u32) -> Result<GlFrame, String> {
        let w = width.max(2);
        let h = height.max(2);
        if w != self.width || h != self.height {
            unsafe {
                resize_fbo(
                    &self.bridge.gl,
                    self.color_tex,
                    self.depth_rbo,
                    self.fbo,
                    w,
                    h,
                )?;
            }
            self.width = w;
            self.height = h;
            // 同 texture 对象，egui id 仍有效
        }

        let fbo_id = self.fbo.0.get() as i32;
        let mut fbo_struct = sys::mpv_opengl_fbo {
            fbo: fbo_id,
            w: w as c_int,
            h: h as c_int,
            internal_format: 0,
        };
        let mut flip: c_int = 1;
        let mut block: c_int = 0;

        let mut params = [
            sys::mpv_render_param {
                type_: MPV_RENDER_PARAM_OPENGL_FBO,
                data: &mut fbo_struct as *mut _ as *mut c_void,
            },
            sys::mpv_render_param {
                type_: MPV_RENDER_PARAM_FLIP_Y,
                data: &mut flip as *mut _ as *mut c_void,
            },
            sys::mpv_render_param {
                type_: MPV_RENDER_PARAM_BLOCK_FOR_TARGET_TIME,
                data: &mut block as *mut _ as *mut c_void,
            },
            sys::mpv_render_param {
                type_: 0,
                data: ptr::null_mut(),
            },
        ];

        let err = unsafe { sys::mpv_render_context_render(self.ctx, params.as_mut_ptr()) };
        if err < 0 {
            return Err(format!("mpv GL render 失败: {}", mpv_err_msg(err)));
        }
        unsafe {
            sys::mpv_render_context_report_swap(self.ctx);
        }

        Ok(GlFrame {
            texture: self.color_tex,
            width: w,
            height: h,
            flip_y: true,
        })
    }

    /// 需要 CPU 像素时（示波器）：从 FBO readback。
    pub fn readback_rgba(&self) -> Result<RgbaFrame, String> {
        let gl = &self.bridge.gl;
        let w = self.width;
        let h = self.height;
        let mut pixels = vec![0u8; (w * h * 4) as usize];
        unsafe {
            gl.bind_framebuffer(glow::FRAMEBUFFER, Some(self.fbo));
            gl.read_pixels(
                0,
                0,
                w as i32,
                h as i32,
                glow::RGBA,
                glow::UNSIGNED_BYTE,
                glow::PixelPackData::Slice(Some(&mut pixels)),
            );
            gl.bind_framebuffer(glow::FRAMEBUFFER, None);
        }
        // GL 原点在左下，翻成 egui 顶左
        let mut rgba = Vec::with_capacity(pixels.len());
        let stride = (w * 4) as usize;
        for row in pixels.chunks_exact(stride).rev() {
            rgba.extend_from_slice(row);
        }
        Ok(RgbaFrame {
            width: w,
            height: h,
            rgba,
        })
    }
}

impl Drop for GlPresenter {
    fn drop(&mut self) {
        if !self.ctx.is_null() {
            unsafe { sys::mpv_render_context_free(self.ctx) };
            self.ctx = ptr::null_mut();
        }
        // 若已交给 egui，不要删 color_tex；未注册则自行释放
        let gl = &self.bridge.gl;
        unsafe {
            if self.egui_tex_id.is_none() {
                gl.delete_texture(self.color_tex);
            }
            gl.delete_renderbuffer(self.depth_rbo);
            gl.delete_framebuffer(self.fbo);
        }
    }
}

unsafe impl Send for GlPresenter {}

// --- Presenter enum -----------------------------------------------------

pub enum Presenter {
    Sw(SwPresenter),
    Gl(GlPresenter),
}

impl Presenter {
    pub fn create(mpv: &Mpv, glow: Option<&Arc<GlowBridge>>) -> Result<Self, String> {
        if let Some(bridge) = glow {
            match GlPresenter::new(mpv, Arc::clone(bridge)) {
                Ok(gl) => return Ok(Presenter::Gl(gl)),
                Err(e) => {
                    tracing::warn!("libmpv GL 出图失败，回退 SW: {e}");
                }
            }
        }
        Ok(Presenter::Sw(SwPresenter::new(mpv)?))
    }

    pub fn mode(&self) -> PresentMode {
        match self {
            Presenter::Sw(_) => PresentMode::Sw,
            Presenter::Gl(_) => PresentMode::Gl,
        }
    }

    pub fn render(&mut self, width: u32, height: u32) -> Result<PresentFrame, String> {
        match self {
            Presenter::Sw(p) => Ok(PresentFrame::Cpu(p.render(width, height)?)),
            Presenter::Gl(p) => Ok(PresentFrame::Gpu(p.render(width, height)?)),
        }
    }

    pub fn as_gl_mut(&mut self) -> Option<&mut GlPresenter> {
        match self {
            Presenter::Gl(p) => Some(p),
            _ => None,
        }
    }

    /// 截一帧 RGBA。GL：优先读回当前 FBO（不改显示分辨率）；空白时才按目标尺寸渲染。
    pub fn capture_rgba(&mut self, width: u32, height: u32) -> Result<RgbaFrame, String> {
        match self {
            Presenter::Sw(p) => p.render(width, height),
            Presenter::Gl(p) => {
                let (cw, ch) = p.size();
                if cw < 2 || ch < 2 {
                    let _ = p.render(width.max(2), height.max(2))?;
                }
                p.readback_rgba()
            }
        }
    }
}

unsafe fn create_fbo(
    gl: &glow::Context,
    w: u32,
    h: u32,
) -> Result<(glow::Framebuffer, glow::Texture, glow::Renderbuffer), String> {
    let color = gl
        .create_texture()
        .map_err(|e| format!("create_texture: {e}"))?;
    gl.bind_texture(glow::TEXTURE_2D, Some(color));
    gl.tex_image_2d(
        glow::TEXTURE_2D,
        0,
        glow::RGBA8 as i32,
        w as i32,
        h as i32,
        0,
        glow::RGBA,
        glow::UNSIGNED_BYTE,
        glow::PixelUnpackData::Slice(None),
    );
    gl.tex_parameter_i32(
        glow::TEXTURE_2D,
        glow::TEXTURE_MIN_FILTER,
        glow::LINEAR as i32,
    );
    gl.tex_parameter_i32(
        glow::TEXTURE_2D,
        glow::TEXTURE_MAG_FILTER,
        glow::LINEAR as i32,
    );
    gl.tex_parameter_i32(
        glow::TEXTURE_2D,
        glow::TEXTURE_WRAP_S,
        glow::CLAMP_TO_EDGE as i32,
    );
    gl.tex_parameter_i32(
        glow::TEXTURE_2D,
        glow::TEXTURE_WRAP_T,
        glow::CLAMP_TO_EDGE as i32,
    );

    let depth = gl
        .create_renderbuffer()
        .map_err(|e| format!("create_renderbuffer: {e}"))?;
    gl.bind_renderbuffer(glow::RENDERBUFFER, Some(depth));
    gl.renderbuffer_storage(
        glow::RENDERBUFFER,
        glow::DEPTH24_STENCIL8,
        w as i32,
        h as i32,
    );

    let fbo = gl
        .create_framebuffer()
        .map_err(|e| format!("create_framebuffer: {e}"))?;
    gl.bind_framebuffer(glow::FRAMEBUFFER, Some(fbo));
    gl.framebuffer_texture_2d(
        glow::FRAMEBUFFER,
        glow::COLOR_ATTACHMENT0,
        glow::TEXTURE_2D,
        Some(color),
        0,
    );
    gl.framebuffer_renderbuffer(
        glow::FRAMEBUFFER,
        glow::DEPTH_STENCIL_ATTACHMENT,
        glow::RENDERBUFFER,
        Some(depth),
    );
    let status = gl.check_framebuffer_status(glow::FRAMEBUFFER);
    gl.bind_framebuffer(glow::FRAMEBUFFER, None);
    if status != glow::FRAMEBUFFER_COMPLETE {
        return Err(format!("FBO incomplete: {status:#x}"));
    }
    Ok((fbo, color, depth))
}

unsafe fn resize_fbo(
    gl: &glow::Context,
    color: glow::Texture,
    depth: glow::Renderbuffer,
    fbo: glow::Framebuffer,
    w: u32,
    h: u32,
) -> Result<(), String> {
    gl.bind_texture(glow::TEXTURE_2D, Some(color));
    gl.tex_image_2d(
        glow::TEXTURE_2D,
        0,
        glow::RGBA8 as i32,
        w as i32,
        h as i32,
        0,
        glow::RGBA,
        glow::UNSIGNED_BYTE,
        glow::PixelUnpackData::Slice(None),
    );
    gl.bind_renderbuffer(glow::RENDERBUFFER, Some(depth));
    gl.renderbuffer_storage(
        glow::RENDERBUFFER,
        glow::DEPTH24_STENCIL8,
        w as i32,
        h as i32,
    );
    gl.bind_framebuffer(glow::FRAMEBUFFER, Some(fbo));
    let status = gl.check_framebuffer_status(glow::FRAMEBUFFER);
    gl.bind_framebuffer(glow::FRAMEBUFFER, None);
    if status != glow::FRAMEBUFFER_COMPLETE {
        return Err(format!("FBO resize incomplete: {status:#x}"));
    }
    Ok(())
}

fn render_sw(
    ctx: *mut sys::mpv_render_context,
    width: u32,
    height: u32,
) -> Result<RgbaFrame, String> {
    let w = width.max(2);
    let h = height.max(2);
    let stride = ((w as usize) * 4 + 63) & !63;
    let mut buf = vec![0u8; stride * h as usize];
    let format = CString::new("rgb0").map_err(|e| e.to_string())?;
    let mut size = [w as c_int, h as c_int];
    let mut stride_sz = stride;
    let mut block: c_int = 0;

    let mut params = [
        sys::mpv_render_param {
            type_: MPV_RENDER_PARAM_SW_SIZE,
            data: size.as_mut_ptr() as *mut c_void,
        },
        sys::mpv_render_param {
            type_: MPV_RENDER_PARAM_SW_FORMAT,
            data: format.as_ptr() as *mut c_void,
        },
        sys::mpv_render_param {
            type_: MPV_RENDER_PARAM_SW_STRIDE,
            data: &mut stride_sz as *mut _ as *mut c_void,
        },
        sys::mpv_render_param {
            type_: MPV_RENDER_PARAM_SW_POINTER,
            data: buf.as_mut_ptr() as *mut c_void,
        },
        sys::mpv_render_param {
            type_: MPV_RENDER_PARAM_BLOCK_FOR_TARGET_TIME,
            data: &mut block as *mut _ as *mut c_void,
        },
        sys::mpv_render_param {
            type_: 0,
            data: ptr::null_mut(),
        },
    ];

    let err = unsafe { sys::mpv_render_context_render(ctx, params.as_mut_ptr()) };
    if err < 0 {
        return Err(format!("mpv SW render 失败: {}", mpv_err_msg(err)));
    }

    let mut rgba = Vec::with_capacity((w * h * 4) as usize);
    for y in 0..h as usize {
        let row = &buf[y * stride..y * stride + (w as usize) * 4];
        for px in row.chunks_exact(4) {
            rgba.extend_from_slice(&[px[0], px[1], px[2], 255]);
        }
    }
    Ok(RgbaFrame {
        width: w,
        height: h,
        rgba,
    })
}

fn mpv_err_msg(err: i32) -> String {
    unsafe {
        let p = sys::mpv_error_string(err);
        if p.is_null() {
            format!("code {err}")
        } else {
            std::ffi::CStr::from_ptr(p).to_string_lossy().into_owned()
        }
    }
}
