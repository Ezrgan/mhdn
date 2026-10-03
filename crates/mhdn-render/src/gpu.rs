//! Low-power wgpu surface. One instanced draw for every quad in the frame.

#![forbid(unsafe_code)]

use std::sync::Arc;

use bytemuck::{Pod, Zeroable};
use winit::window::Window;

use crate::atlas::atlas;
use crate::draw::{Quad, INSTANCE_LIMIT};
use crate::error::RenderError;
use crate::schedule::{pick_format, select_alpha_mode, POWER_PREFERENCE, PRESENT_MODE};
use crate::SHADER;

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct GpuQuad {
    rect: [f32; 4],
    color: [f32; 4],
    uv: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct GpuScreen {
    size: [f32; 2],
    pad: [f32; 2],
}

/// What the surface did since the last [`Renderer::take_present_stats`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PresentStats {
    /// Frames the surface refused with Outdated, Lost or Validation.
    pub errors: u32,
    /// Frames skipped silently because the surface timed out or was occluded.
    pub skipped: u32,
    /// The first refusal this renderer ever saw. Reported once.
    pub first_error: Option<String>,
}

/// Everything an instance hands over once it has a device on the overlay window.
struct Opened {
    adapter: wgpu::Adapter,
    device: wgpu::Device,
    queue: wgpu::Queue,
    surface: wgpu::Surface<'static>,
}

pub struct Renderer {
    adapter_name: String,
    surface_name: String,
    present: PresentStats,
    first_error_seen: bool,
    device: wgpu::Device,
    queue: wgpu::Queue,
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    pipeline: wgpu::RenderPipeline,
    bind_group: wgpu::BindGroup,
    uniform: wgpu::Buffer,
    instances: wgpu::Buffer,
    scratch: Vec<GpuQuad>,
}

impl Renderer {
    pub fn new(window: Arc<Window>) -> Result<Self, RenderError> {
        let Opened {
            adapter,
            device,
            queue,
            surface,
        } = open_window(&window)?;
        let info = adapter.get_info();
        let adapter_name = format!(
            "{} (backend={:?} type={:?} driver={} {})",
            info.name, info.backend, info.device_type, info.driver, info.driver_info
        );

        let caps = surface.get_capabilities(&adapter);
        let format = pick_format(&caps.formats).unwrap_or(wgpu::TextureFormat::Bgra8Unorm);
        let size = window.inner_size();
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            color_space: wgpu::SurfaceColorSpace::Auto,
            width: size.width.max(1),
            height: size.height.max(1),
            present_mode: PRESENT_MODE,
            alpha_mode: select_alpha_mode(&caps.alpha_modes),
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };
        surface.configure(&device, &config);
        let surface_name = format!(
            "format={:?} alpha={:?} offered={:?}",
            config.format, config.alpha_mode, caps.alpha_modes
        );

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("quads"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });
        let bind_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("frame"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX,
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
            label: Some("quads"),
            bind_group_layouts: &[Some(&bind_layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("quads"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[Some(instance_layout())],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });
        let uniform = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("screen"),
            size: std::mem::size_of::<GpuScreen>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let instances = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("quads"),
            size: (std::mem::size_of::<GpuQuad>() * INSTANCE_LIMIT) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let glyphs = atlas();
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("glyphs"),
            size: wgpu::Extent3d {
                width: glyphs.width,
                height: glyphs.height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::R8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        queue.write_texture(
            texture.as_image_copy(),
            &glyphs.pixels,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(glyphs.width),
                rows_per_image: Some(glyphs.height),
            },
            texture.size(),
        );
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("glyphs"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("frame"),
            layout: &bind_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: uniform.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(&sampler),
                },
            ],
        });

        Ok(Self {
            adapter_name,
            surface_name,
            present: PresentStats::default(),
            first_error_seen: false,
            device,
            queue,
            surface,
            config,
            pipeline,
            bind_group,
            uniform,
            instances,
            scratch: Vec::with_capacity(INSTANCE_LIMIT),
        })
    }

    /// The adapter that opened the device, for the diagnostic log.
    pub fn adapter_name(&self) -> &str {
        &self.adapter_name
    }

    /// Surface format and composite alpha, for the diagnostic log. An overlay that
    /// reports `alpha=Opaque` is the one that covers the game with a black sheet.
    pub fn surface_name(&self) -> &str {
        &self.surface_name
    }

    pub fn take_present_stats(&mut self) -> PresentStats {
        std::mem::take(&mut self.present)
    }

    fn note_refusal(&mut self, what: &str, error: bool) {
        if error {
            self.present.errors += 1;
            if !self.first_error_seen {
                self.first_error_seen = true;
                self.present.first_error = Some(format!(
                    "get_current_texture returned {what} (surface {}x{} format={:?} present_mode={:?} alpha_mode={:?})",
                    self.config.width,
                    self.config.height,
                    self.config.format,
                    self.config.present_mode,
                    self.config.alpha_mode
                ));
            }
        } else {
            self.present.skipped += 1;
        }
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        if width == 0 || height == 0 {
            return;
        }
        self.config.width = width;
        self.config.height = height;
        self.surface.configure(&self.device, &self.config);
    }

    pub fn draw(&mut self, quads: &[Quad]) -> Result<(), RenderError> {
        let frame = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(frame)
            | wgpu::CurrentSurfaceTexture::Suboptimal(frame) => frame,
            wgpu::CurrentSurfaceTexture::Timeout => {
                self.note_refusal("Timeout", false);
                return Ok(());
            }
            wgpu::CurrentSurfaceTexture::Occluded => {
                self.note_refusal("Occluded", false);
                return Ok(());
            }
            wgpu::CurrentSurfaceTexture::Outdated => {
                self.note_refusal("Outdated", true);
                return Err(RenderError::Outdated);
            }
            wgpu::CurrentSurfaceTexture::Lost => {
                self.note_refusal("Lost", true);
                return Err(RenderError::Outdated);
            }
            wgpu::CurrentSurfaceTexture::Validation => {
                self.note_refusal("Validation", true);
                return Err(RenderError::Outdated);
            }
        };
        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        if quads.is_empty() {
            let mut encoder = self.encoder();
            clear(&mut encoder, &view);
            self.queue.submit(Some(encoder.finish()));
        } else {
            for (index, chunk) in quads.chunks(INSTANCE_LIMIT).enumerate() {
                self.upload(chunk);
                let mut encoder = self.encoder();
                draw_batch(
                    &mut encoder,
                    &view,
                    &self.pipeline,
                    &self.bind_group,
                    &self.instances,
                    chunk.len() as u32,
                    index == 0,
                );
                self.queue.submit(Some(encoder.finish()));
            }
        }
        self.queue.present(frame);
        Ok(())
    }

    fn encoder(&self) -> wgpu::CommandEncoder {
        self.device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("overlay"),
            })
    }

    fn upload(&mut self, chunk: &[Quad]) {
        self.scratch.clear();
        self.scratch.extend(chunk.iter().map(|quad| GpuQuad {
            rect: [quad.x, quad.y, quad.w, quad.h],
            color: quad.color,
            uv: quad.uv,
        }));
        let screen = GpuScreen {
            size: [self.config.width as f32, self.config.height as f32],
            pad: [0.0, 0.0],
        };
        self.queue
            .write_buffer(&self.uniform, 0, bytemuck::bytes_of(&screen));
        self.queue
            .write_buffer(&self.instances, 0, bytemuck::cast_slice(&self.scratch));
    }
}

fn instance_layout() -> wgpu::VertexBufferLayout<'static> {
    wgpu::VertexBufferLayout {
        array_stride: std::mem::size_of::<GpuQuad>() as u64,
        step_mode: wgpu::VertexStepMode::Instance,
        attributes: &[
            wgpu::VertexAttribute {
                format: wgpu::VertexFormat::Float32x4,
                offset: 0,
                shader_location: 0,
            },
            wgpu::VertexAttribute {
                format: wgpu::VertexFormat::Float32x4,
                offset: 16,
                shader_location: 1,
            },
            wgpu::VertexAttribute {
                format: wgpu::VertexFormat::Float32x4,
                offset: 32,
                shader_location: 2,
            },
        ],
    }
}

fn clear(encoder: &mut wgpu::CommandEncoder, view: &wgpu::TextureView) {
    let attachment = color_attachment(view, true);
    let _pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("overlay"),
        color_attachments: &[Some(attachment)],
        depth_stencil_attachment: None,
        timestamp_writes: None,
        occlusion_query_set: None,
        multiview_mask: None,
    });
}

fn draw_batch(
    encoder: &mut wgpu::CommandEncoder,
    view: &wgpu::TextureView,
    pipeline: &wgpu::RenderPipeline,
    bind_group: &wgpu::BindGroup,
    instances: &wgpu::Buffer,
    count: u32,
    clear_first: bool,
) {
    let attachment = color_attachment(view, clear_first);
    let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("overlay"),
        color_attachments: &[Some(attachment)],
        depth_stencil_attachment: None,
        timestamp_writes: None,
        occlusion_query_set: None,
        multiview_mask: None,
    });
    pass.set_pipeline(pipeline);
    pass.set_bind_group(0, bind_group, &[]);
    pass.set_vertex_buffer(0, instances.slice(..));
    pass.draw(0..6, 0..count);
}

/// Limits for [`Adapter::request_device`]: never above what the adapter reports.
pub fn required_device_limits(adapter: &wgpu::Adapter) -> wgpu::Limits {
    adapter.limits()
}

fn default_instance() -> wgpu::InstanceDescriptor {
    let mut desc = wgpu::InstanceDescriptor::new_without_display_handle();
    desc.backends = wgpu::Backends::all();
    desc
}

/// DirectComposition is the only Windows swapchain wgpu composites with per-pixel
/// alpha. A plain HWND swapchain, DX12 or Vulkan, offers `CompositeAlphaMode::Opaque`
/// and nothing else, so the transparent clear colour reaches the screen as black.
#[cfg(windows)]
fn composited_instance() -> wgpu::InstanceDescriptor {
    let mut desc = wgpu::InstanceDescriptor::new_without_display_handle();
    desc.backends = wgpu::Backends::DX12;
    desc.backend_options.dx12.presentation_system = wgpu::Dx12SwapchainKind::DxgiFromVisual;
    desc
}

#[cfg(windows)]
fn open_window(window: &Arc<Window>) -> Result<Opened, RenderError> {
    match open_instance(window, composited_instance()) {
        Ok(opened) => Ok(opened),
        // A host with no usable DX12 device keeps the overlay running rather than
        // failing to start. It will not be transparent; the log says which one it is.
        Err(err) => open_instance(window, default_instance()).map_err(|_| err),
    }
}

#[cfg(not(windows))]
fn open_window(window: &Arc<Window>) -> Result<Opened, RenderError> {
    open_instance(window, default_instance())
}

fn open_instance(
    window: &Arc<Window>,
    desc: wgpu::InstanceDescriptor,
) -> Result<Opened, RenderError> {
    let backends = desc.backends;
    let instance = wgpu::Instance::new(desc);
    let surface = instance
        .create_surface(Arc::clone(window))
        .map_err(|err| RenderError::Surface(err.to_string()))?;
    let adapter_options = wgpu::RequestAdapterOptions {
        power_preference: POWER_PREFERENCE,
        compatible_surface: Some(&surface),
        force_fallback_adapter: false,
        apply_limit_buckets: true,
    };
    let preferred = pollster::block_on(instance.request_adapter(&adapter_options)).ok();
    let enumerated = pollster::block_on(instance.enumerate_adapters(backends));
    let adapters = ordered_adapters(preferred, enumerated);
    if adapters.is_empty() {
        return Err(RenderError::NoAdapter(
            "no compatible GPU adapters found".to_string(),
        ));
    }
    let (adapter, device, queue) = open_device(&adapters, &surface)?;
    Ok(Opened {
        adapter,
        device,
        queue,
        surface,
    })
}

fn ordered_adapters(
    preferred: Option<wgpu::Adapter>,
    enumerated: Vec<wgpu::Adapter>,
) -> Vec<wgpu::Adapter> {
    let mut adapters = Vec::new();
    if let Some(adapter) = preferred {
        adapters.push(adapter);
    }
    for adapter in enumerated {
        if !adapters.contains(&adapter) {
            adapters.push(adapter);
        }
    }
    adapters
}

fn open_device(
    adapters: &[wgpu::Adapter],
    surface: &wgpu::Surface<'_>,
) -> Result<(wgpu::Adapter, wgpu::Device, wgpu::Queue), RenderError> {
    let mut last_err = String::new();
    for adapter in adapters {
        if !adapter.is_surface_supported(surface) {
            continue;
        }
        let limits = required_device_limits(adapter);
        match pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("mhdn"),
            required_features: wgpu::Features::empty(),
            required_limits: limits,
            experimental_features: wgpu::ExperimentalFeatures::disabled(),
            memory_hints: wgpu::MemoryHints::MemoryUsage,
            trace: wgpu::Trace::Off,
        })) {
            Ok((device, queue)) => return Ok((adapter.clone(), device, queue)),
            Err(err) => last_err = err.to_string(),
        }
    }
    Err(RenderError::Device(if last_err.is_empty() {
        "no adapter could open a GPU device".to_string()
    } else {
        last_err
    }))
}

fn color_attachment(view: &wgpu::TextureView, clear: bool) -> wgpu::RenderPassColorAttachment<'_> {
    wgpu::RenderPassColorAttachment {
        view,
        depth_slice: None,
        resolve_target: None,
        ops: wgpu::Operations {
            load: if clear {
                wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT)
            } else {
                wgpu::LoadOp::Load
            },
            store: wgpu::StoreOp::Store,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_quad_shader_is_valid_wgsl() {
        let module = naga::front::wgsl::parse_str(SHADER).expect("parse");
        let mut validator = naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::all(),
        );
        validator.validate(&module).expect("validate");
    }

    #[test]
    fn one_instance_is_three_vec4s() {
        assert_eq!(std::mem::size_of::<GpuQuad>(), 48);
        assert!(std::mem::size_of::<GpuScreen>() <= 16);
    }
}
