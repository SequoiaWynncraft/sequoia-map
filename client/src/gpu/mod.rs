use std::collections::HashMap;

use wasm_bindgen::JsCast;
use web_sys::HtmlCanvasElement;
use wgpu::util::DeviceExt;

use sequoia_shared::TreasuryLevel;
use sequoia_shared::colors::hsl_to_rgb;

use crate::territory::ClientTerritoryMap;
use crate::tiles::{LoadedTile, TileQuality};
use crate::viewport::Viewport;

pub struct RenderFrameInput<'a> {
    pub vp: &'a Viewport,
    pub territories: &'a ClientTerritoryMap,
    pub hovered: &'a Option<String>,
    pub selected: &'a Option<String>,
    pub tiles: &'a [LoadedTile],
    pub world_bounds: Option<(f64, f64, f64, f64)>,
    pub now: f64,
    pub reference_time_secs: i64,
}

// --- GPU data types ---

#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
struct Vertex {
    position: [f32; 2],
}

const QUAD_VERTICES: &[Vertex] = &[
    Vertex {
        position: [0.0, 0.0],
    },
    Vertex {
        position: [1.0, 0.0],
    },
    Vertex {
        position: [0.0, 1.0],
    },
    Vertex {
        position: [1.0, 1.0],
    },
];

const QUAD_INDICES: &[u16] = &[0, 1, 2, 2, 1, 3];

#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
struct ViewportUniform {
    offset: [f32; 2],
    scale: f32,
    time: f32,
    resolution: [f32; 2],
    _pad1: [f32; 2],
}

/// Per-territory instance data: 28 floats = 112 bytes.
#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct TerritoryInstance {
    pub rect: [f32; 4],          // x, y, width, height (world coords)
    pub color: [f32; 4],         // r, g, b, 1.0 — target/static guild color
    pub state: [f32; 4],         // fill_alpha, border_alpha, flags, 0.0
    pub cooldown: [f32; 4],      // cooldown_frac, treasury_r, treasury_g, treasury_b
    pub anim_color: [f32; 4],    // from_r, from_g, from_b, 0.0
    pub anim_time: [f32; 4],     // start_time_relative_secs, duration_secs, 0, 0
    pub resource_data: [f32; 4], // mode, idx_a, idx_b, flags
}

#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
struct GlowUniform {
    rect: [f32; 4],
    glow_color: [f32; 4],
    expand: f32,
    falloff: f32,
    ring_width: f32,
    fill_tint_alpha: f32,
    fill_tint_rgb: [f32; 3],
    _pad: f32,
}

#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
struct TileRectUniform {
    rect: [f32; 4],
}

// --- Tile texture cache ---

struct TileTexture {
    bind_group: wgpu::BindGroup,
    rect: [f32; 4], // [x, z, width, height] in world coords
    quality: TileQuality,
}

// --- GpuRenderer ---

pub struct GpuRenderer {
    device: wgpu::Device,
    queue: wgpu::Queue,
    surface: wgpu::Surface<'static>,
    surface_config: wgpu::SurfaceConfiguration,

    // Shared geometry
    vertex_buffer: wgpu::Buffer,
    index_buffer: wgpu::Buffer,

    // Viewport uniform (shared by all pipelines)
    viewport_buffer: wgpu::Buffer,
    viewport_bind_group: wgpu::BindGroup,

    // Territory fill+border pipeline (instanced)
    territory_pipeline: wgpu::RenderPipeline,
    instance_buffer: wgpu::Buffer,
    instance_count: u32,
    instance_capacity: u32,

    // Glow pipeline (1-2 quads for selection/hover)
    glow_pipeline: wgpu::RenderPipeline,
    glow_buffer_sel: wgpu::Buffer,
    glow_bind_group_sel: wgpu::BindGroup,
    glow_buffer_hov: wgpu::Buffer,
    glow_bind_group_hov: wgpu::BindGroup,

    // Tile pipeline
    tile_pipeline: wgpu::RenderPipeline,
    tile_bind_group_layout: wgpu::BindGroupLayout,
    tile_sampler: wgpu::Sampler,
    tile_textures: HashMap<usize, TileTexture>,

    // Track current dimensions
    width: u32,
    height: u32,
    dpr: f32,

    // Dirty tracking: skip instance rebuild during pan/zoom
    instance_dirty: bool,

    // Cached max animation end time (epoch ms) — avoids scanning all
    // territories every frame just to check if animations are active.
    max_anim_end_ms: f64,

    // Relative timing: epoch ms at init, for f32-safe shader time
    start_time_ms: f64,

    // Persistent instance buffer to avoid per-rebuild allocation
    instances_buf: Vec<TerritoryInstance>,

    // FPS tracking
    frame_count: u32,
    fps_log_time: f64,

    // Settings
    pub thick_cooldown_borders: bool,
    pub resource_highlight: bool,
}

impl GpuRenderer {
    /// Async initialization with a WebGL2-only path.
    pub async fn init(canvas: HtmlCanvasElement) -> Result<Self, String> {
        web_sys::console::log_1(&"wgpu init: using WebGL2 backend (WebGPU disabled)".into());
        Self::init_with_backends(canvas, wgpu::Backends::GL, "webgl").await
    }

    /// Core initialization parameterized by backend selection.
    async fn init_with_backends(
        canvas: HtmlCanvasElement,
        backends: wgpu::Backends,
        backend_path: &str,
    ) -> Result<Self, String> {
        let width = canvas.width().max(1);
        let height = canvas.height().max(1);
        let rect = canvas.get_bounding_client_rect();
        let css_width = rect.width() as f32;
        let dpr = if css_width > 0.0 {
            (width as f32 / css_width).max(0.5)
        } else {
            web_sys::window()
                .map(|w| w.device_pixel_ratio() as f32)
                .unwrap_or(1.0)
        };

        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
            backends,
            ..Default::default()
        });

        let surface_target = wgpu::SurfaceTarget::Canvas(canvas);
        let surface = instance
            .create_surface(surface_target)
            .map_err(|e| format!("wgpu init ({backend_path}) create_surface: {e}"))?;

        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: Some(&surface),
                ..Default::default()
            })
            .await
            .ok_or_else(|| format!("wgpu init ({backend_path}): no suitable GPU adapter found"))?;

        // WebGL2 adapters expose zero compute limits, so requesting the plain
        // default limits (which include compute) fails validation.
        let required_limits = if backends == wgpu::Backends::GL {
            wgpu::Limits::downlevel_webgl2_defaults().using_resolution(adapter.limits())
        } else {
            wgpu::Limits::default()
        };

        let (device, queue) = adapter
            .request_device(
                &wgpu::DeviceDescriptor {
                    label: Some("sequoia-device"),
                    required_features: wgpu::Features::empty(),
                    required_limits,
                    ..Default::default()
                },
                None,
            )
            .await
            .map_err(|e| format!("wgpu init ({backend_path}) request_device: {e}"))?;

        let mut surface_config = surface
            .get_default_config(&adapter, width, height)
            .ok_or_else(|| format!("wgpu init ({backend_path}): surface unsupported by adapter"))?;
        let caps = surface.get_capabilities(&adapter);

        // Prefer a non-sRGB format so tile textures (uploaded as Rgba8Unorm)
        // pass through without double gamma correction that washes out colors.
        if let Some(format) = caps.formats.iter().copied().find(|f| !f.is_srgb()) {
            surface_config.format = format;
        }

        // Opaque canvases avoid compositor alpha blending on every frame.
        if caps.alpha_modes.contains(&wgpu::CompositeAlphaMode::Opaque) {
            surface_config.alpha_mode = wgpu::CompositeAlphaMode::Opaque;
        } else if caps
            .alpha_modes
            .contains(&wgpu::CompositeAlphaMode::PreMultiplied)
        {
            surface_config.alpha_mode = wgpu::CompositeAlphaMode::PreMultiplied;
        }
        let format = surface_config.format;

        web_sys::console::log_1(
            &format!(
                "wgpu init: path={backend_path} format={:?} present={:?} alpha={:?} latency={}",
                surface_config.format,
                surface_config.present_mode,
                surface_config.alpha_mode,
                surface_config.desired_maximum_frame_latency,
            )
            .into(),
        );
        surface.configure(&device, &surface_config);

        // --- Shared geometry ---
        let vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("quad-verts"),
            contents: bytemuck::cast_slice(QUAD_VERTICES),
            usage: wgpu::BufferUsages::VERTEX,
        });

        let index_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("quad-indices"),
            contents: bytemuck::cast_slice(QUAD_INDICES),
            usage: wgpu::BufferUsages::INDEX,
        });

        // --- Viewport uniform ---
        let viewport_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("viewport-bgl"),
                entries: &[wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                }],
            });

        let viewport_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("viewport-ubo"),
            contents: bytemuck::cast_slice(&[ViewportUniform {
                offset: [0.0, 0.0],
                scale: 1.0,
                time: 0.0,
                resolution: [width as f32 / dpr, height as f32 / dpr],
                _pad1: [0.0, 0.0],
            }]),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });

        let viewport_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("viewport-bg"),
            layout: &viewport_bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: viewport_buffer.as_entire_binding(),
            }],
        });

        // --- Territory pipeline ---
        let territory_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("territory-shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("territory.wgsl").into()),
        });

        let vertex_layout = wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Vertex>() as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &[wgpu::VertexAttribute {
                offset: 0,
                shader_location: 0,
                format: wgpu::VertexFormat::Float32x2,
            }],
        };

        let instance_layout = wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<TerritoryInstance>() as u64,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &[
                wgpu::VertexAttribute {
                    offset: 0,
                    shader_location: 1,
                    format: wgpu::VertexFormat::Float32x4, // rect
                },
                wgpu::VertexAttribute {
                    offset: 16,
                    shader_location: 2,
                    format: wgpu::VertexFormat::Float32x4, // color
                },
                wgpu::VertexAttribute {
                    offset: 32,
                    shader_location: 3,
                    format: wgpu::VertexFormat::Float32x4, // state
                },
                wgpu::VertexAttribute {
                    offset: 48,
                    shader_location: 4,
                    format: wgpu::VertexFormat::Float32x4, // cooldown
                },
                wgpu::VertexAttribute {
                    offset: 64,
                    shader_location: 5,
                    format: wgpu::VertexFormat::Float32x4, // anim_color
                },
                wgpu::VertexAttribute {
                    offset: 80,
                    shader_location: 6,
                    format: wgpu::VertexFormat::Float32x4, // anim_time
                },
                wgpu::VertexAttribute {
                    offset: 96,
                    shader_location: 7,
                    format: wgpu::VertexFormat::Float32x4, // resource_data
                },
            ],
        };

        let territory_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("territory-pl"),
                bind_group_layouts: &[&viewport_bind_group_layout],
                push_constant_ranges: &[],
            });

        let territory_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("territory-pipeline"),
            layout: Some(&territory_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &territory_shader,
                entry_point: Some("vs_main"),
                buffers: &[vertex_layout.clone(), instance_layout],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &territory_shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                cull_mode: None,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        let initial_capacity = 256u32;
        let instance_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("instance-buf"),
            size: (initial_capacity as u64) * std::mem::size_of::<TerritoryInstance>() as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        // --- Glow pipeline ---
        let glow_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("glow-shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("glow.wgsl").into()),
        });

        let glow_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("glow-bgl"),
                entries: &[wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                }],
            });

        let glow_buffer_sel = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("glow-ubo-sel"),
            size: std::mem::size_of::<GlowUniform>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let glow_bind_group_sel = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("glow-bg-sel"),
            layout: &glow_bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: glow_buffer_sel.as_entire_binding(),
            }],
        });

        let glow_buffer_hov = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("glow-ubo-hov"),
            size: std::mem::size_of::<GlowUniform>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let glow_bind_group_hov = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("glow-bg-hov"),
            layout: &glow_bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: glow_buffer_hov.as_entire_binding(),
            }],
        });

        let glow_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("glow-pl"),
            bind_group_layouts: &[&viewport_bind_group_layout, &glow_bind_group_layout],
            push_constant_ranges: &[],
        });

        let glow_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("glow-pipeline"),
            layout: Some(&glow_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &glow_shader,
                entry_point: Some("vs_main"),
                buffers: &[vertex_layout.clone()],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &glow_shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState {
                        color: wgpu::BlendComponent {
                            src_factor: wgpu::BlendFactor::SrcAlpha,
                            dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                            operation: wgpu::BlendOperation::Add,
                        },
                        alpha: wgpu::BlendComponent {
                            src_factor: wgpu::BlendFactor::One,
                            dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                            operation: wgpu::BlendOperation::Add,
                        },
                    }),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                cull_mode: None,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        // --- Tile pipeline ---
        let tile_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("tile-shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("tile.wgsl").into()),
        });

        let tile_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("tile-bgl"),
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

        let tile_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("tile-sampler"),
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            ..Default::default()
        });

        let tile_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("tile-pl"),
            bind_group_layouts: &[&viewport_bind_group_layout, &tile_bind_group_layout],
            push_constant_ranges: &[],
        });

        let tile_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("tile-pipeline"),
            layout: Some(&tile_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &tile_shader,
                entry_point: Some("vs_main"),
                buffers: &[vertex_layout],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &tile_shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                cull_mode: None,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        Ok(Self {
            device,
            queue,
            surface,
            surface_config,
            vertex_buffer,
            index_buffer,
            viewport_buffer,
            viewport_bind_group,
            territory_pipeline,
            instance_buffer,
            instance_count: 0,
            instance_capacity: initial_capacity,
            glow_pipeline,
            glow_buffer_sel,
            glow_bind_group_sel,
            glow_buffer_hov,
            glow_bind_group_hov,
            tile_pipeline,
            tile_bind_group_layout,
            tile_sampler,
            tile_textures: HashMap::new(),
            width,
            height,
            dpr,
            instance_dirty: true,
            max_anim_end_ms: 0.0,
            start_time_ms: js_sys::Date::now(),
            instances_buf: Vec::new(),
            frame_count: 0,
            fps_log_time: 0.0,
            thick_cooldown_borders: false,
            resource_highlight: false,
        })
    }

    /// Mark instance data as needing a rebuild (territory/hover/select/tick changed).
    pub fn mark_instance_dirty(&mut self) {
        self.instance_dirty = true;
    }

    /// Resize the surface when the canvas size changes.
    pub fn resize(&mut self, width: u32, height: u32, dpr: f32) {
        if width == 0 || height == 0 {
            return;
        }
        self.width = width;
        self.height = height;
        self.dpr = dpr;
        self.surface_config.width = width;
        self.surface_config.height = height;
        self.surface.configure(&self.device, &self.surface_config);
    }

    /// Upload tile images as GPU textures with pre-baked rect uniforms.
    pub fn upload_tiles(&mut self, tiles: &[LoadedTile]) {
        let Some(document) = web_sys::window().and_then(|window| window.document()) else {
            web_sys::console::warn_1(&"Skipping tile upload: document is unavailable".into());
            return;
        };

        for tile in tiles {
            let tile_id = tile.id;
            if let Some(existing) = self.tile_textures.get(&tile_id)
                && existing.quality >= tile.quality
            {
                continue;
            }

            let img = &tile.image;
            let w = img.natural_width();
            let h = img.natural_height();
            if w == 0 || h == 0 {
                continue;
            }

            // Draw image to a temporary canvas to extract pixel data
            let Some(tmp_canvas) = document
                .create_element("canvas")
                .ok()
                .and_then(|element| element.dyn_into::<HtmlCanvasElement>().ok())
            else {
                continue;
            };
            tmp_canvas.set_width(w);
            tmp_canvas.set_height(h);
            let Some(tmp_ctx) = tmp_canvas
                .get_context("2d")
                .ok()
                .flatten()
                .and_then(|ctx| ctx.dyn_into::<web_sys::CanvasRenderingContext2d>().ok())
            else {
                continue;
            };
            tmp_ctx
                .draw_image_with_html_image_element(img, 0.0, 0.0)
                .ok();
            let image_data = match tmp_ctx.get_image_data(0.0, 0.0, w as f64, h as f64) {
                Ok(d) => d,
                Err(_) => continue,
            };
            let pixels = image_data.data();

            let texture = self.device.create_texture(&wgpu::TextureDescriptor {
                label: Some("tile-tex"),
                size: wgpu::Extent3d {
                    width: w,
                    height: h,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8Unorm,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            });

            self.queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                &pixels,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(4 * w),
                    rows_per_image: Some(h),
                },
                wgpu::Extent3d {
                    width: w,
                    height: h,
                    depth_or_array_layers: 1,
                },
            );

            let view = texture.create_view(&wgpu::TextureViewDescriptor::default());

            // Pre-compute tile world rect and bake into a dedicated uniform buffer
            let x1 = tile.x1.min(tile.x2) as f32;
            let z1 = tile.z1.min(tile.z2) as f32;
            // Tile bounds are inclusive — add 1 to get exclusive width/height
            let tw = (tile.x1.max(tile.x2) - tile.x1.min(tile.x2) + 1) as f32;
            let th = (tile.z1.max(tile.z2) - tile.z1.min(tile.z2) + 1) as f32;
            let rect = [x1, z1, tw, th];

            let rect_buffer = self
                .device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("tile-rect-ubo"),
                    contents: bytemuck::cast_slice(&[TileRectUniform { rect }]),
                    usage: wgpu::BufferUsages::UNIFORM,
                });

            let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("tile-bg"),
                layout: &self.tile_bind_group_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: rect_buffer.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::TextureView(&view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: wgpu::BindingResource::Sampler(&self.tile_sampler),
                    },
                ],
            });

            self.tile_textures.insert(
                tile_id,
                TileTexture {
                    bind_group,
                    rect,
                    quality: tile.quality,
                },
            );
        }
    }

    /// Build instance data from territories and upload to GPU.
    ///
    /// Animation color interpolation is handled GPU-side: we encode
    /// from_color + timing in the instance data once, and the shader
    /// computes the interpolated color every frame at zero CPU cost.
    fn update_instances(
        &mut self,
        territories: &ClientTerritoryMap,
        hovered: &Option<String>,
        selected: &Option<String>,
        now: f64,
        reference_time_secs: i64,
        thick_cooldown_borders: bool,
    ) {
        let start_ms = self.start_time_ms;

        self.instances_buf.clear();
        self.instances_buf
            .extend(territories.iter().map(|(name, ct)| {
                let loc = &ct.territory.location;
                let (r, g, b) = ct.guild_color;

                let is_hovered = hovered.as_deref() == Some(name.as_str());
                let is_selected = selected.as_deref() == Some(name.as_str());

                let resource_data = if self.resource_highlight {
                    ct.territory.resources.highlight_data()
                } else {
                    [0.0; 4]
                };
                let has_resource =
                    resource_data[0] > 0.5 || (resource_data[3] as u32 & (1 << 10)) != 0; // mode 0 + double emeralds

                let fill_alpha = if has_resource {
                    if is_selected {
                        0.48
                    } else if is_hovered {
                        0.40
                    } else {
                        0.30
                    }
                } else if is_selected {
                    0.35
                } else if is_hovered {
                    0.30
                } else {
                    0.22
                };

                let flags = (is_hovered as u32) + (is_selected as u32) * 2;

                let acquired_secs = ct.territory.acquired.timestamp();
                let age_secs = (reference_time_secs - acquired_secs).max(0);
                let cooldown_frac = if age_secs < 600 {
                    ((600 - age_secs) as f32 / 600.0).clamp(0.0, 1.0)
                } else {
                    0.0
                };

                let treasury_color = TreasuryLevel::from_held_seconds(age_secs).color_f32();

                // Encode animation params for GPU-side interpolation
                let (anim_color, anim_time) = match ct.animation.as_ref() {
                    Some(anim) if anim.current_color(now).is_some() => {
                        let (fr, fg, fb) =
                            hsl_to_rgb(anim.from_hsl.0, anim.from_hsl.1, anim.from_hsl.2);
                        let rel_start = ((anim.start_time - start_ms) / 1000.0) as f32;
                        let dur_secs = (anim.duration / 1000.0) as f32;
                        (
                            [fr as f32 / 255.0, fg as f32 / 255.0, fb as f32 / 255.0, 0.0],
                            [rel_start, dur_secs, 0.0, 0.0],
                        )
                    }
                    _ => ([0.0; 4], [0.0; 4]),
                };

                TerritoryInstance {
                    rect: [
                        loc.left() as f32,
                        loc.top() as f32,
                        loc.width() as f32,
                        loc.height() as f32,
                    ],
                    color: [r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0, 1.0],
                    state: [
                        fill_alpha,
                        0.65,
                        flags as f32,
                        if thick_cooldown_borders && cooldown_frac > 0.0 {
                            2.0
                        } else {
                            1.0
                        },
                    ],
                    cooldown: [
                        cooldown_frac,
                        treasury_color[0],
                        treasury_color[1],
                        treasury_color[2],
                    ],
                    anim_color,
                    anim_time,
                    resource_data,
                }
            }));

        self.instance_count = self.instances_buf.len() as u32;

        // Cache the latest animation end time so render() can check
        // has_anims with a single comparison instead of scanning all territories.
        self.max_anim_end_ms = territories
            .values()
            .filter_map(|ct| ct.animation.as_ref())
            .map(|a| a.start_time + a.duration)
            .fold(0.0f64, f64::max);

        if self.instance_count > self.instance_capacity {
            self.instance_capacity = self.instance_count.next_power_of_two();
            self.instance_buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("instance-buf"),
                size: (self.instance_capacity as u64)
                    * std::mem::size_of::<TerritoryInstance>() as u64,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
        }

        if !self.instances_buf.is_empty() {
            self.queue.write_buffer(
                &self.instance_buffer,
                0,
                bytemuck::cast_slice(&self.instances_buf),
            );
        }
    }

    /// Render a full frame. Returns true if animations are active.
    pub fn render(&mut self, frame: RenderFrameInput<'_>) -> bool {
        let RenderFrameInput {
            vp,
            territories,
            hovered,
            selected,
            tiles,
            world_bounds,
            now,
            reference_time_secs,
        } = frame;

        // FPS counter — log every 2 seconds
        self.frame_count += 1;
        if self.fps_log_time == 0.0 {
            self.fps_log_time = now;
        } else {
            let elapsed = now - self.fps_log_time;
            if elapsed >= 2000.0 {
                let fps = self.frame_count as f64 / (elapsed / 1000.0);
                web_sys::console::log_1(
                    &format!(
                        "fps: {fps:.1} ({} frames / {:.0}ms)",
                        self.frame_count, elapsed
                    )
                    .into(),
                );
                self.frame_count = 0;
                self.fps_log_time = now;
            }
        }

        // CSS pixel dimensions for viewport/culling (shaders work in CSS space)
        let w = self.width as f32 / self.dpr;
        let h = self.height as f32 / self.dpr;

        // Update viewport uniform
        self.queue.write_buffer(
            &self.viewport_buffer,
            0,
            bytemuck::cast_slice(&[ViewportUniform {
                offset: [vp.offset_x as f32, vp.offset_y as f32],
                scale: vp.scale as f32,
                time: ((now - self.start_time_ms) / 1000.0) as f32,
                resolution: [w, h],
                _pad1: [0.0, 0.0],
            }]),
        );

        // Update instance buffer only when state has changed.
        // Animation color interpolation is GPU-side — no per-frame rebuild needed.
        if self.instance_dirty {
            self.update_instances(
                territories,
                hovered,
                selected,
                now,
                reference_time_secs,
                self.thick_cooldown_borders,
            );
            self.instance_dirty = false;
        }

        // Pre-compute glow uniforms and write buffers BEFORE the render pass
        // to avoid pipeline stalls from mid-pass buffer writes on WebGL2/glow.
        let mut draw_sel_glow = false;
        let mut draw_hov_glow = false;

        if let Some(sel_name) = selected {
            if let Some(ct) = territories.get(sel_name) {
                let loc = &ct.territory.location;
                let (r, g, b) = ct.guild_color;
                let expand_world = 8.0 / vp.scale as f32;
                self.queue.write_buffer(
                    &self.glow_buffer_sel,
                    0,
                    bytemuck::cast_slice(&[GlowUniform {
                        rect: [
                            loc.left() as f32 - expand_world,
                            loc.top() as f32 - expand_world,
                            loc.width() as f32 + expand_world * 2.0,
                            loc.height() as f32 + expand_world * 2.0,
                        ],
                        glow_color: [r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0, 0.35],
                        expand: 6.0,
                        falloff: 0.03,
                        ring_width: 1.5,
                        fill_tint_alpha: 0.02,
                        fill_tint_rgb: [r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0],
                        _pad: 0.0,
                    }]),
                );
                draw_sel_glow = true;
            }
        }

        if let Some(hov_name) = hovered {
            if selected.as_deref() != Some(hov_name.as_str()) {
                if let Some(ct) = territories.get(hov_name) {
                    let loc = &ct.territory.location;
                    let (r, g, b) = ct.guild_color;
                    let expand_world = 5.0 / vp.scale as f32;
                    self.queue.write_buffer(
                        &self.glow_buffer_hov,
                        0,
                        bytemuck::cast_slice(&[GlowUniform {
                            rect: [
                                loc.left() as f32 - expand_world,
                                loc.top() as f32 - expand_world,
                                loc.width() as f32 + expand_world * 2.0,
                                loc.height() as f32 + expand_world * 2.0,
                            ],
                            glow_color: [
                                r as f32 / 255.0,
                                g as f32 / 255.0,
                                b as f32 / 255.0,
                                0.25,
                            ],
                            expand: 5.0,
                            falloff: 0.035,
                            ring_width: 1.0,
                            fill_tint_alpha: 0.0,
                            fill_tint_rgb: [0.0, 0.0, 0.0],
                            _pad: 0.0,
                        }]),
                    );
                    draw_hov_glow = true;
                }
            }
        }

        // Get surface texture
        let output = match self.surface.get_current_texture() {
            Ok(t) => t,
            Err(wgpu::SurfaceError::Lost | wgpu::SurfaceError::Outdated) => {
                self.surface.configure(&self.device, &self.surface_config);
                return false;
            }
            Err(_) => return false,
        };

        let view = output
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("render-encoder"),
            });

        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("main-pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: 0.047,
                            g: 0.055,
                            b: 0.090,
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                ..Default::default()
            });

            // Draw tiles
            if !tiles.is_empty() {
                pass.set_pipeline(&self.tile_pipeline);
                pass.set_bind_group(0, &self.viewport_bind_group, &[]);
                pass.set_vertex_buffer(0, self.vertex_buffer.slice(..));
                pass.set_index_buffer(self.index_buffer.slice(..), wgpu::IndexFormat::Uint16);

                for tile in tiles {
                    let Some(tile_tex) = self.tile_textures.get(&tile.id) else {
                        continue;
                    };

                    let [x1, z1, tw, th] = tile_tex.rect;
                    let x2 = x1 + tw;
                    let z2 = z1 + th;

                    // World bounds culling
                    if let Some((bx1, by1, bx2, by2)) = world_bounds {
                        let margin = 300.0;
                        if (x2 as f64) < bx1 - margin
                            || (x1 as f64) > bx2 + margin
                            || (z2 as f64) < by1 - margin
                            || (z1 as f64) > by2 + margin
                        {
                            continue;
                        }
                    }

                    // Frustum cull + screen-size cull (skip tiny tiles)
                    let sx = x1 * vp.scale as f32 + vp.offset_x as f32;
                    let sy = z1 * vp.scale as f32 + vp.offset_y as f32;
                    let sw = tw * vp.scale as f32;
                    let sh = th * vp.scale as f32;
                    if sx + sw < 0.0 || sy + sh < 0.0 || sx > w || sy > h {
                        continue;
                    }
                    // Skip tiles smaller than 4px on screen — saves draw calls
                    // and texture bandwidth at extreme zoom-out
                    if sw < 4.0 || sh < 4.0 {
                        continue;
                    }

                    pass.set_bind_group(1, &tile_tex.bind_group, &[]);
                    pass.draw_indexed(0..6, 0, 0..1);
                }
            }

            // Draw territory fills + borders (instanced)
            if self.instance_count > 0 {
                pass.set_pipeline(&self.territory_pipeline);
                pass.set_bind_group(0, &self.viewport_bind_group, &[]);
                pass.set_vertex_buffer(0, self.vertex_buffer.slice(..));
                pass.set_vertex_buffer(1, self.instance_buffer.slice(..));
                pass.set_index_buffer(self.index_buffer.slice(..), wgpu::IndexFormat::Uint16);
                pass.draw_indexed(0..6, 0, 0..self.instance_count);
            }

            // Glow draws — uniforms already written before pass to avoid
            // pipeline stalls from mid-pass buffer writes on WebGL2/glow
            if draw_sel_glow || draw_hov_glow {
                pass.set_pipeline(&self.glow_pipeline);
                pass.set_bind_group(0, &self.viewport_bind_group, &[]);
                pass.set_vertex_buffer(0, self.vertex_buffer.slice(..));
                pass.set_index_buffer(self.index_buffer.slice(..), wgpu::IndexFormat::Uint16);

                if draw_sel_glow {
                    pass.set_bind_group(1, &self.glow_bind_group_sel, &[]);
                    pass.draw_indexed(0..6, 0, 0..1);
                }

                if draw_hov_glow {
                    pass.set_bind_group(1, &self.glow_bind_group_hov, &[]);
                    pass.draw_indexed(0..6, 0, 0..1);
                }
            }
        }

        self.queue.submit(std::iter::once(encoder.finish()));
        output.present();

        // Check if any animations are still running (cached during instance rebuild)
        now < self.max_anim_end_ms
    }
}
