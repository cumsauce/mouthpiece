//! Runs the effect chain on the GPU. Each effect is one draw into a texture, and the next one reads it.

use crate::Pass;

/// The output is stored as plain bytes, and egui reads it through a view that decodes sRGB.
pub const OUTPUT_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;
pub const OUTPUT_VIEW_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;

const UNIFORM_SIZE: u64 = 96;
/// Uniforms for each pass have to start on this boundary, which every GPU supports.
const UNIFORM_STEP: u64 = 256;
/// More effects than anyone would chain on one frame. Past this, the rest are skipped.
const MAX_PASSES: usize = 64;

struct Target {
    texture: wgpu::Texture,
    view: wgpu::TextureView,
    bind_group: wgpu::BindGroup,
}

pub struct Renderer {
    device: wgpu::Device,
    queue: wgpu::Queue,
    pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    uniforms: wgpu::Buffer,
    source: Option<(u32, u32, Target)>,
    scratch: [Target; 2],
    output: wgpu::Texture,
    output_view: wgpu::TextureView,
    width: u32,
    height: u32,
}

fn usage() -> wgpu::TextureUsages {
    wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_DST | wgpu::TextureUsages::COPY_SRC
}

impl Renderer {
    /// Uses an existing device, such as the one drawing the app's window.
    pub fn new(device: wgpu::Device, queue: wgpu::Queue, width: u32, height: u32) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("picture effects"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shader.wgsl").into()),
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("picture effects"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: true,
                        min_binding_size: wgpu::BufferSize::new(UNIFORM_SIZE),
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
            label: Some("picture effects"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("picture effects"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState { module: &shader, entry_point: Some("vs_main"), compilation_options: Default::default(), buffers: &[] },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState { format: OUTPUT_FORMAT, blend: None, write_mask: wgpu::ColorWrites::ALL })],
            }),
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            multiview_mask: None,
            cache: None,
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let uniforms = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("picture effects"),
            size: UNIFORM_STEP * MAX_PASSES as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let make = |w, h| make_target(&device, &layout, &uniforms, &sampler, w, h);
        let scratch = [make(width, height), make(width, height)];
        let output = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("picture output"),
            size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: OUTPUT_FORMAT,
            usage: usage(),
            view_formats: &[OUTPUT_VIEW_FORMAT],
        });
        let output_view = output.create_view(&Default::default());
        Self { device, queue, pipeline, layout, sampler, uniforms, source: None, scratch, output, output_view, width, height }
    }

    /// Uses a GPU of its own, for rendering with no window. `None` if the machine has no usable GPU.
    pub fn headless(width: u32, height: u32) -> Option<Self> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let adapter = pollster::block_on(instance.request_adapter(&Default::default())).ok()?;
        let (device, queue) = pollster::block_on(adapter.request_device(&Default::default())).ok()?;
        Some(Self::new(device, queue, width, height))
    }

    /// A view of the result that egui can display.
    pub fn display_view(&self) -> wgpu::TextureView {
        self.output.create_view(&wgpu::TextureViewDescriptor { format: Some(OUTPUT_VIEW_FORMAT), ..Default::default() })
    }

    /// Sets the frame to process, as tightly packed RGBA.
    pub fn upload(&mut self, rgba: &[u8], width: u32, height: u32) {
        if self.source.as_ref().is_none_or(|(w, h, _)| *w != width || *h != height) {
            let target = make_target(&self.device, &self.layout, &self.uniforms, &self.sampler, width, height);
            self.source = Some((width, height, target));
        }
        let Some((_, _, target)) = &self.source else { return };
        self.queue.write_texture(
            wgpu::TexelCopyTextureInfo { texture: &target.texture, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
            rgba,
            wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(width * 4), rows_per_image: Some(height) },
            wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
        );
    }

    /// Runs the chain over the uploaded frame, into the output.
    pub fn render(&mut self, passes: &[Pass], time: f32, aspect: f32) {
        let Some((_, _, source)) = &self.source else { return };
        // With no effects, one plain pass still copies and scales the frame to the output.
        let identity = Pass { kind: 0, params: [0.0; 12], area: [0.5; 4], area_on: false, ellipse: true, invert: false, feather: 0.0 };
        let passes: &[Pass] = if passes.is_empty() { std::slice::from_ref(&identity) } else { &passes[..passes.len().min(MAX_PASSES)] };
        let mut data = vec![0u8; passes.len() * UNIFORM_STEP as usize];
        for (i, pass) in passes.iter().enumerate() {
            write_uniforms(&mut data[i * UNIFORM_STEP as usize..], pass, time, aspect);
        }
        self.queue.write_buffer(&self.uniforms, 0, &data);
        let mut encoder = self.device.create_command_encoder(&Default::default());
        let mut input = &source.bind_group;
        for i in 0..passes.len() {
            let last = i + 1 == passes.len();
            let target = if last { &self.output_view } else { &self.scratch[i % 2].view };
            let mut draw = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("picture effect"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: target,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::BLACK), store: wgpu::StoreOp::Store },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            draw.set_pipeline(&self.pipeline);
            draw.set_bind_group(0, input, &[(i as u64 * UNIFORM_STEP) as u32]);
            draw.draw(0..3, 0..1);
            drop(draw);
            input = &self.scratch[i % 2].bind_group;
        }
        self.queue.submit([encoder.finish()]);
    }

    /// Reads the output back as tightly packed RGBA. Waits for the GPU, so it's for export, not preview.
    pub fn read_output(&self) -> Vec<u8> {
        let row = self.width * 4;
        let padded = row.div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT) * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
        let buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("picture readback"),
            size: (padded * self.height) as u64,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let mut encoder = self.device.create_command_encoder(&Default::default());
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo { texture: &self.output, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
            wgpu::TexelCopyBufferInfo { buffer: &buffer, layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(padded), rows_per_image: Some(self.height) } },
            wgpu::Extent3d { width: self.width, height: self.height, depth_or_array_layers: 1 },
        );
        self.queue.submit([encoder.finish()]);
        let slice = buffer.slice(..);
        slice.map_async(wgpu::MapMode::Read, |_| {});
        let _ = self.device.poll(wgpu::PollType::wait_indefinitely());
        let mut out = Vec::with_capacity((row * self.height) as usize);
        if let Ok(mapped) = slice.get_mapped_range() {
            for line in mapped.chunks(padded as usize) {
                out.extend_from_slice(&line[..row as usize]);
            }
        }
        buffer.unmap();
        out
    }
}

fn make_target(device: &wgpu::Device, layout: &wgpu::BindGroupLayout, uniforms: &wgpu::Buffer, sampler: &wgpu::Sampler, width: u32, height: u32) -> Target {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("picture frame"),
        size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: OUTPUT_FORMAT,
        usage: usage(),
        view_formats: &[],
    });
    let view = texture.create_view(&Default::default());
    let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("picture frame"),
        layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding { buffer: uniforms, offset: 0, size: wgpu::BufferSize::new(UNIFORM_SIZE) }),
            },
            wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&view) },
            wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::Sampler(sampler) },
        ],
    });
    Target { texture, view, bind_group }
}

fn write_uniforms(out: &mut [u8], pass: &Pass, time: f32, aspect: f32) {
    let mut put = |at: usize, bytes: [u8; 4]| out[at..at + 4].copy_from_slice(&bytes);
    put(0, pass.kind.to_le_bytes());
    put(4, time.to_le_bytes());
    put(8, aspect.to_le_bytes());
    put(12, (pass.area_on as u32).to_le_bytes());
    for (i, v) in pass.area.iter().enumerate() {
        put(16 + i * 4, v.to_le_bytes());
    }
    put(32, pass.feather.to_le_bytes());
    put(36, (pass.ellipse as u8 as f32).to_le_bytes());
    put(40, (pass.invert as u8 as f32).to_le_bytes());
    for (i, v) in pass.params.iter().enumerate() {
        put(48 + i * 4, v.to_le_bytes());
    }
}
