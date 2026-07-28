//! 无 libmpv 时的播放后端桩：抽帧预览仍可用，连续播放不可用。

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use eframe::egui::{self, Context, TextureHandle, TextureId, Vec2};

use crate::video_review::domain::VideoItem;

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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FidelityMode {
    #[default]
    Performance,
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

#[derive(Clone)]
pub struct GlowBridge;

impl GlowBridge {
    pub fn from_creation_context(_cc: &eframe::CreationContext<'_>) -> Option<Self> {
        None
    }
}

#[derive(Debug, Clone, Default)]
pub struct SyncClock {
    playing: bool,
    rate: f64,
    origin_ms: u64,
}

impl SyncClock {
    pub fn playing(&self) -> bool {
        self.playing
    }

    pub fn rate(&self) -> f64 {
        self.rate
    }

    pub fn pause_at(&mut self, ms: u64) {
        self.playing = false;
        self.origin_ms = ms;
    }
}

pub struct ComparePlayer {
    clock: SyncClock,
    backend: PlaybackBackendInfo,
    audio_master: Option<i64>,
    ab_loop: bool,
    show_safe_frame: bool,
    rate: f64,
    fidelity: FidelityMode,
}

impl Default for ComparePlayer {
    fn default() -> Self {
        Self {
            clock: SyncClock {
                playing: false,
                rate: 1.0,
                origin_ms: 0,
            },
            backend: PlaybackBackendInfo {
                available: false,
                message: Some("未启用 mpv feature（Windows/CI 默认抽帧预览）".into()),
            },
            audio_master: None,
            ab_loop: false,
            show_safe_frame: false,
            rate: 1.0,
            fidelity: FidelityMode::Performance,
        }
    }
}

impl ComparePlayer {
    pub fn set_glow_bridge(&mut self, _bridge: Arc<GlowBridge>) {}

    pub fn backend_info(&self) -> &PlaybackBackendInfo {
        &self.backend
    }

    pub fn clock(&self) -> &SyncClock {
        &self.clock
    }

    pub fn playing(&self) -> bool {
        self.clock.playing
    }

    pub fn rate(&self) -> f64 {
        self.rate
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
        self.fidelity = mode;
    }

    pub fn audio_master(&self) -> Option<i64> {
        self.audio_master
    }

    pub fn status_label(&self) -> String {
        "抽帧模式（无 libmpv）".into()
    }

    pub fn register_gl_textures(&mut self, _frame: &mut eframe::Frame) {}

    pub fn ensure_probed(&mut self) {
        self.backend.available = false;
    }

    pub fn toggle_play(&mut self, global_ms: u64) {
        if self.clock.playing {
            self.pause(global_ms);
        } else {
            self.clock.playing = true;
            self.clock.origin_ms = global_ms;
        }
    }

    pub fn pause(&mut self, global_ms: u64) {
        self.clock.pause_at(global_ms);
    }

    pub fn set_rate(&mut self, rate: f64) {
        self.rate = rate.clamp(0.25, 4.0);
        self.clock.rate = self.rate;
    }

    pub fn cycle_rate_slower(&mut self) {
        let steps = [0.5, 1.0, 1.5, 2.0];
        let next = steps
            .iter()
            .rev()
            .find(|r| **r < self.rate - 0.01)
            .copied()
            .unwrap_or(0.5);
        self.set_rate(next);
    }

    pub fn cycle_rate_faster(&mut self) {
        let steps = [0.5, 1.0, 1.5, 2.0];
        let next = steps
            .iter()
            .find(|r| **r > self.rate + 0.01)
            .copied()
            .unwrap_or(2.0);
        self.set_rate(next);
    }

    pub fn tick_global_time(&mut self, max_dur: u64, _master_offset_ms: i64) -> u64 {
        if !self.clock.playing {
            return self.clock.origin_ms.min(max_dur);
        }
        self.clock.origin_ms = self.clock.origin_ms.saturating_add(16).min(max_dur);
        self.clock.origin_ms
    }

    pub fn apply_ab_loop(&mut self, global_ms: u64, a_ms: u64, b_ms: u64) -> u64 {
        if self.ab_loop && global_ms >= b_ms && b_ms > a_ms {
            a_ms
        } else {
            global_ms
        }
    }

    pub fn on_user_seek(&mut self, global_ms: u64, _kind: SeekKind) {
        self.clock.origin_ms = global_ms;
    }

    pub fn set_audio_master(&mut self, video_id: i64) {
        self.audio_master = Some(video_id);
    }

    pub fn sync_roster(&mut self, _videos: &[&VideoItem]) {}

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

    pub fn present_with_sizes(
        &mut self,
        _ctx: &Context,
        _videos: &[&VideoItem],
        _global_ms: u64,
        _scrubbing: bool,
        _pane_size: egui::Vec2,
        _size_hints: &HashMap<i64, (u32, u32)>,
    ) {
    }

    pub fn frame_step_all(&mut self, _videos: &[&VideoItem], global_ms: u64, forward: bool) -> u64 {
        if forward {
            global_ms.saturating_add(42)
        } else {
            global_ms.saturating_sub(42)
        }
    }

    pub fn seek_relative(&mut self, global_ms: u64, max_dur: u64, delta_ms: i64) -> u64 {
        let next = if delta_ms >= 0 {
            global_ms.saturating_add(delta_ms as u64)
        } else {
            global_ms.saturating_sub((-delta_ms) as u64)
        };
        next.min(max_dur)
    }

    pub fn pane_texture(&self, _video_id: i64) -> Option<PaneTexture<'_>> {
        None
    }

    pub fn texture(&self, _video_id: i64) -> Option<&TextureHandle> {
        None
    }

    pub fn session_error(&self, _video_id: i64) -> Option<&str> {
        self.backend.message.as_deref()
    }

    pub fn capture_master_rgba(&mut self, _w: u32, _h: u32) -> Option<(i64, RgbaFrame)> {
        None
    }

    pub fn capture_video_rgba(&mut self, _video_id: i64, _w: u32, _h: u32) -> Option<RgbaFrame> {
        None
    }

    pub fn invalidate_video(&mut self, _video_id: i64) {}

    pub fn clear(&mut self) {
        self.audio_master = None;
        self.clock.pause_at(0);
    }
}

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
        let now = Instant::now();
        if self
            .last_at
            .map(|t| now.duration_since(t) >= self.interval)
            .unwrap_or(true)
        {
            self.last_at = Some(now);
            self.last_ms = Some(time_ms);
            true
        } else {
            false
        }
    }
}
