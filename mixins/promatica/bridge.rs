// Render side of the overlay, inserted into horizon-client-render's `gfx::native_render` as
// `promatica_bridge`: the game thread publishes quads here, the `world_drawn` hook draws them
// in a pass of their own after the world.
//
// The quads are what the entity renderer takes (`mesher::Vertex`, UVs in the shared atlas
// `WorldDrawn::atlas`, colour with the opacity in its alpha). `normal.w = 127` marks a quad with
// an outward normal: it is dropped while it faces away from the camera, so a ghost shows only
// its near faces. A depth buffer of the pass's own keeps the ghosts' inner faces hidden; the
// world's depth (`RenderNeeds::world_depth`) hides what is behind terrain, unless `through`.
// The highlights (`marks`) come in a second pass over all of it, never tested against the world.

use crate::mesher::Vertex;
use std::collections::HashMap;
use std::sync::Mutex;
use wgpu::util::DeviceExt;

/// What the game wants drawn: quads per 16x16x16 section, each section replaced on its own.
#[derive(Default)]
pub struct Scene {
    /// World block position the vertex positions are relative to (shared by every section).
    pub anchor: [i32; 3],
    /// Per section: its revision and quads (4 vertices each). Once uploaded the quads are let go
    /// (the GPU keeps them), so they take memory once, not twice.
    pub sections: HashMap<[i32; 3], (u64, Vec<Vertex>)>,
    /// The highlights, per section in the same way.
    pub marks: HashMap<[i32; 3], (u64, Vec<Vertex>)>,
    /// Set when the GPU side was made anew (another colour format) after quads were let go: the
    /// game builds every section again (`take_lost`).
    pub lost: bool,
    /// Bumped on every change.
    pub revision: u64,
    /// Draw through terrain (no test against the world's depth).
    pub through: bool,
    pub visible: bool,
}

pub static SCENE: Mutex<Option<Scene>> = Mutex::new(None);

fn scene<R>(f: impl FnOnce(&mut Scene) -> R) -> R {
    let mut s = SCENE.lock().unwrap_or_else(|e| e.into_inner());
    f(s.get_or_insert_with(Scene::default))
}

/// Sets the anchor; a new one drops every section (their positions were relative to the old).
pub fn set_anchor(anchor: [i32; 3]) {
    scene(|s| {
        if s.anchor != anchor {
            s.anchor = anchor;
            s.sections.clear();
            s.marks.clear();
            s.revision += 1;
        }
    });
}

/// Replaces section `key`'s quads (the game thread). Empty removes it.
pub fn put_section(key: [i32; 3], quads: Vec<Vertex>) {
    scene(|s| {
        s.revision += 1;
        if quads.is_empty() {
            s.sections.remove(&key);
        } else {
            let rev = s.revision;
            s.sections.insert(key, (rev, quads));
        }
    });
}

pub fn remove_section(key: [i32; 3]) {
    scene(|s| {
        if s.sections.remove(&key).is_some() {
            s.revision += 1;
        }
    });
}

/// Replaces section `key`'s highlights. Empty removes them.
pub fn put_marks(key: [i32; 3], quads: Vec<Vertex>) {
    scene(|s| {
        s.revision += 1;
        if quads.is_empty() {
            s.marks.remove(&key);
        } else {
            let rev = s.revision;
            s.marks.insert(key, (rev, quads));
        }
    });
}

/// Drops every highlight.
pub fn clear_marks() {
    scene(|s| {
        s.marks.clear();
        s.revision += 1;
    });
}

/// Whether the sections have to be handed over again (the GPU side was made anew).
pub fn take_lost() -> bool {
    scene(|s| std::mem::take(&mut s.lost))
}

/// Changes only the draw options, keeping the uploaded geometry.
pub fn options(through: bool, visible: bool) {
    scene(|s| {
        s.through = through;
        s.visible = visible;
    });
}

/// Drops every section. The revision keeps counting: the GPU side keeps a buffer while its
/// section's revision matches, so starting again from 0 would keep showing the old quads.
pub fn clear() {
    scene(|s| {
        s.sections.clear();
        s.marks.clear();
        s.revision += 1;
    });
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Uniform {
    clip: [[f32; 4]; 4],
    /// y: 1 = through terrain, zw: world depth size / colour size.
    params: [f32; 4],
    /// xyz: the anchor relative to the camera.
    offset: [f32; 4],
}

const SHADER: &str = r#"
struct U { clip: mat4x4<f32>, params: vec4<f32>, offset: vec4<f32> };
@group(0) @binding(0) var<uniform> u: U;
@group(0) @binding(1) var atlas: texture_2d<f32>;
@group(0) @binding(2) var atlas_sampler: sampler;
@group(0) @binding(3) var world_depth: texture_depth_2d;

struct Out {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec4<f32>,
    @location(2) @interpolate(flat) back: u32,
};

@vertex fn vs(@location(0) p: vec3<f32>, @location(1) uv: vec2<f32>, @location(2) layer: u32,
              @location(3) color: vec4<f32>, @location(4) light: u32,
              @location(5) normal: vec4<f32>) -> Out {
    var o: Out;
    o.pos = u.clip * vec4<f32>(p, 1.0);
    o.uv = uv;
    o.color = color;
    // A flat face gives every corner the same answer.
    let rel = p + u.offset.xyz;
    o.back = select(0u, 1u, normal.w > 0.5 && dot(rel, normal.xyz) > 0.0);
    return o;
}

@fragment fn fs(in: Out) -> @location(0) vec4<f32> {
    if in.back == 1u { discard; }
    if u.params.y < 0.5 {
        // Reversed depth: 1 is near, 0 the sky. Behind the world's surface: hidden.
        let dims = vec2<f32>(textureDimensions(world_depth));
        let at = clamp(in.pos.xy * u.params.zw, vec2<f32>(0.0), dims - vec2<f32>(1.0));
        let d = textureLoad(world_depth, vec2<i32>(at), 0);
        if in.pos.z < d - 0.000002 { discard; }
    }
    let t = textureSampleLevel(atlas, atlas_sampler, in.uv, 0.0);
    if t.a < 0.1 { discard; }
    return t * in.color;
}
"#;

struct Gpu {
    format: wgpu::TextureFormat,
    pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    uniform: wgpu::Buffer,
    /// The same for the highlights' pass (always through terrain).
    marks_uniform: wgpu::Buffer,
    white: wgpu::Texture,
    depth: Option<(wgpu::TextureView, [u32; 2])>,
    /// Per section: the revision uploaded, its vertex buffer and quad count.
    sections: HashMap<[i32; 3], (u64, wgpu::Buffer, u32)>,
    marks: HashMap<[i32; 3], (u64, wgpu::Buffer, u32)>,
    indices: Option<(wgpu::Buffer, u32)>,
    revision: u64,
}

static GPU: Mutex<Option<Gpu>> = Mutex::new(None);

impl Gpu {
    fn new(device: &wgpu::Device, queue: &wgpu::Queue, format: wgpu::TextureFormat) -> Self {
        let entry = |binding, ty| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
            ty,
            count: None,
        };
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Promatica ghost bindings"),
            entries: &[
                entry(
                    0,
                    wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                ),
                entry(
                    1,
                    wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                ),
                entry(2, wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering)),
                entry(
                    3,
                    wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Depth,
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                ),
            ],
        });
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Promatica ghost shader"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Promatica ghost layout"),
            bind_group_layouts: &[Some(&layout)],
            ..Default::default()
        });
        // `Vertex`: pos, uv, layer, color, light, normal (then glint_uv, unused).
        let attributes = wgpu::vertex_attr_array![
            0 => Float32x3, 1 => Float32x2, 2 => Uint32, 3 => Unorm8x4, 4 => Uint32, 5 => Snorm8x4
        ];
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Promatica ghosts"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs"),
                compilation_options: Default::default(),
                buffers: &[Some(wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<Vertex>() as u64,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &attributes,
                })],
            },
            primitive: wgpu::PrimitiveState { cull_mode: None, ..Default::default() },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth32Float,
                depth_write_enabled: Some(true),
                depth_compare: Some(wgpu::CompareFunction::GreaterEqual),
                stencil: Default::default(),
                bias: Default::default(),
            }),
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("Promatica atlas"),
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });
        let uniform_buffer = || {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("Promatica ghost uniform"),
                size: std::mem::size_of::<Uniform>() as u64,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            })
        };
        let (uniform, marks_uniform) = (uniform_buffer(), uniform_buffer());
        let white = device.create_texture_with_data(
            queue,
            &wgpu::TextureDescriptor {
                label: Some("Promatica white"),
                size: wgpu::Extent3d { width: 1, height: 1, depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8Unorm,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            },
            wgpu::util::TextureDataOrder::LayerMajor,
            &[255, 255, 255, 255],
        );
        Self {
            format,
            pipeline,
            layout,
            sampler,
            uniform,
            marks_uniform,
            white,
            depth: None,
            sections: HashMap::new(),
            marks: HashMap::new(),
            indices: None,
            revision: u64::MAX,
        }
    }

    fn depth_view(&mut self, device: &wgpu::Device, size: [u32; 2]) -> &wgpu::TextureView {
        if self.depth.as_ref().is_none_or(|(_, s)| *s != size) {
            let texture = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("Promatica ghost depth"),
                size: wgpu::Extent3d { width: size[0].max(1), height: size[1].max(1), depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Depth32Float,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                view_formats: &[],
            });
            self.depth = Some((texture.create_view(&Default::default()), size));
        }
        &self.depth.as_ref().unwrap().0
    }

    /// Brings the GPU's sections in line with the scene: gone ones dropped, changed ones
    /// uploaded again, the rest kept.
    fn sync(&mut self, device: &wgpu::Device, scene: &mut Scene) {
        self.revision = scene.revision;
        for (wanted, have) in [(&mut scene.sections, &mut self.sections), (&mut scene.marks, &mut self.marks)] {
            have.retain(|k, (rev, ..)| wanted.get(k).is_some_and(|(r, _)| r == rev));
            for (key, (rev, quads)) in wanted.iter_mut() {
                let n = (quads.len() / 4) as u32;
                if have.contains_key(key) || n == 0 {
                    continue;
                }
                let buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("Promatica ghost vertices"),
                    contents: bytemuck::cast_slice(&quads[..n as usize * 4]),
                    usage: wgpu::BufferUsages::VERTEX,
                });
                have.insert(*key, (*rev, buffer, n));
                *quads = Vec::new();
            }
        }
        let most = self.sections.values().chain(self.marks.values()).map(|(_, _, n)| *n).max().unwrap_or(0);
        if most > 0 && self.indices.as_ref().is_none_or(|(_, n)| *n < most) {
            let n = most.next_power_of_two();
            let data: Vec<u32> = (0..n).flat_map(|q| [0, 1, 2, 0, 2, 3].map(|i| q * 4 + i)).collect();
            let buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("Promatica ghost indices"),
                contents: bytemuck::cast_slice(&data),
                usage: wgpu::BufferUsages::INDEX,
            });
            self.indices = Some((buffer, n));
        }
    }
}

/// Draws the scene over the world (the `world_drawn` hook).
pub fn draw(world: &mut super::WorldDrawn<'_>) {
    let mut scene = SCENE.lock().unwrap_or_else(|e| e.into_inner());
    let Some(scene) = scene.as_mut() else { return };
    if !scene.visible || (scene.sections.is_empty() && scene.marks.is_empty()) {
        return;
    }
    let mut gpu = GPU.lock().unwrap_or_else(|e| e.into_inner());
    if gpu.as_ref().is_none_or(|g| g.format != world.color_format) {
        // Made anew: what was uploaded before is gone with the old one.
        if gpu.is_some() {
            scene.lost = true;
        }
        *gpu = Some(Gpu::new(world.device, world.queue, world.color_format));
    }
    let gpu = gpu.as_mut().unwrap();
    if gpu.revision != scene.revision {
        gpu.sync(world.device, scene);
    }
    let Some((indices, _)) = &gpu.indices else { return };
    let offset = (glam::DVec3::new(f64::from(scene.anchor[0]), f64::from(scene.anchor[1]), f64::from(scene.anchor[2]))
        - world.camera)
        .as_vec3();
    let clip = world.jittered * glam::Mat4::from_translation(offset);
    let params = [
        0.0,
        if scene.through { 1.0 } else { 0.0 },
        world.depth_size[0] as f32 / world.size[0].max(1) as f32,
        world.depth_size[1] as f32 / world.size[1].max(1) as f32,
    ];
    let uniform = |params| Uniform { clip: clip.to_cols_array_2d(), params, offset: [offset.x, offset.y, offset.z, 0.0] };
    world.queue.write_buffer(&gpu.uniform, 0, bytemuck::bytes_of(&uniform(params)));
    world.queue.write_buffer(&gpu.marks_uniform, 0, bytemuck::bytes_of(&uniform([params[0], 1.0, params[2], params[3]])));
    let atlas_view = world
        .atlas
        .map_or_else(|| gpu.white.create_view(&Default::default()), |a| a.create_view(&Default::default()));
    let group_with = |buffer: &wgpu::Buffer| {
        world.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Promatica ghost resources"),
            layout: &gpu.layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: buffer.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&atlas_view) },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::Sampler(&gpu.sampler) },
                wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::TextureView(world.depth) },
            ],
        })
    };
    let (group, marks_group) = (group_with(&gpu.uniform), group_with(&gpu.marks_uniform));
    let indices = indices.clone();
    let depth = gpu.depth_view(world.device, world.size).clone();
    // The ghosts, then the highlights over them: each pass with a depth buffer of its own.
    for (group, sections) in [(&group, &gpu.sections), (&marks_group, &gpu.marks)] {
        if sections.is_empty() {
            continue;
        }
        let mut pass = world.encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("Promatica ghosts"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: world.color_view,
                resolve_target: None,
                depth_slice: None,
                ops: wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store },
            })],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: &depth,
                depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(0.0), store: wgpu::StoreOp::Discard }),
                stencil_ops: None,
            }),
            ..Default::default()
        });
        pass.set_pipeline(&gpu.pipeline);
        pass.set_bind_group(0, group, &[]);
        pass.set_index_buffer(indices.slice(..), wgpu::IndexFormat::Uint32);
        for (key, (_, buffer, quads)) in sections {
            if !in_view(&clip, *key, scene.anchor) {
                continue;
            }
            pass.set_vertex_buffer(0, buffer.slice(..));
            pass.draw_indexed(0..quads * 6, 0, 0..1);
        }
    }
}

/// Whether section `key` (world section coordinates) may be on screen: false only when all its
/// corners lie beyond one side of the view (or all behind the camera). The frames of the area
/// and the save (keys far out of the world) are always drawn.
fn in_view(clip: &glam::Mat4, key: [i32; 3], anchor: [i32; 3]) -> bool {
    if key.iter().any(|&v| v <= i32::MIN + 16) {
        return true;
    }
    let lo = [0, 1, 2].map(|i| (i64::from(key[i]) * 16 - i64::from(anchor[i])) as f32);
    // Each side counts the corners beyond it: left, right, below, above, behind.
    let mut out = [0u8; 5];
    for c in 0..8 {
        let p = glam::Vec3::new(
            lo[0] + if c & 1 == 0 { 0.0 } else { 16.0 },
            lo[1] + if c & 2 == 0 { 0.0 } else { 16.0 },
            lo[2] + if c & 4 == 0 { 0.0 } else { 16.0 },
        );
        let q = *clip * p.extend(1.0);
        out[0] += u8::from(q.x < -q.w);
        out[1] += u8::from(q.x > q.w);
        out[2] += u8::from(q.y < -q.w);
        out[3] += u8::from(q.y > q.w);
        out[4] += u8::from(q.w <= 0.0);
    }
    out.iter().all(|&n| n < 8)
}
