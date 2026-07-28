//! 1–6 路对比播放器：共享 SyncClock + 每路 MpvSession。

use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use eframe::egui::{self, Context, TextureHandle, TextureId, Vec2};

use crate::video_review::domain::VideoItem;
use crate::video_review::ui::multi_compare::MAX_COMPARE_VIDEOS;

use super::glow_bridge::GlowBridge;
use super::mpv_session::{FidelityMode, MpvSession, SOURCE_MAX_H, SOURCE_MAX_W};
use super::present::{PresentFrame, PresentMode, RgbaFrame};
use super::sync_clock::SyncClock;

const TEXTURE_LRU: usize = 8;
const SCRUB_MAX_W: u32 = 960;
const NATIVE_SCRUB_MAX_W: u32 = 1280;
const PLAY_MAX_W: u32 = 1920;
const PERF_IDLE_MAX_W: u32 = 1920;
const RES_UPSCALE_DELAY: Duration = Duration::from_millis(200);

const RATE_STEPS: [f64; 4] = [0.5, 1.0, 1.5, 2.0];

fn perf_max_w(lanes: usize, scrubbing: bool, playing: bool, allow_hires: bool) -> u32 {
    if scrubbing || !allow_hires {
        return SCRUB_MAX_W;
    }
    if playing {
        return match lanes {
            1..=2 => PLAY_MAX_W,
            3..=4 => 1600,
            _ => 1280,
        };
    }
    match lanes {
        1..=2 => PERF_IDLE_MAX_W,
        3..=4 => 1600,
        _ => 1280,
    }
}

fn fit_to_budget(
    src_w: u32,
    src_h: u32,
    budget_w: u32,
    budget_h: u32,
    max_edge: u32,
) -> (u32, u32) {
    let sw = src_w.max(2).min(SOURCE_MAX_W);
    let sh = src_h.max(2).min(SOURCE_MAX_H);
    let bw = budget_w.min(max_edge).max(160);
    let bh = budget_h.min(max_edge).max(90);
    let scale = (bw as f32 / sw as f32).min(bh as f32 / sh as f32).min(1.0);
    (
        ((sw as f32) * scale).round().max(2.0) as u32,
        ((sh as f32) * scale).round().max(2.0) as u32,
    )
}

/// 显示尺寸始终 ≈ 面板物理像素（≤ 片源）；原片不人为再砍，性能档按路数封顶。
fn resolve_display_size(
    mode: FidelityMode,
    src_w: u32,
    src_h: u32,
    budget_w: u32,
    budget_h: u32,
    lanes: usize,
    scrubbing: bool,
    playing: bool,
    allow_hires: bool,
) -> (u32, u32) {
    let sw = src_w.max(2).min(SOURCE_MAX_W);
    let sh = src_h.max(2).min(SOURCE_MAX_H);
    match mode {
        FidelityMode::Native => {
            let max_edge = if !scrubbing && allow_hires {
                SOURCE_MAX_W
            } else {
                NATIVE_SCRUB_MAX_W
            };
            fit_to_budget(sw, sh, budget_w, budget_h, max_edge)
        }
        FidelityMode::Performance => {
            let max_w = perf_max_w(lanes, scrubbing, playing, allow_hires);
            fit_to_budget(sw, sh, budget_w, budget_h, max_w)
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct PlaybackBackendInfo {
    pub available: bool,
    pub message: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SeekKind {
    Scrubbing,
    Committed,
}

#[derive(Clone, Copy)]
pub struct GpuPaneTexture {
    pub id: TextureId,
    pub size: Vec2,
    pub flip_y: bool,
}

pub enum PaneTexture<'a> {
    Cpu(&'a TextureHandle),
    Gpu(GpuPaneTexture),
}

struct GpuView {
    id: TextureId,
    size: Vec2,
    flip_y: bool,
}

pub struct ComparePlayer {
    clock: SyncClock,
    sessions: HashMap<i64, MpvSession>,
    textures: HashMap<i64, TextureHandle>,
    gpu_views: HashMap<i64, GpuView>,
    texture_order: VecDeque<i64>,
    backend: PlaybackBackendInfo,
    probed: bool,
    last_force_seek: bool,
    audio_master: Option<i64>,
    scrub_ended_at: Option<Instant>,
    was_scrubbing: bool,
    ab_loop: bool,
    show_safe_frame: bool,
    present_mode_hint: PresentMode,
    hwdec_hint: &'static str,
    glow: Option<Arc<GlowBridge>>,
    fidelity: FidelityMode,
}

impl Default for ComparePlayer {
    fn default() -> Self {
        Self {
            clock: SyncClock::default(),
            sessions: HashMap::new(),
            textures: HashMap::new(),
            gpu_views: HashMap::new(),
            texture_order: VecDeque::new(),
            backend: PlaybackBackendInfo {
                available: false,
                message: Some("尚未探测 libmpv".into()),
            },
            probed: false,
            last_force_seek: true,
            audio_master: None,
            scrub_ended_at: None,
            was_scrubbing: false,
            ab_loop: false,
            show_safe_frame: false,
            present_mode_hint: PresentMode::Sw,
            hwdec_hint: "—",
            glow: None,
            fidelity: FidelityMode::Performance,
        }
    }
}

impl ComparePlayer {
    pub fn set_glow_bridge(&mut self, bridge: Arc<GlowBridge>) {
        self.glow = Some(bridge);
    }

    pub fn backend_info(&self) -> &PlaybackBackendInfo {
        &self.backend
    }

    pub fn clock(&self) -> &SyncClock {
        &self.clock
    }

    pub fn playing(&self) -> bool {
        self.clock.playing()
    }

    pub fn rate(&self) -> f64 {
        self.clock.rate()
    }

    pub fn ab_loop(&self) -> bool {
        self.ab_loop
    }

    pub fn set_ab_loop(&mut self, on: bool) {
        self.ab_loop = on;
    }

    pub fn show_safe_frame(&self) -> bool {
        self.show_safe_frame
    }

    pub fn toggle_safe_frame(&mut self) {
        self.show_safe_frame = !self.show_safe_frame;
    }

    pub fn fidelity(&self) -> FidelityMode {
        self.fidelity
    }

    pub fn set_fidelity(&mut self, mode: FidelityMode) {
        if self.fidelity != mode {
            self.fidelity = mode;
            self.last_force_seek = true;
            for session in self.sessions.values_mut() {
                session.apply_fidelity(mode);
            }
        }
    }

    pub fn audio_master(&self) -> Option<i64> {
        self.audio_master
    }

    pub fn status_label(&self) -> String {
        if !self.backend.available {
            return "抽帧模式".into();
        }
        // native/perf = 缩放档；出图始终按面板物理像素（GPU 缩放）。
        format!(
            "libmpv · {} · {} · {} · 面板",
            self.hwdec_hint,
            self.present_mode_hint.label(),
            self.fidelity.short_status()
        )
    }

    /// 将尚未注册的 GL 纹理挂到 egui（每帧在 App::update 里调用）。
    pub fn register_gl_textures(&mut self, frame: &mut eframe::Frame) {
        let ids: Vec<i64> = self.sessions.keys().copied().collect();
        for id in ids {
            let Some(session) = self.sessions.get_mut(&id) else {
                continue;
            };
            let Some(gl_p) = session.presenter_mut().as_gl_mut() else {
                continue;
            };
            if let Some((tex, w, h)) = gl_p.take_texture_for_register() {
                let egui_id = frame.register_native_glow_texture(tex);
                gl_p.mark_registered(egui_id);
                self.gpu_views.insert(
                    id,
                    GpuView {
                        id: egui_id,
                        size: Vec2::new(w as f32, h as f32),
                        flip_y: true,
                    },
                );
                self.present_mode_hint = PresentMode::Gl;
            }
        }
    }

    pub fn ensure_probed(&mut self) {
        if self.probed {
            return;
        }
        self.probed = true;
        match super::probe_libmpv() {
            Ok(()) => {
                self.backend = PlaybackBackendInfo {
                    available: true,
                    message: None,
                };
            }
            Err(e) => {
                self.backend = PlaybackBackendInfo {
                    available: false,
                    message: Some(format!(
                        "libmpv 不可用，已回退抽帧预览。安装：brew install mpv（{e}）"
                    )),
                };
            }
        }
    }

    pub fn toggle_play(&mut self, global_ms: u64) {
        self.ensure_probed();
        if !self.backend.available {
            return;
        }
        self.clock.toggle(global_ms);
        let playing = self.clock.playing();
        let rate = self.clock.rate();
        for session in self.sessions.values_mut() {
            session.set_speed(rate);
            if playing {
                session.invalidate_seek_cache();
                session.set_paused(false);
            } else {
                session.set_paused(true);
            }
        }
        self.last_force_seek = !playing;
    }

    pub fn pause(&mut self, global_ms: u64) {
        self.clock.pause_at(global_ms);
        for session in self.sessions.values_mut() {
            session.set_paused(true);
        }
        self.last_force_seek = true;
    }

    pub fn set_rate(&mut self, rate: f64) {
        self.clock.set_rate(rate);
        let r = self.clock.rate();
        for session in self.sessions.values_mut() {
            session.set_speed(r);
        }
    }

    pub fn cycle_rate_slower(&mut self) {
        let cur = self.clock.rate();
        let next = RATE_STEPS
            .iter()
            .rev()
            .find(|&&r| r < cur - 0.01)
            .copied()
            .unwrap_or(RATE_STEPS[0]);
        self.set_rate(next);
    }

    pub fn cycle_rate_faster(&mut self) {
        let cur = self.clock.rate();
        let next = RATE_STEPS
            .iter()
            .find(|&&r| r > cur + 0.01)
            .copied()
            .unwrap_or(*RATE_STEPS.last().unwrap());
        self.set_rate(next);
    }

    /// 推进时钟；播放中优先用主路 time-pos 换算全局时间。
    pub fn tick_global_time(&mut self, max_dur: u64, master_offset_ms: i64) -> u64 {
        if !self.clock.playing() {
            return self.clock.now_ms().min(max_dur);
        }

        let mut t = if let Some(master_id) = self.audio_master {
            if let Some(local) = self.sessions.get(&master_id).and_then(|s| s.time_pos_ms()) {
                let global = (local as i64 - master_offset_ms).max(0) as u64;
                self.clock.sync_from_master(global);
                global
            } else {
                self.clock.now_ms()
            }
        } else {
            self.clock.now_ms()
        };

        t = t.min(max_dur);
        if t >= max_dur && max_dur > 0 {
            self.pause(max_dur);
        }
        t
    }

    /// A-B：越过 B 回到 A（global 时间）。
    pub fn apply_ab_loop(&mut self, global_ms: u64, a_ms: u64, b_ms: u64) -> u64 {
        if !self.ab_loop || b_ms <= a_ms {
            return global_ms;
        }
        if global_ms >= b_ms {
            self.on_user_seek(a_ms, SeekKind::Committed);
            for s in self.sessions.values_mut() {
                s.set_paused(false);
            }
            self.clock.play_from(a_ms);
            return a_ms;
        }
        global_ms
    }

    pub fn on_user_seek(&mut self, global_ms: u64, kind: SeekKind) {
        self.clock.seek_origin(global_ms);
        match kind {
            SeekKind::Scrubbing => {
                if self.clock.playing() {
                    self.pause(global_ms);
                } else {
                    self.last_force_seek = true;
                }
                self.was_scrubbing = true;
            }
            SeekKind::Committed => {
                if self.clock.playing() {
                    self.pause(global_ms);
                } else {
                    self.last_force_seek = true;
                }
                if self.was_scrubbing {
                    self.scrub_ended_at = Some(Instant::now());
                    self.was_scrubbing = false;
                }
            }
        }
    }

    pub fn set_audio_master(&mut self, video_id: i64) {
        self.audio_master = Some(video_id);
        self.apply_mute_state();
    }

    fn apply_mute_state(&mut self) {
        let master = self.audio_master;
        for (id, session) in self.sessions.iter_mut() {
            let muted = master.map(|m| m != *id).unwrap_or(true);
            session.set_muted(muted);
        }
    }

    pub fn sync_roster(&mut self, videos: &[&VideoItem]) {
        self.ensure_probed();
        if !self.backend.available {
            return;
        }

        let want: Vec<(i64, PathBuf)> = videos
            .iter()
            .take(MAX_COMPARE_VIDEOS)
            .map(|v| (v.id, v.file_path.clone()))
            .collect();
        let want_ids: Vec<i64> = want.iter().map(|(id, _)| *id).collect();

        // 不在 roster 内的销毁；同 id 同 path 复用
        self.sessions.retain(|id, _| want_ids.contains(id));
        self.textures.retain(|id, _| want_ids.contains(id));
        self.gpu_views.retain(|id, _| want_ids.contains(id));
        self.texture_order.retain(|id| want_ids.contains(id));

        if let Some(m) = self.audio_master {
            if !want_ids.contains(&m) {
                self.audio_master = want_ids.first().copied();
            }
        } else {
            self.audio_master = want_ids.first().copied();
        }

        for (id, path) in want {
            if let Some(s) = self.sessions.get(&id) {
                if s.path() == path.as_path() {
                    continue;
                }
                self.sessions.remove(&id);
            }
            match MpvSession::open(id, &path, self.glow.as_ref()) {
                Ok(mut session) => {
                    session.set_paused(true);
                    session.set_speed(self.clock.rate());
                    session.apply_fidelity(self.fidelity);
                    self.present_mode_hint = session.present_mode();
                    self.hwdec_hint = session.hwdec_label();
                    self.sessions.insert(id, session);
                    self.last_force_seek = true;
                }
                Err(e) => {
                    self.backend.message = Some(format!("打开视频失败 (#{id}): {e}"));
                }
            }
        }
        self.apply_mute_state();
    }

    pub fn present(
        &mut self,
        ctx: &Context,
        videos: &[&VideoItem],
        global_ms: u64,
        scrubbing: bool,
        pane_size: egui::Vec2,
    ) {
        self.present_with_sizes(
            ctx,
            videos,
            global_ms,
            scrubbing,
            pane_size,
            &HashMap::new(),
        );
    }

    /// `size_hints`：某路优先用指定显示分辨率（如 Solo 全屏）。
    pub fn present_with_sizes(
        &mut self,
        ctx: &Context,
        videos: &[&VideoItem],
        global_ms: u64,
        scrubbing: bool,
        pane_size: egui::Vec2,
        size_hints: &HashMap<i64, (u32, u32)>,
    ) {
        self.ensure_probed();
        if !self.backend.available {
            return;
        }

        self.sync_roster(videos);

        if scrubbing {
            self.was_scrubbing = true;
        } else if self.was_scrubbing {
            self.scrub_ended_at = Some(Instant::now());
            self.was_scrubbing = false;
        }

        let force = scrubbing || !self.clock.playing() || self.last_force_seek;
        let allow_hires = !scrubbing
            && self
                .scrub_ended_at
                .map(|t| t.elapsed() >= RES_UPSCALE_DELAY)
                .unwrap_or(true);
        let ppp = ctx.pixels_per_point().max(1.0);
        // 可见路数（有 size_hints 时只计入镜格）决定性能档封顶。
        let visible_lanes = if size_hints.is_empty() {
            videos.len().max(1)
        } else {
            size_hints.len().max(1)
        };

        let master_id = self.audio_master;

        let mut cpu_updates = Vec::new();
        let mut gpu_updates = Vec::new();
        for video in videos.iter().take(MAX_COMPARE_VIDEOS) {
            let Some(session) = self.sessions.get_mut(&video.id) else {
                continue;
            };
            let on_screen = size_hints.is_empty() || size_hints.contains_key(&video.id);
            let budget_pts = size_hints.get(&video.id).copied().unwrap_or_else(|| {
                let n = visible_lanes as f32;
                let aw = ((pane_size.x / n.sqrt()).max(160.0)).round() as u32;
                let ah = ((aw as f32) * (video.height.max(1) as f32 / video.width.max(1) as f32))
                    .round()
                    .max(90.0) as u32;
                (aw, ah)
            });
            let budget_px = (
                ((budget_pts.0 as f32) * ppp).round() as u32,
                ((budget_pts.1 as f32) * ppp).round() as u32,
            );
            let src_w = if video.width > 0 {
                video.width
            } else {
                budget_px.0.max(640)
            };
            let src_h = if video.height > 0 {
                video.height
            } else {
                budget_px.1.max(360)
            };
            let (dw, dh) = if on_screen {
                resolve_display_size(
                    self.fidelity,
                    src_w,
                    src_h,
                    budget_px.0,
                    budget_px.1,
                    visible_lanes,
                    scrubbing,
                    self.clock.playing(),
                    allow_hires,
                )
            } else {
                (320, 180)
            };
            session.set_display_size(dw, dh);

            let local = video.effective_time_ms(global_ms).min(video.duration_ms);

            if force {
                if scrubbing {
                    session.seek_fast(local);
                } else {
                    session.seek_exact(local);
                }
                session.set_paused(true);
            } else if Some(video.id) == master_id {
                session.set_paused(false);
            } else {
                session.correct_if_drifted(local);
                session.set_paused(false);
            }

            // 仅入镜路每帧出图；后台路只保活 seek，不占满 GPU。
            if !on_screen {
                continue;
            }

            if let Some(frame) = session.render_frame() {
                match frame {
                    PresentFrame::Cpu(rgba) => cpu_updates.push((video.id, rgba)),
                    PresentFrame::Gpu(gl_frame) => {
                        let size = Vec2::new(gl_frame.width as f32, gl_frame.height as f32);
                        let flip_y = gl_frame.flip_y;
                        let egui_id = session
                            .presenter_mut()
                            .as_gl_mut()
                            .and_then(|p| p.egui_texture_id());
                        gpu_updates.push((video.id, egui_id, size, flip_y));
                    }
                }
            }
        }
        for (vid, rgba) in cpu_updates {
            self.store_cpu_texture(ctx, vid, &rgba);
        }
        for (vid, egui_id, size, flip_y) in gpu_updates {
            self.present_mode_hint = PresentMode::Gl;
            if let Some(id) = egui_id {
                self.gpu_views.insert(vid, GpuView { id, size, flip_y });
            } else if let Some(view) = self.gpu_views.get_mut(&vid) {
                view.size = size;
                view.flip_y = flip_y;
            }
        }

        self.last_force_seek = false;

        if self.clock.playing() {
            ctx.request_repaint();
        }
    }

    pub fn frame_step_all(&mut self, videos: &[&VideoItem], global_ms: u64, forward: bool) -> u64 {
        self.pause(global_ms);
        for video in videos.iter().take(MAX_COMPARE_VIDEOS) {
            if let Some(s) = self.sessions.get_mut(&video.id) {
                s.frame_step(forward);
            }
        }
        // 用主路时间回写
        let new_ms = self
            .audio_master
            .and_then(|id| self.sessions.get(&id))
            .and_then(|s| s.time_pos_ms())
            .unwrap_or(global_ms);
        self.clock.pause_at(new_ms);
        self.last_force_seek = true;
        new_ms
    }

    pub fn seek_relative(&mut self, global_ms: u64, max_dur: u64, delta_ms: i64) -> u64 {
        let t = (global_ms as i64 + delta_ms).clamp(0, max_dur as i64) as u64;
        self.on_user_seek(t, SeekKind::Committed);
        t
    }

    pub fn pane_texture(&self, video_id: i64) -> Option<PaneTexture<'_>> {
        if let Some(g) = self.gpu_views.get(&video_id) {
            return Some(PaneTexture::Gpu(GpuPaneTexture {
                id: g.id,
                size: g.size,
                flip_y: g.flip_y,
            }));
        }
        self.textures.get(&video_id).map(PaneTexture::Cpu)
    }

    pub fn texture(&self, video_id: i64) -> Option<&TextureHandle> {
        self.textures.get(&video_id)
    }

    pub fn session_error(&self, video_id: i64) -> Option<&str> {
        self.sessions.get(&video_id).and_then(|s| s.last_error())
    }

    pub fn capture_master_rgba(&mut self, w: u32, h: u32) -> Option<(i64, RgbaFrame)> {
        let id = self.audio_master?;
        let frame = self.capture_video_rgba(id, w, h)?;
        Some((id, frame))
    }

    /// 从指定路截当前帧（示波器联动）；失败返回 None，调用方回退 ffmpeg。
    pub fn capture_video_rgba(&mut self, video_id: i64, w: u32, h: u32) -> Option<RgbaFrame> {
        self.sessions.get_mut(&video_id)?.capture_rgba(w, h)
    }

    fn store_cpu_texture(&mut self, ctx: &Context, video_id: i64, frame: &RgbaFrame) {
        let size = [frame.width as usize, frame.height as usize];
        let color = egui::ColorImage::from_rgba_unmultiplied(size, &frame.rgba);
        if let Some(tex) = self.textures.get_mut(&video_id) {
            tex.set(color, egui::TextureOptions::LINEAR);
        } else {
            let tex = ctx.load_texture(
                format!("mpv_frame_{video_id}"),
                color,
                egui::TextureOptions::LINEAR,
            );
            self.textures.insert(video_id, tex);
            self.texture_order.push_back(video_id);
            while self.texture_order.len() > TEXTURE_LRU {
                if let Some(old) = self.texture_order.pop_front() {
                    if old != video_id {
                        self.textures.remove(&old);
                    }
                }
            }
        }
        self.texture_order.retain(|id| *id != video_id);
        self.texture_order.push_back(video_id);
    }

    pub fn invalidate_video(&mut self, video_id: i64) {
        if let Some(s) = self.sessions.get_mut(&video_id) {
            s.invalidate_seek_cache();
        }
        self.last_force_seek = true;
    }

    pub fn clear(&mut self) {
        self.sessions.clear();
        self.textures.clear();
        self.gpu_views.clear();
        self.texture_order.clear();
        self.audio_master = None;
        self.clock.pause_at(0);
    }
}

/// 示波器采样节流：播放中不要每帧抽帧。
#[derive(Debug, Clone)]
pub struct ScopeSampleThrottle {
    last_at: Option<Instant>,
    interval: Duration,
    last_ms: Option<u64>,
}

impl Default for ScopeSampleThrottle {
    fn default() -> Self {
        Self {
            last_at: None,
            interval: Duration::from_millis(150),
            last_ms: None,
        }
    }
}

impl ScopeSampleThrottle {
    pub fn with_interval(ms: u64) -> Self {
        Self {
            interval: Duration::from_millis(ms.max(50)),
            ..Default::default()
        }
    }

    pub fn last_sampled_ms(&self) -> Option<u64> {
        self.last_ms
    }

    pub fn should_sample(&mut self, time_ms: u64, playing: bool, scrubbing: bool) -> bool {
        if scrubbing || !playing {
            self.last_at = Some(Instant::now());
            self.last_ms = Some(time_ms);
            return true;
        }
        if self.last_ms == Some(time_ms) {
            return false;
        }
        let due = self
            .last_at
            .map(|t| t.elapsed() >= self.interval)
            .unwrap_or(true);
        if due {
            self.last_at = Some(Instant::now());
            self.last_ms = Some(time_ms);
            true
        } else {
            false
        }
    }
}
