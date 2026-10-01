//! Low-power wgpu surface. Presents only when the caller submits quads or a clear.

#![forbid(unsafe_code)]

use std::sync::Arc;

use bytemuck::{Pod, Zeroable};
use winit::window::Window;

use crate::draw::{Quad, QUADS_PER_BATCH};
use crate::error::RenderError;
use crate::schedule::{pick_format, select_alpha_mode, POWER_PREFERENCE, PRESENT_MODE};
use crate::SHADER;

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct GpuQuad {
    rect: [f32; 4],
    color: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct GpuFrame {
    screen: [f32; 2],
    pad: [f32; 2],
    quads: [GpuQuad; QUADS_PER_BATCH],
}

pub struct Renderer {
    device: wgpu::Device,
    queue: wgpu::Queue,
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    pipeline: wgpu::RenderPipeline,
    bind_group: wgpu::BindGroup,
    uniform: wgpu::Buffer,
}

impl Renderer {
    pub fn new(window: Arc<Window>) -> Result<Self, RenderError> {
        let mut desc = wgpu::InstanceDescriptor::new_without_display_handle();
        desc.backends = wgpu::Backends::all();
        let instance = wgpu::Instance::new(desc);
        let surface = instance
            .create_surface(Arc::clone(&window))
            .map_err(|err| RenderError::Surface(err.to_string()))?;
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: POWER_PREFERENCE,
            compatible_surface: Some(&surface),
            force_fallback_adapter: false,
            apply_limit_buckets: true,
        }))
        .map_err(|err| RenderError::NoAdapter(err.to_string()))?;
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("mhdn"),
            required_features: wgpu::Features::empty(),
            required_limits: wgpu::Limits::default(),
            experimental_features: wgpu::ExperimentalFeatures::disabled(),
            memory_hints: wgpu::MemoryHints::MemoryUsage,
            trace: wgpu::Trace::Off,
        }))
        .map_err(|err| RenderError::Device(err.to_string()))?;

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

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("quads"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });
        let bind_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("frame"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
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
                buffers: &[],
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
            label: Some("frame"),
            size: std::mem::size_of::<GpuFrame>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("frame"),
            layout: &bind_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform.as_entire_binding(),
            }],
        });

        Ok(Self {
            device,
            queue,
            surface,
            config,
            pipeline,
            bind_group,
            uniform,
        })
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
            wgpu::CurrentSurfaceTexture::Timeout | wgpu::CurrentSurfaceTexture::Occluded => {
                return Ok(())
            }
            wgpu::CurrentSurfaceTexture::Outdated
            | wgpu::CurrentSurfaceTexture::Lost
            | wgpu::CurrentSurfaceTexture::Validation => return Err(RenderError::Outdated),
        };
        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("overlay"),
            });
        if quads.is_empty() {
            clear(&mut encoder, &view);
        } else {
            for (index, chunk) in quads.chunks(QUADS_PER_BATCH).enumerate() {
                self.upload(chunk);
                draw_batch(
                    &mut encoder,
                    &view,
                    &self.pipeline,
                    &self.bind_group,
                    chunk.len() as u32,
                    index == 0,
                );
            }
        }
        self.queue.submit(Some(encoder.finish()));
        self.queue.present(frame);
        Ok(())
    }

    fn upload(&self, chunk: &[Quad]) {
        let mut frame = GpuFrame::zeroed();
        frame.screen = [self.config.width as f32, self.config.height as f32];
        for (slot, quad) in chunk.iter().enumerate() {
            frame.quads[slot] = GpuQuad {
                rect: [quad.x, quad.y, quad.w, quad.h],
                color: quad.color,
            };
        }
        self.queue
            .write_buffer(&self.uniform, 0, bytemuck::bytes_of(&frame));
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
    pass.draw(0..6, 0..count);
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
    fn a_frame_of_quads_fits_in_one_uniform() {
        assert!(std::mem::size_of::<GpuFrame>() <= 16 * 1024);
        assert_eq!(
            std::mem::size_of::<GpuQuad>() * QUADS_PER_BATCH + 16,
            std::mem::size_of::<GpuFrame>()
        );
    }
}
