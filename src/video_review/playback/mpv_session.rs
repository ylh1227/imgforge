//! 单路 libmpv 会话：load / seek / pause / mute / frame-step / GL|SW 出帧。

use std::path::{Path, PathBuf};
use std::sync::Arc;

use libmpv2::Mpv;

use super::glow_bridge::GlowBridge;
use super::present::{PresentFrame, PresentMode, Presenter, RgbaFrame};

const DRIFT_MS: u64 = 60;
/// 出帧上限（防异常元数据 / 面板物理像素封顶）。
pub const SOURCE_MAX_W: u32 = 7680;
pub const SOURCE_MAX_H: u32 = 4320;

/// 出帧清晰度：性能（默认）或原片。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FidelityMode {
    /// 按面板预算出帧，缩放更省，多路更流畅。
    #[default]
    Performance,
    /// 片源色彩 + GPU 高质量缩放到面板物理像素（非整幅片源 FBO）。
    Native,
}

impl FidelityMode {
    pub fn label(self) -> &'static str {
        match self {
            Self::Performance => "性能",
            Self::Native => "原片",
        }
    }

    pub fn short_status(self) -> &'static str {
        match self {
            Self::Performance => "perf",
            Self::Native => "native",
        }
    }
}

pub struct MpvSession {
    #[allow(dead_code)]
    pub video_id: i64,
    path: PathBuf,
    mpv: Mpv,
    presenter: Presenter,
    last_seek_ms: Option<u64>,
    last_seek_fast: bool,
    display_w: u32,
    display_h: u32,
    last_error: Option<String>,
    muted: bool,
    hwdec_label: &'static str,
    fidelity: FidelityMode,
}

impl MpvSession {
    pub fn open(
        video_id: i64,
        path: &Path,
        glow: Option<&Arc<GlowBridge>>,
    ) -> Result<Self, String> {
        let (hwdec, hwdec_label) = if cfg!(target_os = "macos") {
            ("videotoolbox", "VT")
        } else {
            ("auto", "HW")
        };

        let mpv = Mpv::with_initializer(|init| {
            init.set_option("vo", "libmpv")?;
            init.set_option("hwdec", hwdec)?;
            init.set_option("keep-open", "yes")?;
            init.set_option("idle", "yes")?;
            init.set_option("pause", "yes")?;
            init.set_option("mute", "yes")?;
            init.set_option("osc", "no")?;
            init.set_option("osd-level", "0")?;
            init.set_option("input-default-bindings", "no")?;
            init.set_option("input-vo-keyboard", "no")?;
            init.set_option("hr-seek", "yes")?;
            let _ = init.set_option("video-timing-offset", "0");
            let _ = init.set_option("cache", "yes");
            let _ = init.set_option("demuxer-max-bytes", "64MiB");
            let _ = init.set_option("demuxer-readahead-secs", "2");
            // 色彩保真（两档共用）；缩放默认性能档，打开后再 apply_fidelity。
            let _ = init.set_option("video-output-levels", "full");
            let _ = init.set_option("tone-mapping", "auto");
            let _ = init.set_option("gamut-mapping-mode", "auto");
            let _ = init.set_option("target-colorspace-hint", "yes");
            let _ = init.set_option("icc-profile-auto", "yes");
            let _ = init.set_option("scale", "lanczos");
            let _ = init.set_option("cscale", "lanczos");
            let _ = init.set_option("dscale", "bilinear");
            let _ = init.set_option("correct-downscaling", "yes");
            let _ = init.set_option("linear-downscaling", "no");
            let _ = init.set_option("sigmoid-upscaling", "yes");
            let _ = init.set_option("dither-depth", "auto");
            let _ = init.set_option("deband", "no");
            Ok(())
        })
        .map_err(|e| format!("创建 libmpv 失败: {e}"))?;

        let path_str = path.to_string_lossy();
        mpv.command("loadfile", &[&path_str, "replace"])
            .map_err(|e| format!("loadfile 失败: {e}"))?;

        for _ in 0..50 {
            let _ = mpv.wait_event(0.02);
            if mpv.get_property::<i64>("video-params/w").is_ok()
                || mpv.get_property::<f64>("duration").unwrap_or(0.0) > 0.0
            {
                break;
            }
        }

        let presenter = Presenter::create(&mpv, glow)?;

        Ok(Self {
            video_id,
            path: path.to_path_buf(),
            mpv,
            presenter,
            last_seek_ms: None,
            last_seek_fast: false,
            display_w: 640,
            display_h: 360,
            last_error: None,
            muted: true,
            hwdec_label,
            fidelity: FidelityMode::Performance,
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn last_error(&self) -> Option<&str> {
        self.last_error.as_deref()
    }

    pub fn present_mode(&self) -> PresentMode {
        self.presenter.mode()
    }

    pub fn hwdec_label(&self) -> &'static str {
        self.hwdec_label
    }

    pub fn set_display_size(&mut self, w: u32, h: u32) {
        self.display_w = w.max(2).min(SOURCE_MAX_W);
        self.display_h = h.max(2).min(SOURCE_MAX_H);
    }

    /// 按画质档切换 GPU 缩放（原片高质量 / 性能省算力）。
    pub fn apply_fidelity(&mut self, mode: FidelityMode) {
        self.fidelity = mode;
        match mode {
            FidelityMode::Native => {
                let _ = self.mpv.set_property("scale", "ewa_lanczossharp");
                let _ = self.mpv.set_property("cscale", "ewa_lanczossharp");
                let _ = self.mpv.set_property("dscale", "mitchell");
                let _ = self.mpv.set_property("linear-downscaling", true);
                let _ = self.mpv.set_property("correct-downscaling", true);
                let _ = self.mpv.set_property("sigmoid-upscaling", true);
            }
            FidelityMode::Performance => {
                let _ = self.mpv.set_property("scale", "bilinear");
                let _ = self.mpv.set_property("cscale", "bilinear");
                let _ = self.mpv.set_property("dscale", "bilinear");
                let _ = self.mpv.set_property("linear-downscaling", false);
                let _ = self.mpv.set_property("correct-downscaling", true);
                let _ = self.mpv.set_property("sigmoid-upscaling", false);
            }
        }
    }

    pub fn set_paused(&mut self, paused: bool) {
        let _ = self.mpv.set_property("pause", paused);
    }

    pub fn set_speed(&mut self, rate: f64) {
        let _ = self.mpv.set_property("speed", rate.clamp(0.25, 4.0));
    }

    pub fn set_muted(&mut self, muted: bool) {
        self.muted = muted;
        let _ = self.mpv.set_property("mute", muted);
    }

    pub fn time_pos_ms(&self) -> Option<u64> {
        let secs: f64 = self.mpv.get_property("time-pos").ok()?;
        Some((secs * 1000.0).round().max(0.0) as u64)
    }

    pub fn seek_fast(&mut self, time_ms: u64) {
        if self.last_seek_ms == Some(time_ms) && self.last_seek_fast {
            return;
        }
        let secs = time_ms as f64 / 1000.0;
        match self
            .mpv
            .command("seek", &[&format!("{secs:.6}"), "absolute+keyframes"])
        {
            Ok(()) => {
                self.last_seek_ms = Some(time_ms);
                self.last_seek_fast = true;
                for _ in 0..4 {
                    let _ = self.mpv.wait_event(0.0);
                }
            }
            Err(e) => self.last_error = Some(format!("seek 失败: {e}")),
        }
    }

    pub fn seek_exact(&mut self, time_ms: u64) {
        if self.last_seek_ms == Some(time_ms) && !self.last_seek_fast {
            return;
        }
        let secs = time_ms as f64 / 1000.0;
        match self
            .mpv
            .command("seek", &[&format!("{secs:.6}"), "absolute+exact"])
        {
            Ok(()) => {
                self.last_seek_ms = Some(time_ms);
                self.last_seek_fast = false;
                for _ in 0..8 {
                    let _ = self.mpv.wait_event(0.0);
                }
            }
            Err(e) => self.last_error = Some(format!("seek 失败: {e}")),
        }
    }

    pub fn frame_step(&mut self, forward: bool) {
        self.set_paused(true);
        let cmd = if forward {
            "frame-step"
        } else {
            "frame-back-step"
        };
        if let Err(e) = self.mpv.command(cmd, &[]) {
            self.last_error = Some(format!("{cmd} 失败: {e}"));
        } else {
            self.invalidate_seek_cache();
            for _ in 0..8 {
                let _ = self.mpv.wait_event(0.0);
            }
        }
    }

    pub fn correct_if_drifted(&mut self, target_ms: u64) {
        let Some(pos) = self.time_pos_ms() else {
            self.seek_exact(target_ms);
            return;
        };
        if pos.abs_diff(target_ms) > DRIFT_MS {
            self.invalidate_seek_cache();
            self.seek_exact(target_ms);
        }
    }

    pub fn invalidate_seek_cache(&mut self) {
        self.last_seek_ms = None;
        self.last_seek_fast = false;
    }

    pub fn render_frame(&mut self) -> Option<PresentFrame> {
        match self.presenter.render(self.display_w, self.display_h) {
            Ok(frame) => {
                self.last_error = None;
                Some(frame)
            }
            Err(e) => {
                self.last_error = Some(e);
                None
            }
        }
    }

    pub fn presenter_mut(&mut self) -> &mut Presenter {
        &mut self.presenter
    }

    pub fn capture_rgba(&mut self, w: u32, h: u32) -> Option<RgbaFrame> {
        match self.presenter.capture_rgba(w.max(2), h.max(2)) {
            Ok(f) => {
                self.last_error = None;
                Some(f)
            }
            Err(e) => {
                self.last_error = Some(e);
                None
            }
        }
    }
}
