use std::sync::Arc;

use bytemuck::{Pod, Zeroable};

use crate::capture::DesktopCapture;
use crate::log;
use crate::texture::{build_disk_texture, DISK_TEXTURE_HEIGHT, DISK_TEXTURE_WIDTH};

const TARGET_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Bgra8Unorm;
const HDR_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;

/// The layered window is a tight square: diameter = WINDOW_SCALE * shadow radius.
/// Keeping this small is what makes the glow click-through outside the core.
pub const WINDOW_SCALE: f32 = 6.0;

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub struct Uniforms {
    pub resolution: [f32; 2],
    pub time: f32,
    pub hole_radius: f32,
    pub camera: [f32; 4],
    pub disk: [f32; 4],
    pub look: [f32; 4],
    pub motion: [f32; 4],
    pub flags: [f32; 4],
    pub capture: [f32; 4],
    pub accent: [f32; 4],
    pub fun: [f32; 4],
    pub extra: [f32; 4],
}

#[derive(Clone, Copy, Debug)]
pub struct FrameParams {
    pub time: f32,
    pub hole_radius: f32,
    pub yaw: f32,
    pub pitch: f32,
    pub roll: f32,
    pub distance: f32,
    pub disk_inner: f32,
    pub disk_outer: f32,
    pub disk_thickness: f32,
    pub disk_brightness: f32,
    pub exposure: f32,
    pub glow: f32,
    pub pulse: f32,
    pub pulse_age: f32,
    pub fade: f32,
    pub hover_x: f32,
    pub hover_y: f32,
    pub motion_amount: f32,
    pub lens_strength: f32,
    pub accent: [f32; 3],
    pub fun: [f32; 4],
    pub focus: f32,
    pub debug: u32,
}

impl Default for FrameParams {
    fn default() -> Self {
        Self {
            time: 0.0,
            hole_radius: 52.0,
            yaw: 0.055,
            pitch: 0.020,
            roll: -0.022,
            distance: 16.0,
            disk_inner: 2.85,
            disk_outer: 9.00,
            disk_thickness: 0.018,
            disk_brightness: 2.10,
            exposure: 1.08,
            glow: 1.0,
            pulse: 0.0,
            pulse_age: 0.0,
            fade: 1.0,
            hover_x: 0.0,
            hover_y: 0.0,
            motion_amount: 1.0,
            lens_strength: 0.60,
            accent: [1.0, 0.79, 0.62],
            fun: [0.0, -1.0, 0.0, 0.0],
            focus: 0.0,
            debug: 0,
        }
    }
}

pub struct Renderer {
    device: Arc<wgpu::Device>,
    queue: Arc<wgpu::Queue>,
    scene_pipeline: wgpu::RenderPipeline,
    composite_pipeline: wgpu::RenderPipeline,
    scene_bind_group: wgpu::BindGroup,
    composite_bind_group: wgpu::BindGroup,
    composite_layout: wgpu::BindGroupLayout,
    hdr_sampler: wgpu::Sampler,
    uniform_buffer: wgpu::Buffer,
    backdrop_texture: wgpu::Texture,
    backdrop_view: wgpu::TextureView,
    capture: Option<DesktopCapture>,
    capture_meta: [f32; 4],
    capture_size: (u32, u32),
    capture_pad: u32,
    capture_error_logged: bool,
    _disk_texture: wgpu::Texture,
    scene_target: wgpu::Texture,
    scene_view: wgpu::TextureView,
    target: wgpu::Texture,
    target_view: wgpu::TextureView,
    readback: wgpu::Buffer,
    width: u32,
    height: u32,
    padded_bytes_per_row: u32,
    pixels: Vec<u8>,
    adapter_name: String,
    backend: String,
}

impl Renderer {
    pub fn new(width: u32, height: u32) -> Result<Self, String> {
        let width = width.max(32);
        let height = height.max(32);
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::DX12 | wgpu::Backends::VULKAN | wgpu::Backends::GL,
            ..Default::default()
        });
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            compatible_surface: None,
            force_fallback_adapter: false,
        }))
        .ok_or_else(|| "没有可用的 GPU 适配器（需要 DirectX 12 / Vulkan）".to_string())?;
        let info = adapter.get_info();
        log::info(&format!(
            "gpu adapter: {} ({:?}, {:?})",
            info.name, info.backend, info.device_type
        ));
        let (device, queue) = pollster::block_on(adapter.request_device(
            &wgpu::DeviceDescriptor {
                label: Some("singularity-device"),
                required_features: wgpu::Features::empty(),
                required_limits: wgpu::Limits::default(),
            },
            None,
        ))
        .map_err(|e| format!("创建 GPU 设备失败: {e}"))?;

        let scene_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("singularity-scene-shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shader.wgsl").into()),
        });
        let composite_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("singularity-composite-shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("composite.wgsl").into()),
        });

        let scene_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("singularity-scene-bgl"),
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
        let composite_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("singularity-composite-bgl"),
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
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
            ],
        });

        let scene_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("singularity-scene-layout"),
                bind_group_layouts: &[&scene_layout],
                push_constant_ranges: &[],
            });
        let composite_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("singularity-composite-layout"),
                bind_group_layouts: &[&composite_layout],
                push_constant_ranges: &[],
            });

        let scene_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("singularity-scene-pipeline"),
            layout: Some(&scene_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &scene_shader,
                entry_point: "vs_main",
                compilation_options: Default::default(),
                buffers: &[],
            },
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &scene_shader,
                entry_point: "fs_main",
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: HDR_FORMAT,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview: None,
        });
        let composite_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("singularity-composite-pipeline"),
            layout: Some(&composite_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &composite_shader,
                entry_point: "vs_main",
                compilation_options: Default::default(),
                buffers: &[],
            },
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &composite_shader,
                entry_point: "fs_main",
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: TARGET_FORMAT,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview: None,
        });

        let uniform_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("singularity-uniforms"),
            size: std::mem::size_of::<Uniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let (disk_texture, disk_view, sampler) = create_disk_texture(&device, &queue)?;
        let scene_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("singularity-scene-bind-group"),
            layout: &scene_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: uniform_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&disk_view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(&sampler),
                },
            ],
        });

        let hdr_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("singularity-hdr-sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });

        let padded_bytes_per_row = padded_row_bytes(width);
        let scene_target = create_hdr_target(&device, width, height);
        let scene_view = scene_target.create_view(&wgpu::TextureViewDescriptor::default());
        let (backdrop_texture, backdrop_view) = create_backdrop_texture(&device, 1, 1);
        let composite_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("singularity-composite-bind-group"),
            layout: &composite_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: uniform_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&scene_view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(&hdr_sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::TextureView(&backdrop_view),
                },
            ],
        });
        let target = create_target(&device, width, height);
        let target_view = target.create_view(&wgpu::TextureViewDescriptor::default());
        let readback = create_readback(&device, width, height);

        Ok(Self {
            device: Arc::new(device),
            queue: Arc::new(queue),
            scene_pipeline,
            composite_pipeline,
            scene_bind_group,
            composite_bind_group,
            composite_layout,
            hdr_sampler,
            uniform_buffer,
            backdrop_texture,
            backdrop_view,
            capture: None,
            capture_meta: [0.0; 4],
            capture_size: (1, 1),
            capture_pad: 0,
            capture_error_logged: false,
            _disk_texture: disk_texture,
            scene_target,
            scene_view,
            target,
            target_view,
            readback,
            width,
            height,
            padded_bytes_per_row,
            pixels: vec![0; (width * height * 4) as usize],
            adapter_name: info.name,
            backend: format!("{:?}", info.backend),
        })
    }

    pub fn adapter_name(&self) -> &str {
        &self.adapter_name
    }

    pub fn backend(&self) -> &str {
        &self.backend
    }

    pub fn device(&self) -> Arc<wgpu::Device> {
        self.device.clone()
    }

    pub fn queue(&self) -> Arc<wgpu::Queue> {
        self.queue.clone()
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        let width = width.max(32);
        let height = height.max(32);
        if width == self.width && height == self.height {
            return;
        }
        self.width = width;
        self.height = height;
        self.padded_bytes_per_row = padded_row_bytes(width);
        self.scene_target = create_hdr_target(&self.device, width, height);
        self.scene_view = self
            .scene_target
            .create_view(&wgpu::TextureViewDescriptor::default());
        self.composite_bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("singularity-composite-bind-group"),
            layout: &self.composite_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: self.uniform_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&self.scene_view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(&self.hdr_sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::TextureView(&self.backdrop_view),
                },
            ],
        });
        self.target = create_target(&self.device, width, height);
        self.target_view = self
            .target
            .create_view(&wgpu::TextureViewDescriptor::default());
        self.readback = create_readback(&self.device, width, height);
        self.pixels.resize((width * height * 4) as usize, 0);
        log::info(&format!("renderer resize {width}x{height}"));
    }

    fn rebuild_composite_bind_group(&mut self) {
        self.composite_bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("singularity-composite-bind-group"),
            layout: &self.composite_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: self.uniform_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&self.scene_view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(&self.hdr_sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::TextureView(&self.backdrop_view),
                },
            ],
        });
    }

    /// Capture the desktop just behind the pet and upload it as the lens source.
    /// The pet window must have WDA_EXCLUDEFROMCAPTURE set, otherwise this
    /// would feed the pet's own pixels back into the lens.
    pub fn capture_backdrop(&mut self, left: i32, top: i32) -> Result<(), String> {
        let radius = self.width as f32 / WINDOW_SCALE;
        let pad = (radius * 2.0).ceil().max(24.0) as u32;
        let capture_width = self.width + pad * 2;
        let capture_height = self.height + pad * 2;

        if self.capture.is_none() || self.capture_size != (capture_width, capture_height) {
            let capture = DesktopCapture::new(capture_width, capture_height)?;
            let (texture, view) =
                create_backdrop_texture(&self.device, capture_width, capture_height);
            self.capture = Some(capture);
            self.backdrop_texture = texture;
            self.backdrop_view = view;
            self.capture_size = (capture_width, capture_height);
            self.capture_pad = pad;
            self.rebuild_composite_bind_group();
        }

        let capture = self.capture.as_ref().ok_or("桌面捕获未初始化")?;
        let result = capture.capture(left - pad as i32, top - pad as i32);
        if let Err(error) = &result {
            if !self.capture_error_logged {
                log::warn(&format!("desktop capture failed: {error}"));
                self.capture_error_logged = true;
            }
            self.capture_meta = [0.0; 4];
            return result;
        }

        let captured_width = capture.width();
        let captured_height = capture.height();
        self.queue.write_texture(
            wgpu::ImageCopyTexture {
                texture: &self.backdrop_texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            capture.pixels(),
            wgpu::ImageDataLayout {
                offset: 0,
                bytes_per_row: Some(captured_width * 4),
                rows_per_image: Some(captured_height),
            },
            wgpu::Extent3d {
                width: captured_width,
                height: captured_height,
                depth_or_array_layers: 1,
            },
        );
        self.capture_meta = [
            pad as f32,
            pad as f32,
            captured_width as f32,
            captured_height as f32,
        ];
        self.capture_error_logged = false;
        Ok(())
    }

    pub fn disable_backdrop(&mut self) {
        self.capture_meta = [0.0; 4];
    }

    /// Development helper: feed a synthetic BGRA image through the same lens
    /// path as the live desktop capture.
    pub fn set_backdrop_image(&mut self, pixels: &[u8], width: u32, height: u32) {
        let (texture, view) = create_backdrop_texture(&self.device, width, height);
        self.queue.write_texture(
            wgpu::ImageCopyTexture {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            pixels,
            wgpu::ImageDataLayout {
                offset: 0,
                bytes_per_row: Some(width * 4),
                rows_per_image: Some(height),
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );
        self.backdrop_texture = texture;
        self.backdrop_view = view;
        self.capture_size = (width, height);
        self.capture_meta = [0.0, 0.0, width as f32, height as f32];
        self.rebuild_composite_bind_group();
    }

    pub fn render(&mut self, params: &FrameParams) -> Result<&[u8], String> {
        let uniforms = Uniforms {
            resolution: [self.width as f32, self.height as f32],
            time: params.time,
            hole_radius: params.hole_radius,
            camera: [params.yaw, params.pitch, params.roll, params.distance],
            disk: [
                params.disk_inner,
                params.disk_outer,
                params.disk_thickness,
                params.disk_brightness,
            ],
            look: [params.exposure, params.glow, params.pulse, params.pulse_age],
            motion: [
                params.fade,
                params.hover_x,
                params.hover_y,
                params.motion_amount,
            ],
            flags: [params.debug as f32, params.lens_strength, 1.0, 0.0],
            capture: self.capture_meta,
            accent: [params.accent[0], params.accent[1], params.accent[2], 1.0],
            fun: params.fun,
            extra: [params.focus, 0.0, 0.0, 0.0],
        };
        self.queue
            .write_buffer(&self.uniform_buffer, 0, bytemuck::bytes_of(&uniforms));

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("singularity-encoder"),
            });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("singularity-scene-pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &self.scene_view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            pass.set_pipeline(&self.scene_pipeline);
            pass.set_bind_group(0, &self.scene_bind_group, &[]);
            pass.draw(0..3, 0..1);
        }
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("singularity-composite-pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &self.target_view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            pass.set_pipeline(&self.composite_pipeline);
            pass.set_bind_group(0, &self.composite_bind_group, &[]);
            pass.draw(0..3, 0..1);
        }
        encoder.copy_texture_to_buffer(
            wgpu::ImageCopyTexture {
                texture: &self.target,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::ImageCopyBuffer {
                buffer: &self.readback,
                layout: wgpu::ImageDataLayout {
                    offset: 0,
                    bytes_per_row: Some(self.padded_bytes_per_row),
                    rows_per_image: Some(self.height),
                },
            },
            wgpu::Extent3d {
                width: self.width,
                height: self.height,
                depth_or_array_layers: 1,
            },
        );
        self.queue.submit(Some(encoder.finish()));

        let slice = self.readback.slice(..);
        slice.map_async(wgpu::MapMode::Read, |_| {});
        self.device.poll(wgpu::Maintain::Wait);
        let mapped = slice.get_mapped_range();
        let row_bytes = (self.width * 4) as usize;
        let padded = self.padded_bytes_per_row as usize;
        for y in 0..self.height as usize {
            let src = &mapped[y * padded..y * padded + row_bytes];
            let dst = &mut self.pixels[y * row_bytes..(y + 1) * row_bytes];
            dst.copy_from_slice(src);
        }
        drop(mapped);
        self.readback.unmap();
        Ok(&self.pixels)
    }
}

fn padded_row_bytes(width: u32) -> u32 {
    let unaligned = width * 4;
    (unaligned + 255) & !255
}

fn create_target(device: &wgpu::Device, width: u32, height: u32) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some("singularity-target"),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: TARGET_FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    })
}

fn create_backdrop_texture(
    device: &wgpu::Device,
    width: u32,
    height: u32,
) -> (wgpu::Texture, wgpu::TextureView) {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("singularity-backdrop"),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Bgra8Unorm,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    (texture, view)
}

fn create_hdr_target(device: &wgpu::Device, width: u32, height: u32) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some("singularity-hdr-target"),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: HDR_FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    })
}

fn create_readback(device: &wgpu::Device, width: u32, height: u32) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("singularity-readback"),
        size: (padded_row_bytes(width) as u64) * height as u64,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    })
}

fn create_disk_texture(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
) -> Result<(wgpu::Texture, wgpu::TextureView, wgpu::Sampler), String> {
    let data = build_disk_texture();
    let size = wgpu::Extent3d {
        width: DISK_TEXTURE_WIDTH,
        height: DISK_TEXTURE_HEIGHT,
        depth_or_array_layers: 1,
    };
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("singularity-disk-texture"),
        size,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    queue.write_texture(
        wgpu::ImageCopyTexture {
            texture: &texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        &data,
        wgpu::ImageDataLayout {
            offset: 0,
            bytes_per_row: Some(DISK_TEXTURE_WIDTH * 4),
            rows_per_image: Some(DISK_TEXTURE_HEIGHT),
        },
        size,
    );
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
        label: Some("singularity-disk-sampler"),
        address_mode_u: wgpu::AddressMode::Repeat,
        address_mode_v: wgpu::AddressMode::ClampToEdge,
        address_mode_w: wgpu::AddressMode::ClampToEdge,
        mag_filter: wgpu::FilterMode::Linear,
        min_filter: wgpu::FilterMode::Linear,
        mipmap_filter: wgpu::FilterMode::Linear,
        ..Default::default()
    });
    Ok((texture, view, sampler))
}
