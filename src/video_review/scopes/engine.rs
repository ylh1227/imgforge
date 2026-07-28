//! wgpu 离屏示波器引擎：分析 → GPU 绘制 → readback（资源可复用）。

use std::num::NonZeroU32;
use std::sync::{Arc, Mutex};

use bytemuck::{Pod, Zeroable};
use image::{imageops::FilterType, RgbaImage};

use super::aggregate::scope_mode_uniforms;
use super::color::SKIN_TONE_ANGLE_DEG;
use super::histogram;
use super::vectorscope::{self, VECTOR_SIZE};
use super::waveform;
use super::{ScopeKind, ScopeOptions, ScopeRequest, ScopeRgba};

const DEFAULT_MAX_INPUT_EDGE: u32 = 1280;
const SHADER: &str = include_str!("scope_shader.wgsl");

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct ScopeUniforms {
    kind: u32,
    mode: u32,
    scale: u32,
    show_box: u32,
    out_w: u32,
    out_h: u32,
    data_w: u32,
    data_h: u32,
    skin_angle: f32,
    _pad: [f32; 3],
}

struct DataTexture {
    w: u32,
    h: u32,
    texture: wgpu::Texture,
}

struct OutputTargets {
    out_w: u32,
    out_h: u32,
    padded_bpr: u32,
    target: wgpu::Texture,
    staging: wgpu::Buffer,
}

pub struct ScopeEngine {
    device: Arc<wgpu::Device>,
    queue: Arc<wgpu::Queue>,
    pipeline: wgpu::RenderPipeline,
    bind_group_layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    uniform_buf: wgpu::Buffer,
    data_tex: Mutex<Option<DataTexture>>,
    outputs: Mutex<Option<OutputTargets>>,
}

impl ScopeEngine {
    pub fn try_new() -> Result<Self, String> {
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
            backends: wgpu::Backends::PRIMARY,
            ..Default::default()
        });
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::LowPower,
            compatible_surface: None,
            force_fallback_adapter: false,
        }))
        .ok_or_else(|| "未找到可用 GPU 适配器".to_string())?;

        let (device, queue) = pollster::block_on(adapter.request_device(
            &wgpu::DeviceDescriptor {
                label: Some("imgforge_scope_device"),
                required_features: wgpu::Features::empty(),
                required_limits:
                    wgpu::Limits::downlevel_defaults().using_resolution(adapter.limits()),
                memory_hints: wgpu::MemoryHints::MemoryUsage,
            },
            None,
        ))
        .map_err(|e| format!("创建 wgpu Device 失败：{e}"))?;

        let device = Arc::new(device);
        let queue = Arc::new(queue);

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("scope_shader"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });

        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("scope_bgl"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("scope_pl"),
            bind_group_layouts: &[&bind_group_layout],
            push_constant_ranges: &[],
        });

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("scope_pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: wgpu::TextureFormat::Rgba8Unorm,
                    blend: Some(wgpu::BlendState::REPLACE),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("scope_sampler"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });

        let uniform_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("scope_uniforms"),
            size: std::mem::size_of::<ScopeUniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        Ok(Self {
            device,
            queue,
            pipeline,
            bind_group_layout,
            sampler,
            uniform_buf,
            data_tex: Mutex::new(None),
            outputs: Mutex::new(None),
        })
    }

    pub fn render(&self, request: &ScopeRequest<'_>) -> Result<ScopeRgba, String> {
        let max_edge = if request.max_input_edge == 0 {
            DEFAULT_MAX_INPUT_EDGE
        } else {
            request.max_input_edge
        };
        let img = prepare_input(request.rgba, max_edge)?;
        let out_w = request.out_width.max(64);
        let out_h = request.out_height.max(64);
        let opts = &request.options;

        let (data_w, data_h, pixels) = match request.kind {
            ScopeKind::Histogram => {
                let bins = histogram::analyze(&img);
                let map = histogram::bins_to_height_map(&bins, opts.histogram_scale);
                (256u32, 4u32, map.to_vec())
            }
            ScopeKind::Waveform => {
                let data = waveform::analyze(&img, opts.waveform_mode);
                let map = waveform::to_intensity_map(&data);
                (data.width, 256u32, map)
            }
            ScopeKind::Vectorscope => {
                let data = vectorscope::analyze(&img);
                let map = vectorscope::to_intensity_map(&data);
                (VECTOR_SIZE, VECTOR_SIZE, map)
            }
        };

        self.render_precomputed(request.kind, opts, out_w, out_h, data_w, data_h, &pixels)
    }

    /// 使用已分析/已聚合的强度图渲染示波器。
    pub fn render_precomputed(
        &self,
        kind: ScopeKind,
        options: &ScopeOptions,
        out_width: u32,
        out_height: u32,
        data_w: u32,
        data_h: u32,
        pixels: &[u8],
    ) -> Result<ScopeRgba, String> {
        let out_w = out_width.max(64);
        let out_h = out_height.max(64);
        let (kind_u, mode_u, scale_u) = scope_mode_uniforms(kind, options);
        let uniforms = ScopeUniforms {
            kind: kind_u,
            mode: mode_u,
            scale: scale_u,
            show_box: u32::from(options.vectorscope_75_box),
            out_w,
            out_h,
            data_w,
            data_h,
            skin_angle: SKIN_TONE_ANGLE_DEG,
            _pad: [0.0; 3],
        };
        self.render_from_data(out_w, out_h, data_w, data_h, pixels, uniforms)
    }

    fn ensure_data_texture(&self, data_w: u32, data_h: u32) -> Result<(), String> {
        let mut guard = self
            .data_tex
            .lock()
            .map_err(|_| "示波器 data texture 锁失败".to_string())?;
        let needs_new = match guard.as_ref() {
            Some(t) => t.w != data_w || t.h != data_h,
            None => true,
        };
        if needs_new {
            let texture = self.device.create_texture(&wgpu::TextureDescriptor {
                label: Some("scope_data"),
                size: wgpu::Extent3d {
                    width: data_w,
                    height: data_h,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8Unorm,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            });
            *guard = Some(DataTexture {
                w: data_w,
                h: data_h,
                texture,
            });
        }
        Ok(())
    }

    fn ensure_outputs(&self, out_w: u32, out_h: u32) -> Result<(), String> {
        let align = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
        let unpadded = out_w * 4;
        let padded = (unpadded + align - 1) / align * align;
        let mut guard = self
            .outputs
            .lock()
            .map_err(|_| "示波器 output 锁失败".to_string())?;
        let needs_new = match guard.as_ref() {
            Some(t) => t.out_w != out_w || t.out_h != out_h,
            None => true,
        };
        if needs_new {
            let target = self.device.create_texture(&wgpu::TextureDescriptor {
                label: Some("scope_target"),
                size: wgpu::Extent3d {
                    width: out_w,
                    height: out_h,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8Unorm,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
                view_formats: &[],
            });
            let staging = self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("scope_staging"),
                size: (padded * out_h) as u64,
                usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            *guard = Some(OutputTargets {
                out_w,
                out_h,
                padded_bpr: padded,
                target,
                staging,
            });
        }
        Ok(())
    }

    fn render_from_data(
        &self,
        out_w: u32,
        out_h: u32,
        data_w: u32,
        data_h: u32,
        data: &[u8],
        uniforms: ScopeUniforms,
    ) -> Result<ScopeRgba, String> {
        let expected = (data_w * data_h) as usize;
        if data.len() != expected {
            return Err(format!(
                "示波器数据尺寸不匹配：期望 {expected}，实际 {}",
                data.len()
            ));
        }

        let mut rgba_data = Vec::with_capacity(expected * 4);
        for &v in data {
            rgba_data.extend_from_slice(&[v, v, v, 255]);
        }

        self.ensure_data_texture(data_w, data_h)?;
        self.ensure_outputs(out_w, out_h)?;

        {
            let guard = self
                .data_tex
                .lock()
                .map_err(|_| "示波器 data texture 锁失败".to_string())?;
            let data_tex = guard.as_ref().ok_or("data texture 未初始化")?;
            self.queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &data_tex.texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                &rgba_data,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(data_w * 4),
                    rows_per_image: Some(data_h),
                },
                wgpu::Extent3d {
                    width: data_w,
                    height: data_h,
                    depth_or_array_layers: 1,
                },
            );
        }

        self.queue
            .write_buffer(&self.uniform_buf, 0, bytemuck::bytes_of(&uniforms));

        let data_view = {
            let guard = self
                .data_tex
                .lock()
                .map_err(|_| "示波器 data texture 锁失败".to_string())?;
            let data_tex = guard.as_ref().ok_or("data texture 未初始化")?;
            data_tex
                .texture
                .create_view(&wgpu::TextureViewDescriptor::default())
        };

        let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("scope_bg"),
            layout: &self.bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: self.uniform_buf.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&data_view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
            ],
        });

        let mut outputs_guard = self
            .outputs
            .lock()
            .map_err(|_| "示波器 output 锁失败".to_string())?;
        let outputs = outputs_guard.as_mut().ok_or("output 未初始化")?;
        let padded = outputs.padded_bpr;
        let unpadded = out_w * 4;
        let target_view = outputs
            .target
            .create_view(&wgpu::TextureViewDescriptor::default());

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("scope_encoder"),
            });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("scope_pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &target_view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: 0.04,
                            g: 0.04,
                            b: 0.05,
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &bind_group, &[]);
            pass.draw(0..3, 0..1);
        }
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &outputs.target,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &outputs.staging,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(NonZeroU32::new(padded).map(|n| n.get()).unwrap_or(padded)),
                    rows_per_image: Some(out_h),
                },
            },
            wgpu::Extent3d {
                width: out_w,
                height: out_h,
                depth_or_array_layers: 1,
            },
        );
        self.queue.submit(Some(encoder.finish()));

        let slice = outputs.staging.slice(..);
        let (tx, rx) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |r| {
            let _ = tx.send(r);
        });
        self.device.poll(wgpu::Maintain::Wait);
        rx.recv()
            .map_err(|_| "示波器 readback 通道断开".to_string())?
            .map_err(|e| format!("示波器 map 失败：{e}"))?;

        let mapped = slice.get_mapped_range();
        let mut rgba = Vec::with_capacity((out_w * out_h * 4) as usize);
        for row in 0..out_h {
            let start = (row * padded) as usize;
            let end = start + unpadded as usize;
            rgba.extend_from_slice(&mapped[start..end]);
        }
        drop(mapped);
        outputs.staging.unmap();
        drop(outputs_guard);

        Ok(ScopeRgba {
            width: out_w,
            height: out_h,
            rgba,
        })
    }
}

fn prepare_input(src: &RgbaImage, max_edge: u32) -> Result<RgbaImage, String> {
    let (w, h) = src.dimensions();
    if w == 0 || h == 0 {
        return Err("示波器输入帧为空".into());
    }
    let limit = max_edge.max(64);
    let longest = w.max(h);
    if longest <= limit {
        return Ok(src.clone());
    }
    let scale = limit as f32 / longest as f32;
    let nw = ((w as f32 * scale).round() as u32).max(1);
    let nh = ((h as f32 * scale).round() as u32).max(1);
    Ok(image::imageops::resize(src, nw, nh, FilterType::CatmullRom))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::video_review::scopes::{ScopeKind, ScopeOptions, ScopeRequest};
    use image::{ImageBuffer, Rgba};

    #[test]
    #[ignore = "需要可用 GPU / wgpu 适配器"]
    fn engine_renders_histogram() {
        let engine = ScopeEngine::try_new().expect("wgpu");
        let img = ImageBuffer::from_pixel(32, 24, Rgba([200u8, 40, 40, 255]));
        let out = engine
            .render(&ScopeRequest {
                kind: ScopeKind::Histogram,
                rgba: &img,
                out_width: 320,
                out_height: 180,
                max_input_edge: 960,
                options: ScopeOptions::default(),
            })
            .expect("render");
        assert_eq!(out.rgba.len(), (320 * 180 * 4) as usize);
    }
}
