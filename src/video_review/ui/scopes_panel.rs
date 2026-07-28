//! 视频示波器侧栏：当前帧异步渲染 + 整段/片段聚合。

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use eframe::egui::{self, Color32, Context, RichText, TextureHandle, Ui, Vec2};

use crate::gui::{theme, widgets, BackgroundJob, JobContext};
use crate::ui::progress::ProgressReporter;
use crate::video_review::domain::VideoItem;
use crate::video_review::playback::RgbaFrame;
use crate::video_review::scopes::{
    sample_timestamps, AggregateAccumulator, AggregateCacheKey, AggregateRange, AggregateRangeMode,
    AggregatedScope, HistogramMode, HistogramScale, ScopeCacheKey, ScopeEngine, ScopeKind,
    ScopeOptions, ScopeRequest, ScopeRgba, ScopeViewMode, WaveformMode, AGG_FRAME_WIDTH,
};
use crate::video_review::service::ffmpeg_backend::{FfmpegBackend, VideoBackend};
use crate::video_review::service::frame_cache::FrameCache;
use crate::video_review::service::VideoReviewService;
use crate::video_review::ui::multi_compare::format_ms;

const SCOPE_PANEL_WIDTH: f32 = 380.0;
/// 侧栏固定标签列（短中文：视图/类型/模式/范围）。
const SCOPE_LABEL_W: f32 = 44.0;
const FRAME_FETCH_W: u32 = 640;

const OUT_IDLE_W: u32 = 640;
const OUT_IDLE_H: u32 = 360;
const OUT_SCRUB_W: u32 = 320;
const OUT_SCRUB_H: u32 = 180;
const OUT_AGG_W: u32 = 640;
const OUT_AGG_H: u32 = 360;
const EDGE_IDLE: u32 = 1280;
const EDGE_SCRUB: u32 = 640;

/// 拖动期间最少间隔多久发起一次新渲染。
const SCRUB_SPAWN_INTERVAL: Duration = Duration::from_millis(80);

enum PendingFrameSource {
    /// 磁盘预览帧；分析优先 lossless 再抽。
    Disk {
        frame_path: PathBuf,
        video_path: PathBuf,
        local_time_ms: u64,
    },
    /// 播放器当前帧（mpv 截帧，跳过 ffmpeg）。
    Live {
        rgba: Arc<[u8]>,
        width: u32,
        height: u32,
    },
}

struct PendingRender {
    key: ScopeCacheKey,
    source: PendingFrameSource,
    kind: ScopeKind,
    options: ScopeOptions,
    out_w: u32,
    out_h: u32,
    max_input_edge: u32,
}

struct RenderOk {
    key: ScopeCacheKey,
    image: ScopeRgba,
}

struct AggregateOk {
    key: AggregateCacheKey,
    image: ScopeRgba,
    meta: AggregatedScope,
}

pub struct ScopesPanel {
    pub enabled: bool,
    kind: ScopeKind,
    options: ScopeOptions,
    view_mode: ScopeViewMode,
    range_mode: AggregateRangeMode,
    engine: Option<Arc<ScopeEngine>>,
    init_job: BackgroundJob<Arc<ScopeEngine>>,
    init_error: Option<String>,
    init_started: bool,
    render_job: BackgroundJob<RenderOk>,
    /// 最新期望渲染（latest-wins）。
    pending: Option<PendingRender>,
    in_flight_key: Option<ScopeCacheKey>,
    last_spawn_at: Option<Instant>,
    texture: Option<TextureHandle>,
    cache_key: Option<ScopeCacheKey>,
    last_error: Option<String>,
    busy: bool,
    // 聚合
    aggregate_job: BackgroundJob<AggregateOk>,
    aggregate_cache_key: Option<AggregateCacheKey>,
    aggregate_meta: Option<AggregatedScope>,
    aggregate_in_flight: Option<AggregateCacheKey>,
    /// 外部「用作聚合范围」请求。
    pending_inout: Option<(u64, u64)>,
}

impl Default for ScopesPanel {
    fn default() -> Self {
        Self {
            enabled: false,
            kind: ScopeKind::Histogram,
            options: ScopeOptions::default(),
            view_mode: ScopeViewMode::Current,
            range_mode: AggregateRangeMode::Full,
            engine: None,
            init_job: BackgroundJob::default(),
            init_error: None,
            init_started: false,
            render_job: BackgroundJob::default(),
            pending: None,
            in_flight_key: None,
            last_spawn_at: None,
            texture: None,
            cache_key: None,
            last_error: None,
            busy: false,
            aggregate_job: BackgroundJob::default(),
            aggregate_cache_key: None,
            aggregate_meta: None,
            aggregate_in_flight: None,
            pending_inout: None,
        }
    }
}

impl ScopesPanel {
    pub fn panel_width(&self) -> f32 {
        if self.enabled {
            SCOPE_PANEL_WIDTH
        } else {
            0.0
        }
    }

    /// 片段列表「用作聚合范围」。
    pub fn use_aggregate_range(&mut self, start_ms: u64, end_ms: u64) {
        self.enabled = true;
        self.view_mode = ScopeViewMode::Aggregate;
        self.range_mode = AggregateRangeMode::InOut;
        self.pending_inout = Some((start_ms, end_ms));
        self.aggregate_cache_key = None;
        self.aggregate_meta = None;
    }

    pub fn take_pending_inout(&mut self) -> Option<(u64, u64)> {
        self.pending_inout.take()
    }

    pub fn view_is_current(&self) -> bool {
        self.view_mode == ScopeViewMode::Current
    }

    pub fn ui(
        &mut self,
        ctx: &Context,
        ui: &mut Ui,
        service: &VideoReviewService,
        source: Option<&VideoItem>,
        time_ms: u64,
        scrubbing: bool,
        in_out: (u64, u64),
        // 播放器当前帧；有则优先分析，跳过 ffmpeg 抽帧。
        live_rgba: Option<&RgbaFrame>,
    ) {
        if self.enabled {
            self.ensure_engine_async(ctx);
            self.poll_jobs(ctx);
        }

        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 6.0;

            let mut plot_hint: Option<&str> = None;
            if let Some(err) = &self.init_error {
                ui.colored_label(Color32::from_rgb(220, 80, 80), err);
                plot_hint = Some("GPU 不可用");
            } else if self.engine.is_none() {
                plot_hint = Some("GPU 初始化中…");
            } else if source.is_none() {
                plot_hint = Some("选择视频后显示示波器");
            } else if let Some(video) = source {
                match self.view_mode {
                    ScopeViewMode::Current => {
                        if self.aggregate_job.is_running() {
                            self.aggregate_job.request_cancel();
                        }
                        if let Some(frame) = live_rgba {
                            self.request_render_rgba(ctx, video, time_ms, frame, scrubbing);
                            if self.texture.is_none() {
                                plot_hint = Some("分析中…");
                            }
                        } else if self.view_cache_fresh(video.id, time_ms, scrubbing) {
                            // 已有同参纹理：跳过反复 ffmpeg。
                        } else {
                            let frame_path = service
                                .frame_at(video, time_ms, FRAME_FETCH_W)
                                .ok()
                                .flatten();
                            if let Some(path) = frame_path {
                                self.request_render(ctx, video, time_ms, &path, scrubbing);
                                if self.texture.is_none() {
                                    plot_hint = Some("抽帧中…");
                                }
                            } else {
                                plot_hint = Some("抽帧中…");
                            }
                        }
                    }
                    ScopeViewMode::Aggregate => {
                        self.pending = None;
                        let range = match self.range_mode {
                            AggregateRangeMode::Full => AggregateRange::full(video.duration_ms),
                            AggregateRangeMode::InOut => AggregateRange {
                                start_ms: in_out.0,
                                end_ms: in_out.1,
                            }
                            .clamped(video.duration_ms),
                        };
                        if !range.is_valid() {
                            ui.colored_label(
                                Color32::from_rgb(220, 80, 80),
                                "In-Out 无效：结束时间需大于开始时间",
                            );
                            plot_hint = Some("范围无效");
                        } else {
                            self.request_aggregate(ctx, video, range);
                            if self.texture.is_none() && self.aggregate_job.is_running() {
                                plot_hint = Some("聚合采样中…");
                            } else if self.texture.is_none() {
                                plot_hint = Some("准备聚合…");
                            }
                        }
                    }
                }
            }

            if let Some(err) = &self.last_error {
                ui.colored_label(Color32::from_rgb(220, 80, 80), err);
            }

            self.plot_area(ui, plot_hint);
        });
    }

    /// 控件条：放在时间轴区「示波器」开关下方（左侧），不占右侧波形高度。
    pub fn controls_ui(&mut self, ui: &mut Ui, in_out: (u64, u64)) {
        ui.spacing_mut().item_spacing.y = 6.0;
        if self.busy {
            ui.label(RichText::new("更新中…").weak().size(11.0));
        }

        scope_labeled_row(ui, "视图", |ui| {
            if ui
                .selectable_label(self.view_mode == ScopeViewMode::Current, "当前帧")
                .clicked()
            {
                self.view_mode = ScopeViewMode::Current;
            }
            if ui
                .selectable_label(self.view_mode == ScopeViewMode::Aggregate, "聚合")
                .clicked()
            {
                self.view_mode = ScopeViewMode::Aggregate;
            }
        });

        scope_labeled_row(ui, "类型", |ui| {
            let w = ui.available_width().max(120.0);
            egui::ComboBox::from_id_salt("scope_kind")
                .width(w)
                .selected_text(self.kind.label())
                .show_ui(ui, |ui| {
                    for kind in [
                        ScopeKind::Histogram,
                        ScopeKind::Waveform,
                        ScopeKind::Vectorscope,
                    ] {
                        if ui
                            .selectable_value(&mut self.kind, kind, kind.label())
                            .changed()
                        {
                            self.invalidate_aggregate_cache();
                        }
                    }
                });
        });

        self.options_ui(ui);

        if self.view_mode == ScopeViewMode::Aggregate {
            scope_labeled_row(ui, "范围", |ui| {
                if ui
                    .selectable_label(self.range_mode == AggregateRangeMode::Full, "整段")
                    .clicked()
                {
                    self.range_mode = AggregateRangeMode::Full;
                    self.invalidate_aggregate_cache();
                }
                if ui
                    .selectable_label(self.range_mode == AggregateRangeMode::InOut, "In-Out")
                    .clicked()
                {
                    self.range_mode = AggregateRangeMode::InOut;
                    self.invalidate_aggregate_cache();
                }
            });
            if self.range_mode == AggregateRangeMode::InOut {
                ui.label(
                    RichText::new(format!("{} – {}", format_ms(in_out.0), format_ms(in_out.1)))
                        .weak()
                        .size(11.0),
                );
            }
            if let Some(meta) = &self.aggregate_meta {
                ui.label(
                    RichText::new(format!(
                        "聚合 · {} 帧{} · {}–{}",
                        meta.sample_count,
                        if meta.skipped > 0 {
                            format!("（跳过 {}）", meta.skipped)
                        } else {
                            String::new()
                        },
                        format_ms(meta.range.start_ms),
                        format_ms(meta.range.end_ms)
                    ))
                    .weak()
                    .size(11.0),
                );
            }
            if self.aggregate_job.is_running() {
                if let Some(p) = self.aggregate_job.progress() {
                    let frac = p.fraction();
                    let done = p.completed.load(std::sync::atomic::Ordering::Relaxed);
                    let total = p.total.load(std::sync::atomic::Ordering::Relaxed).max(1);
                    ui.add(
                        egui::ProgressBar::new(frac)
                            .desired_width(ui.available_width())
                            .text(format!("采样 {done}/{total}")),
                    );
                }
                if ui.small_button("取消").clicked() {
                    self.aggregate_job.request_cancel();
                }
            }
        }
    }

    fn invalidate_aggregate_cache(&mut self) {
        self.aggregate_cache_key = None;
        self.aggregate_meta = None;
    }

    /// 波形区占满剩余高度；无图时画等高占位，避免侧栏空洞。
    fn plot_area(&self, ui: &mut Ui, placeholder: Option<&str>) {
        let avail = ui.available_size();
        let h = avail.y.max(180.0);
        let w = avail.x.max(64.0);
        let (rect, _) = ui.allocate_exact_size(egui::vec2(w, h), egui::Sense::hover());
        let bg = ui.visuals().extreme_bg_color;
        ui.painter().rect_filled(rect, 6.0, bg);

        if let Some(tex) = &self.texture {
            let size = fit_size(tex.size_vec2(), rect.size());
            let offset = rect.center() - size * 0.5;
            let img_rect = egui::Rect::from_min_size(offset, size);
            ui.painter().image(
                tex.id(),
                img_rect,
                egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                Color32::WHITE,
            );
        } else if let Some(msg) = placeholder {
            ui.painter().text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                msg,
                egui::FontId::proportional(13.0),
                ui.visuals().weak_text_color(),
            );
        }
    }

    fn ensure_engine_async(&mut self, ctx: &Context) {
        if self.engine.is_some() || self.init_error.is_some() {
            return;
        }
        if !self.init_started {
            self.init_started = true;
            self.init_job
                .spawn(ctx, 1, |_| ScopeEngine::try_new().map(Arc::new));
        }
    }

    fn poll_jobs(&mut self, ctx: &Context) {
        if let Some(result) = self.init_job.poll(ctx) {
            match result {
                Ok(engine) => {
                    self.engine = Some(engine);
                    self.init_error = None;
                }
                Err(e) => {
                    self.init_error = Some(e);
                }
            }
        }

        if let Some(result) = self.render_job.poll(ctx) {
            self.in_flight_key = None;
            match result {
                Ok(ok) => {
                    if self.view_mode == ScopeViewMode::Current {
                        self.apply_texture(ctx, &ok.key, &ok.image);
                        self.cache_key = Some(ok.key);
                        self.last_error = None;
                    }
                }
                Err(e) => {
                    if self.view_mode == ScopeViewMode::Current {
                        self.last_error = Some(e);
                    }
                }
            }
        }

        if let Some(result) = self.aggregate_job.poll(ctx) {
            self.aggregate_in_flight = None;
            match result {
                Ok(ok) => {
                    if self.view_mode == ScopeViewMode::Aggregate {
                        self.apply_agg_texture(ctx, &ok);
                        self.aggregate_cache_key = Some(ok.key);
                        self.aggregate_meta = Some(ok.meta);
                        self.last_error = None;
                    }
                }
                Err(e) => {
                    if e.contains("取消") {
                        // 用户取消不刷红字。
                    } else if self.view_mode == ScopeViewMode::Aggregate {
                        self.last_error = Some(e);
                    }
                }
            }
        }

        self.try_spawn_pending(ctx);
        self.busy = self.render_job.is_running()
            || self.pending.is_some()
            || self.aggregate_job.is_running();
    }

    fn apply_texture(&mut self, ctx: &Context, key: &ScopeCacheKey, image: &ScopeRgba) {
        let size = [image.width as usize, image.height as usize];
        let color = egui::ColorImage::from_rgba_unmultiplied(size, &image.rgba);
        let tex = ctx.load_texture(
            format!("video_scope_{}_{}", key.video_id, key.kind.label()),
            color,
            egui::TextureOptions::LINEAR,
        );
        self.texture = Some(tex);
    }

    fn apply_agg_texture(&mut self, ctx: &Context, ok: &AggregateOk) {
        let size = [ok.image.width as usize, ok.image.height as usize];
        let color = egui::ColorImage::from_rgba_unmultiplied(size, &ok.image.rgba);
        let tex = ctx.load_texture(
            format!(
                "video_scope_agg_{}_{}_{}_{}",
                ok.key.video_id,
                ok.key.start_ms,
                ok.key.end_ms,
                ok.key.kind.label()
            ),
            color,
            egui::TextureOptions::LINEAR,
        );
        self.texture = Some(tex);
    }

    fn view_dims(scrubbing: bool) -> (u32, u32, u32) {
        if scrubbing {
            (OUT_SCRUB_W, OUT_SCRUB_H, EDGE_SCRUB)
        } else {
            (OUT_IDLE_W, OUT_IDLE_H, EDGE_IDLE)
        }
    }

    /// 当前纹理是否已覆盖同一视频/时刻/参数（可不重复抽帧）。
    fn view_cache_fresh(&self, video_id: i64, time_ms: u64, scrubbing: bool) -> bool {
        let (out_w, out_h, _) = Self::view_dims(scrubbing);
        let Some(k) = &self.cache_key else {
            return false;
        };
        k.video_id == video_id
            && k.time_ms == time_ms
            && k.kind == self.kind
            && k.out_w == out_w
            && k.out_h == out_h
            && k.histogram_mode == self.options.histogram_mode as u8
            && k.histogram_scale == self.options.histogram_scale as u8
            && k.waveform_mode == self.options.waveform_mode as u8
            && k.vectorscope_75_box == self.options.vectorscope_75_box
            && self.texture.is_some()
    }

    fn enqueue_pending(
        &mut self,
        ctx: &Context,
        key: ScopeCacheKey,
        source: PendingFrameSource,
        scrubbing: bool,
        out_w: u32,
        out_h: u32,
        max_input_edge: u32,
    ) {
        if self.cache_key.as_ref() == Some(&key) {
            if self.pending.as_ref().map(|p| &p.key) == Some(&key) {
                self.pending = None;
            }
            return;
        }
        if self.in_flight_key.as_ref() == Some(&key) {
            return;
        }
        if self.pending.as_ref().map(|p| &p.key) == Some(&key) {
            return;
        }

        self.pending = Some(PendingRender {
            key,
            source,
            kind: self.kind,
            options: self.options.clone(),
            out_w,
            out_h,
            max_input_edge,
        });

        if !scrubbing {
            self.try_spawn_pending(ctx);
        } else {
            let due = self
                .last_spawn_at
                .map(|t| t.elapsed() >= SCRUB_SPAWN_INTERVAL)
                .unwrap_or(true);
            if due {
                self.try_spawn_pending(ctx);
            } else {
                ctx.request_repaint_after(SCRUB_SPAWN_INTERVAL);
            }
        }
    }

    fn request_render(
        &mut self,
        ctx: &Context,
        video: &VideoItem,
        time_ms: u64,
        frame_path: &PathBuf,
        scrubbing: bool,
    ) {
        let (out_w, out_h, max_input_edge) = Self::view_dims(scrubbing);
        let key = ScopeCacheKey::from_parts(
            video.id,
            time_ms,
            self.kind,
            &self.options,
            out_w,
            out_h,
            &frame_path.to_string_lossy(),
        );
        let local_t = video.effective_time_ms(time_ms).min(video.duration_ms);
        self.enqueue_pending(
            ctx,
            key,
            PendingFrameSource::Disk {
                frame_path: frame_path.clone(),
                video_path: video.file_path.clone(),
                local_time_ms: local_t,
            },
            scrubbing,
            out_w,
            out_h,
            max_input_edge,
        );
    }

    fn request_render_rgba(
        &mut self,
        ctx: &Context,
        video: &VideoItem,
        time_ms: u64,
        frame: &RgbaFrame,
        scrubbing: bool,
    ) {
        if frame.width < 2 || frame.height < 2 || frame.rgba.len() < 16 {
            return;
        }
        let (out_w, out_h, max_input_edge) = Self::view_dims(scrubbing);
        let tag = format!("mpv://{}/{}", video.id, time_ms);
        let key = ScopeCacheKey::from_parts(
            video.id,
            time_ms,
            self.kind,
            &self.options,
            out_w,
            out_h,
            &tag,
        );
        self.enqueue_pending(
            ctx,
            key,
            PendingFrameSource::Live {
                rgba: Arc::from(frame.rgba.as_slice()),
                width: frame.width,
                height: frame.height,
            },
            scrubbing,
            out_w,
            out_h,
            max_input_edge,
        );
    }

    fn request_aggregate(&mut self, ctx: &Context, video: &VideoItem, range: AggregateRange) {
        let timestamps = sample_timestamps(range);
        let sample_n = timestamps.len();
        let key = AggregateCacheKey::from_parts(
            video.id,
            range,
            self.kind,
            &self.options,
            sample_n,
            OUT_AGG_W,
            OUT_AGG_H,
        );

        if self.aggregate_cache_key.as_ref() == Some(&key) && self.texture.is_some() {
            return;
        }
        if self.aggregate_in_flight.as_ref() == Some(&key) {
            return;
        }
        if self.aggregate_job.is_running() {
            // 参数变了则取消旧任务，下一帧再开新任务。
            if self.aggregate_in_flight.as_ref() != Some(&key) {
                self.aggregate_job.request_cancel();
            }
            return;
        }

        let Some(engine) = self.engine.clone() else {
            return;
        };

        self.aggregate_in_flight = Some(key.clone());
        self.busy = true;
        let video_path = video.file_path.clone();
        let kind = self.kind;
        let options = self.options.clone();

        self.aggregate_job
            .spawn_with_context(ctx, sample_n, move |job: JobContext| {
                run_aggregate_job(
                    job, engine, video_path, range, timestamps, kind, options, key,
                )
            });
    }

    fn try_spawn_pending(&mut self, ctx: &Context) {
        if self.view_mode != ScopeViewMode::Current {
            return;
        }
        if self.render_job.is_running() {
            return;
        }
        let Some(engine) = self.engine.clone() else {
            return;
        };
        let Some(pending) = self.pending.take() else {
            return;
        };

        self.in_flight_key = Some(pending.key.clone());
        self.last_spawn_at = Some(Instant::now());
        self.busy = true;

        let PendingRender {
            key,
            source,
            kind,
            options,
            out_w,
            out_h,
            max_input_edge,
        } = pending;

        self.render_job.spawn(ctx, 1, move |_| {
            let img = match source {
                PendingFrameSource::Live {
                    rgba,
                    width,
                    height,
                } => image::RgbaImage::from_raw(width, height, rgba.to_vec())
                    .ok_or_else(|| "播放器截帧尺寸与数据不匹配".to_string())?,
                PendingFrameSource::Disk {
                    frame_path,
                    video_path,
                    local_time_ms,
                } => {
                    let backend: Arc<dyn VideoBackend> = Arc::new(FfmpegBackend::with_defaults());
                    let cache = FrameCache::new(backend).ok();
                    if let Some(cache) = cache.as_ref() {
                        match cache.ensure_frame_lossless(
                            &video_path,
                            local_time_ms,
                            AGG_FRAME_WIDTH,
                        ) {
                            Ok(path) => image::open(&path)
                                .map_err(|e| format!("读取分析帧失败：{e}"))?
                                .to_rgba8(),
                            Err(_) => image::open(&frame_path)
                                .map_err(|e| format!("读取帧失败：{e}"))?
                                .to_rgba8(),
                        }
                    } else {
                        image::open(&frame_path)
                            .map_err(|e| format!("读取帧失败：{e}"))?
                            .to_rgba8()
                    }
                }
            };
            let image = engine.render(&ScopeRequest {
                kind,
                rgba: &img,
                out_width: out_w,
                out_height: out_h,
                max_input_edge,
                options,
            })?;
            Ok(RenderOk { key, image })
        });
    }

    fn options_ui(&mut self, ui: &mut Ui) {
        let before = self.options.clone();
        match self.kind {
            ScopeKind::Histogram => {
                scope_labeled_row(ui, "模式", |ui| {
                    let w = ui.available_width().max(100.0);
                    egui::ComboBox::from_id_salt("scope_hist_mode")
                        .width(w)
                        .selected_text(match self.options.histogram_mode {
                            HistogramMode::Parade => "Parade",
                            HistogramMode::Overlay => "Overlay",
                            HistogramMode::Stack => "Stack",
                        })
                        .show_ui(ui, |ui| {
                            ui.selectable_value(
                                &mut self.options.histogram_mode,
                                HistogramMode::Parade,
                                "Parade",
                            );
                            ui.selectable_value(
                                &mut self.options.histogram_mode,
                                HistogramMode::Overlay,
                                "Overlay",
                            );
                            ui.selectable_value(
                                &mut self.options.histogram_mode,
                                HistogramMode::Stack,
                                "Stack",
                            );
                        });
                });
                scope_labeled_row(ui, "缩放", |ui| {
                    let w = ui.available_width().max(100.0);
                    egui::ComboBox::from_id_salt("scope_hist_scale")
                        .width(w)
                        .selected_text(match self.options.histogram_scale {
                            HistogramScale::Linear => "Linear",
                            HistogramScale::Log => "Log",
                        })
                        .show_ui(ui, |ui| {
                            ui.selectable_value(
                                &mut self.options.histogram_scale,
                                HistogramScale::Linear,
                                "Linear",
                            );
                            ui.selectable_value(
                                &mut self.options.histogram_scale,
                                HistogramScale::Log,
                                "Log",
                            );
                        });
                });
            }
            ScopeKind::Waveform => {
                scope_labeled_row(ui, "通道", |ui| {
                    let w = ui.available_width().max(100.0);
                    egui::ComboBox::from_id_salt("scope_wave_mode")
                        .width(w)
                        .selected_text(match self.options.waveform_mode {
                            WaveformMode::Luma => "Luma",
                            WaveformMode::RgbParade => "RGB Parade",
                        })
                        .show_ui(ui, |ui| {
                            ui.selectable_value(
                                &mut self.options.waveform_mode,
                                WaveformMode::Luma,
                                "Luma",
                            );
                            ui.selectable_value(
                                &mut self.options.waveform_mode,
                                WaveformMode::RgbParade,
                                "RGB Parade",
                            );
                        });
                });
            }
            ScopeKind::Vectorscope => {
                scope_labeled_row(ui, "参考", |ui| {
                    ui.checkbox(&mut self.options.vectorscope_75_box, "75% 圈");
                });
            }
        }
        if self.options != before {
            self.invalidate_aggregate_cache();
        }
    }
}

/// 示波器侧栏：固定标签列 + 同高控件行。
fn scope_labeled_row(ui: &mut Ui, label: &str, add_contents: impl FnOnce(&mut Ui)) {
    let dark = ui.visuals().dark_mode;
    widgets::equal_height_row(ui, 6.0, |ui| {
        ui.add_sized(
            egui::vec2(SCOPE_LABEL_W, widgets::TOOLBAR_ROW_HEIGHT),
            egui::Label::new(
                RichText::new(label)
                    .size(12.0)
                    .color(theme::secondary_label(dark)),
            ),
        );
        ui.allocate_ui_with_layout(
            egui::vec2(
                (ui.available_width()).max(80.0),
                widgets::TOOLBAR_ROW_HEIGHT,
            ),
            egui::Layout::left_to_right(egui::Align::Center),
            add_contents,
        );
    });
}

fn run_aggregate_job(
    job: JobContext,
    engine: Arc<ScopeEngine>,
    video_path: PathBuf,
    range: AggregateRange,
    timestamps: Vec<u64>,
    kind: ScopeKind,
    options: ScopeOptions,
    key: AggregateCacheKey,
) -> Result<AggregateOk, String> {
    let backend: Arc<dyn VideoBackend> = Arc::new(FfmpegBackend::with_defaults());
    let cache = FrameCache::new(backend).map_err(|e| format!("抽帧缓存初始化失败：{e}"))?;
    let mut acc = AggregateAccumulator::new(kind, options.clone());
    let total = timestamps.len();

    for (i, t) in timestamps.into_iter().enumerate() {
        if job.is_cancelled() {
            return Err("已取消".into());
        }
        job.progress
            .set_current_label(&format!("采样 {}/{}", i + 1, total));
        match cache.ensure_frame_lossless(&video_path, t, AGG_FRAME_WIDTH) {
            Ok(path) => match image::open(&path) {
                Ok(img) => acc.push_frame(&img.to_rgba8()),
                Err(_) => acc.note_skip(),
            },
            Err(_) => acc.note_skip(),
        }
        job.progress.inc(None);
    }

    let meta = acc.finish(range)?;
    let image = engine.render_precomputed(
        meta.kind,
        &options,
        OUT_AGG_W,
        OUT_AGG_H,
        meta.data_w,
        meta.data_h,
        &meta.pixels,
    )?;
    Ok(AggregateOk { key, image, meta })
}

fn fit_size(tex: Vec2, avail: Vec2) -> Vec2 {
    let avail = Vec2::new(avail.x.max(64.0), avail.y.max(64.0));
    let scale = (avail.x / tex.x).min(avail.y / tex.y).min(1.0);
    tex * scale
}

/// 解析示波器信号源：对比模式取第一路勾选，否则取当前视频。
pub fn resolve_scope_source<'a>(
    videos: &'a [VideoItem],
    compare_mode: bool,
    compare_ids: &[i64],
    selected_ids: &[i64],
    current_video: Option<i64>,
    // 对比时优先听哪一路（与示波器同源）。
    audio_master: Option<i64>,
) -> Option<&'a VideoItem> {
    let id = if compare_mode {
        audio_master
            .filter(|id| compare_ids.contains(id) || selected_ids.contains(id))
            .or_else(|| compare_ids.first().copied())
            .or_else(|| selected_ids.first().copied())
    } else {
        current_video.or_else(|| selected_ids.first().copied())
    }?;
    videos.iter().find(|v| v.id == id)
}
