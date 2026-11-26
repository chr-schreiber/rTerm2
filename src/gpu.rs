//! GPU-Renderer mit wgpu (Vulkan, notfalls OpenGL).
//!
//! Prinzip: Alles, was gezeichnet wird, ist ein Rechteck („Instanz“) – Hintergründe, Cursor,
//! Trennlinien, Tab-Leiste und jedes Zeichen. Zeichen werden einmal mit fontdue gerastert und in
//! eine Atlas-Textur (R8) geladen. Pro Bild geht nur die Liste der Instanzen an die GPU, gezeichnet
//! wird mit einem einzigen Draw-Call in Aufrufreihenfolge (Späteres liegt oben).

use std::collections::HashMap;
use std::sync::Arc;

use anyhow::{anyhow, Context};
use winit::window::Window;

use crate::font::FontAtlas;
use crate::quad::{clip_quad, intersect, srgb_to_linear, Quad, ShelfPacker};
use crate::renderer::{Canvas, Rect};
use crate::theme::Rgb;

/// Kantenlänge der Atlas-Textur. 2048 ist auch auf schwacher Hardware (GLES 3) erlaubt.
const ATLAS_SIZE: u32 = 2048;
/// Rechteck (4) + UV (4) + Farbe (4)
const FLOATS_PER_INSTANCE: usize = 12;
const INSTANCE_BYTES: u64 = (FLOATS_PER_INSTANCE * 4) as u64;

const INSTANCE_ATTRS: [wgpu::VertexAttribute; 3] =
    wgpu::vertex_attr_array![0 => Float32x4, 1 => Float32x4, 2 => Float32x4];

/// Von allen Fenstern geteilt: Instanz, Adapter, Device, Queue.
pub struct GpuContext {
    instance: wgpu::Instance,
    adapter: wgpu::Adapter,
    device: wgpu::Device,
    queue: wgpu::Queue,
}

impl GpuContext {
    /// Sucht eine Grafikkarte, die zur Surface des ersten Fensters passt.
    pub fn new(window: Arc<Window>) -> anyhow::Result<(Arc<Self>, wgpu::Surface<'static>)> {
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor::default());
        let surface = instance
            .create_surface(window)
            .context("GPU-Surface anlegen")?;
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            // Ein Terminal braucht keine dedizierte GPU – spart Akku auf Laptops.
            power_preference: wgpu::PowerPreference::LowPower,
            compatible_surface: Some(&surface),
            force_fallback_adapter: false,
        }))
        .context("Keine passende Grafikkarte gefunden")?;

        let info = adapter.get_info();
        log::info!(
            "GPU: {} ({:?}, Treiber {})",
            info.name,
            info.backend,
            info.driver
        );

        let limits = wgpu::Limits::downlevel_webgl2_defaults().using_resolution(adapter.limits());
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("rterm2"),
            required_limits: limits,
            ..Default::default()
        }))
        .context("GPU-Device anlegen")?;

        Ok((
            Arc::new(Self {
                instance,
                adapter,
                device,
                queue,
            }),
            surface,
        ))
    }

    /// Surface für ein weiteres Fenster.
    pub fn create_surface(&self, window: Arc<Window>) -> anyhow::Result<wgpu::Surface<'static>> {
        self.instance
            .create_surface(window)
            .context("GPU-Surface anlegen")
    }
}

#[derive(Clone, Copy)]
struct AtlasGlyph {
    /// Position im Atlas (Pixel)
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    xmin: f32,
    ymin: f32,
}

/// Glyph-Textur + Belegung. Wird geleert, wenn sich die Schrift ändert oder der Platz ausgeht.
struct GlyphAtlas {
    texture: wgpu::Texture,
    packer: ShelfPacker,
    glyphs: HashMap<char, Option<AtlasGlyph>>,
    font_generation: u64,
}

impl GlyphAtlas {
    fn reset(&mut self) {
        self.glyphs.clear();
        self.packer.reset();
    }

    /// Glyph aus dem Atlas holen; beim ersten Mal rastern und hochladen. `None` = nichts zu zeichnen.
    fn lookup(&mut self, queue: &wgpu::Queue, font: &mut FontAtlas, c: char) -> Option<AtlasGlyph> {
        if font.generation != self.font_generation {
            self.reset();
            self.font_generation = font.generation;
        }
        if let Some(g) = self.glyphs.get(&c) {
            return *g;
        }

        let g = font.glyph(c);
        let (w, h) = (g.metrics.width as u32, g.metrics.height as u32);
        let entry = if w == 0 || h == 0 {
            None
        } else {
            let pos = match self.packer.alloc(w, h) {
                Some(p) => Some(p),
                None => {
                    // Atlas voll: neu anfangen (sehr selten; kann ein Bild lang falsche Zeichen zeigen)
                    log::info!("Glyph-Atlas voll – wird neu aufgebaut");
                    self.glyphs.clear();
                    self.packer.reset();
                    self.packer.alloc(w, h)
                }
            };
            pos.map(|(x, y)| {
                queue.write_texture(
                    wgpu::TexelCopyTextureInfo {
                        texture: &self.texture,
                        mip_level: 0,
                        origin: wgpu::Origin3d { x, y, z: 0 },
                        aspect: wgpu::TextureAspect::All,
                    },
                    &g.coverage,
                    wgpu::TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(w),
                        rows_per_image: Some(h),
                    },
                    wgpu::Extent3d {
                        width: w,
                        height: h,
                        depth_or_array_layers: 1,
                    },
                );
                AtlasGlyph {
                    x: x as f32,
                    y: y as f32,
                    w: w as f32,
                    h: h as f32,
                    xmin: g.metrics.xmin as f32,
                    ymin: g.metrics.ymin as f32,
                }
            })
        };
        self.glyphs.insert(c, entry);
        entry
    }
}

/// GPU-Zustand eines Fensters.
pub struct GpuRenderer {
    ctx: Arc<GpuContext>,
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    srgb: bool,
    pipeline: wgpu::RenderPipeline,
    bind_group: wgpu::BindGroup,
    uniforms: wgpu::Buffer,
    instances: wgpu::Buffer,
    instance_capacity: u64,
    atlas: GlyphAtlas,
    data: Vec<f32>,
    clear: Rgb,
}

impl GpuRenderer {
    pub fn new(
        ctx: Arc<GpuContext>,
        surface: wgpu::Surface<'static>,
        width: u32,
        height: u32,
    ) -> anyhow::Result<Self> {
        let device = &ctx.device;
        let caps = surface.get_capabilities(&ctx.adapter);
        // Ohne sRGB-Wandlung mischen GPU und CPU-Renderer die Farben gleich.
        let format = caps
            .formats
            .iter()
            .copied()
            .find(|f| !f.is_srgb())
            .or_else(|| caps.formats.first().copied())
            .ok_or_else(|| anyhow!("Die Surface bietet keine Bildformate an"))?;
        let mut config = surface
            .get_default_config(&ctx.adapter, width.max(1), height.max(1))
            .ok_or_else(|| anyhow!("Die Grafikkarte kann in dieses Fenster nicht zeichnen"))?;
        config.format = format;
        config.view_formats = vec![];
        config.present_mode = wgpu::PresentMode::AutoVsync;
        surface.configure(device, &config);
        log::info!(
            "GPU-Surface: {format:?}, {}×{}",
            config.width,
            config.height
        );

        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("glyph-atlas"),
            size: wgpu::Extent3d {
                width: ATLAS_SIZE,
                height: ATLAS_SIZE,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::R8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("glyph-sampler"),
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });
        let uniforms = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("globals"),
            size: 16,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("rterm2"),
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
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("rterm2"),
            layout: &layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: uniforms.as_entire_binding(),
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

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("rterm2"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shader.wgsl").into()),
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("rterm2"),
            bind_group_layouts: &[&layout],
            push_constant_ranges: &[],
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("rterm2"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: INSTANCE_BYTES,
                    step_mode: wgpu::VertexStepMode::Instance,
                    attributes: &INSTANCE_ATTRS,
                }],
            },
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleStrip,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview: None,
            cache: None,
        });

        let instance_capacity = 4096 * INSTANCE_BYTES;
        let instances = Self::instance_buffer(device, instance_capacity);

        Ok(Self {
            srgb: format.is_srgb(),
            atlas: GlyphAtlas {
                texture,
                packer: ShelfPacker::new(ATLAS_SIZE),
                glyphs: HashMap::new(),
                font_generation: u64::MAX,
            },
            ctx,
            surface,
            config,
            pipeline,
            bind_group,
            uniforms,
            instances,
            instance_capacity,
            data: Vec::new(),
            clear: Rgb::hex(0),
        })
    }

    fn instance_buffer(device: &wgpu::Device, size: u64) -> wgpu::Buffer {
        device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("instances"),
            size,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        })
    }

    /// Ein Bild zeichnen: `paint` füllt die Instanzliste über das `Canvas`-Interface.
    pub fn render(&mut self, width: u32, height: u32, paint: impl FnOnce(&mut dyn Canvas)) {
        if width == 0 || height == 0 {
            return;
        }
        if self.config.width != width || self.config.height != height {
            self.config.width = width;
            self.config.height = height;
            self.surface.configure(&self.ctx.device, &self.config);
        }

        self.data.clear();
        {
            let mut canvas = GpuCanvas {
                queue: &self.ctx.queue,
                atlas: &mut self.atlas,
                data: &mut self.data,
                clear: &mut self.clear,
                srgb: self.srgb,
                screen: [0.0, 0.0, width as f32, height as f32],
            };
            paint(&mut canvas);
        }

        let device = &self.ctx.device;
        let queue = &self.ctx.queue;
        let bytes = as_bytes(&self.data);
        if bytes.len() as u64 > self.instance_capacity {
            self.instance_capacity = (bytes.len() as u64).next_power_of_two();
            self.instances = Self::instance_buffer(device, self.instance_capacity);
        }
        if !bytes.is_empty() {
            queue.write_buffer(&self.instances, 0, bytes);
        }
        queue.write_buffer(
            &self.uniforms,
            0,
            as_bytes(&[width as f32, height as f32, 0.0, 0.0]),
        );

        let frame = match self.surface.get_current_texture() {
            Ok(f) => f,
            Err(wgpu::SurfaceError::Lost | wgpu::SurfaceError::Outdated) => {
                // Z. B. nach Größenänderung oder Monitorwechsel: neu einrichten, nächstes Bild klappt
                self.surface.configure(device, &self.config);
                return;
            }
            Err(e) => {
                log::warn!("GPU-Bild nicht verfügbar: {e}");
                return;
            }
        };
        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("rterm2"),
        });
        {
            let [r, g, b] = self.color3(self.clear);
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("rterm2"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: r as f64,
                            g: g as f64,
                            b: b as f64,
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            let count = (self.data.len() / FLOATS_PER_INSTANCE) as u32;
            if count > 0 {
                pass.set_pipeline(&self.pipeline);
                pass.set_bind_group(0, &self.bind_group, &[]);
                pass.set_vertex_buffer(0, self.instances.slice(..bytes.len() as u64));
                pass.draw(0..4, 0..count);
            }
        }
        queue.submit(Some(encoder.finish()));
        frame.present();
    }

    fn color3(&self, c: Rgb) -> [f32; 3] {
        color3(c, self.srgb)
    }
}

fn color3(c: Rgb, srgb: bool) -> [f32; 3] {
    if srgb {
        [
            srgb_to_linear(c.r),
            srgb_to_linear(c.g),
            srgb_to_linear(c.b),
        ]
    } else {
        [c.r as f32 / 255.0, c.g as f32 / 255.0, c.b as f32 / 255.0]
    }
}

fn as_bytes(v: &[f32]) -> &[u8] {
    // f32 hat keine ungültigen Bitmuster und keine Füllbytes – die Umdeutung ist sicher.
    unsafe { std::slice::from_raw_parts(v.as_ptr().cast::<u8>(), std::mem::size_of_val(v)) }
}

/// `Canvas` für ein Bild auf der GPU: sammelt Instanzen.
struct GpuCanvas<'a> {
    queue: &'a wgpu::Queue,
    atlas: &'a mut GlyphAtlas,
    data: &'a mut Vec<f32>,
    clear: &'a mut Rgb,
    srgb: bool,
    screen: Quad,
}

impl GpuCanvas<'_> {
    fn push(&mut self, dst: Quad, uv: Quad, color: Rgb, alpha: f32) {
        let [r, g, b] = color3(color, self.srgb);
        self.data.extend_from_slice(&dst);
        self.data.extend_from_slice(&uv);
        self.data.extend_from_slice(&[r, g, b, alpha]);
    }

    fn clip(&self, clip: Rect) -> Option<Quad> {
        intersect(rect_quad(clip), self.screen)
    }
}

fn rect_quad(r: Rect) -> Quad {
    [r.x as f32, r.y as f32, r.w as f32, r.h as f32]
}

impl Canvas for GpuCanvas<'_> {
    fn clear(&mut self, color: Rgb) {
        *self.clear = color;
        self.data.clear();
    }

    fn fill(&mut self, clip: Rect, x: usize, y: usize, w: usize, h: usize, color: Rgb) {
        let Some(clip) = self.clip(clip) else { return };
        if let Some(d) = intersect([x as f32, y as f32, w as f32, h as f32], clip) {
            self.push(d, [0.0; 4], color, 1.0);
        }
    }

    fn tint(&mut self, r: Rect, color: Rgb, alpha: u8) {
        if let Some(d) = intersect(rect_quad(r), self.screen) {
            self.push(d, [0.0; 4], color, alpha as f32 / 255.0);
        }
    }

    fn glyph(&mut self, clip: Rect, font: &mut FontAtlas, c: char, x: usize, y: usize, color: Rgb) {
        let Some(clip) = self.clip(clip) else { return };
        let ascent = font.ascent;
        let Some(g) = self.atlas.lookup(self.queue, font, c) else {
            return;
        };
        let baseline = y as f32 + ascent.round();
        let dst = [x as f32 + g.xmin, baseline - (g.h + g.ymin), g.w, g.h];
        let src = [g.x, g.y, g.w, g.h];
        if let Some((d, s)) = clip_quad(dst, src, clip) {
            let n = ATLAS_SIZE as f32;
            self.push(d, [s[0] / n, s[1] / n, s[2] / n, s[3] / n], color, 1.0);
        }
    }
}
