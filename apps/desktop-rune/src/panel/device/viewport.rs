//! The device panel's 3D view on the GPU: web `phoneViewer.ts`'s scene, lights and materials,
//! drawn offscreen through Rune's compositor hook and placed at the panel's `El::effect` slot.

use std::sync::Arc;

use rune::compose::{Compositor, DeviceKey, Frame};
use rune::render::{CLIP_WGSL, EffectSite, Instance, KIND_EFFECT};

use super::model::{self, Model};
use crate::App;

/// The 3D view's `El::effect` tag.
pub const MARK: u32 = 0x3D01;
const SAMPLES: u32 = 4;
const COLOR: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;
const DEPTH: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;
/// position, normal, uv, colour.
const VERTEX_FLOATS: usize = 12;

/// `phoneViewer.ts`'s lights: an ambient of 2.4 and three directional lights in sRGB colours,
/// shaded as three.js's `MeshStandardMaterial` without an environment map.
pub const MESH_WGSL: &str = "
struct Frame { view_proj: mat4x4f, world: mat4x4f, eye: vec4f, screen: vec4f }
@group(0) @binding(0) var<uniform> frame: Frame;
@group(0) @binding(1) var screen_tex: texture_2d<f32>;
@group(0) @binding(2) var samp: sampler;
struct Mat { base: vec4f, params: vec4f }
@group(1) @binding(0) var<uniform> mat: Mat;
@group(1) @binding(1) var base_tex: texture_2d<f32>;

struct VIn { @location(0) pos: vec3f, @location(1) normal: vec3f, @location(2) uv: vec2f, @location(3) color: vec4f }
struct VOut {
    @builtin(position) clip: vec4f,
    @location(0) world: vec3f,
    @location(1) normal: vec3f,
    @location(2) uv: vec2f,
    @location(3) color: vec4f,
    @location(4) local: vec2f,
}

@vertex fn vs(v: VIn) -> VOut {
    let w = frame.world * vec4f(v.pos, 1.0);
    var o: VOut;
    o.clip = frame.view_proj * w;
    o.world = w.xyz;
    o.normal = (frame.world * vec4f(v.normal, 0.0)).xyz;
    o.uv = v.uv;
    o.color = v.color;
    o.local = v.pos.xy;
    return o;
}

const PI: f32 = 3.14159265;

fn ggx(n_h: f32, a: f32) -> f32 {
    let a2 = a * a;
    let d = n_h * n_h * (a2 - 1.0) + 1.0;
    return a2 / (PI * d * d);
}

fn smith(n_l: f32, n_v: f32, a: f32) -> f32 {
    let a2 = a * a;
    let gv = n_l * sqrt(n_v * n_v * (1.0 - a2) + a2);
    let gl = n_v * sqrt(n_l * n_l * (1.0 - a2) + a2);
    return 0.5 / max(gv + gl, 1e-6);
}

fn light(dir: vec3f, color: vec3f, n: vec3f, v: vec3f, diffuse: vec3f, f0: vec3f, a: f32) -> vec3f {
    let l = normalize(dir);
    let n_l = clamp(dot(n, l), 0.0, 1.0);
    let h = normalize(l + v);
    let n_h = clamp(dot(n, h), 0.0, 1.0);
    let v_h = clamp(dot(v, h), 0.0, 1.0);
    let n_v = clamp(dot(n, v), 1e-4, 1.0);
    let f = f0 + (vec3f(1.0) - f0) * pow(1.0 - v_h, 5.0);
    return color * n_l * (diffuse / PI + f * smith(n_l, n_v, a) * ggx(n_h, a));
}

@fragment fn fs(o: VOut, @builtin(front_facing) front: bool) -> @location(0) vec4f {
    if (mat.params.z > 1.5) {
        // The device's screen, as `updateDisplayUv` maps it; three.js flips canvas textures.
        let s = frame.screen;
        let u = (o.local.x + s.x * 0.5) / s.x;
        let v = (o.local.y + s.y * 0.5) / s.y;
        var uv = vec2f(u, v);
        if (s.z > 0.5) {
            if (s.w > 0.0) { uv = vec2f(1.0 - v, u); } else { uv = vec2f(v, 1.0 - u); }
        }
        let c = textureSample(screen_tex, samp, vec2f(uv.x, 1.0 - uv.y));
        return vec4f(c.rgb, 1.0);
    }
    var base = mat.base * o.color;
    if (mat.params.w > 0.5) {
        base = base * textureSample(base_tex, samp, o.uv);
    }
    if (mat.params.z > 0.5) {
        return vec4f(base.rgb * base.a, base.a);
    }
    var n = normalize(o.normal);
    if (!front) { n = -n; }
    let v = normalize(frame.eye.xyz - o.world);
    let metallic = clamp(mat.params.x, 0.0, 1.0);
    let rough = clamp(mat.params.y, 0.0525, 1.0);
    let a = rough * rough;
    let diffuse = base.rgb * (1.0 - metallic);
    let f0 = mix(vec3f(0.04), base.rgb, metallic);
    var c = diffuse * 2.4 / PI;
    c += light(vec3f(-3.0, 4.0, 5.0), vec3f(0.776, 0.846, 1.0) * 5.0, n, v, diffuse, f0, a);
    c += light(vec3f(3.0, 1.0, -3.0), vec3f(4.0), n, v, diffuse, f0, a);
    c += light(vec3f(-2.0, -2.0, -4.0), vec3f(0.347, 0.412, 0.658) * 2.0, n, v, diffuse, f0, a);
    c = min(c, vec3f(1.0));
    return vec4f(c * base.a, base.a);
}
";

/// Places the offscreen view at the slot, clipped like the primitives around it.
pub const COMPOSITE_WGSL: &str = "
struct U { rect: vec4f, clip: vec4f, fade: vec4<u32>, frame: vec4f, flags: vec4f }
@group(0) @binding(0) var<uniform> u: U;
@group(0) @binding(1) var view: texture_2d<f32>;
@group(0) @binding(2) var samp: sampler;
struct V { @builtin(position) pos: vec4f, @location(0) uv: vec2f }
@vertex fn vs(@builtin(vertex_index) vi: u32) -> V {
    let c = vec2f(f32(vi & 1u), f32(vi >> 1u));
    let l = (u.rect.xy + c * u.rect.zw - u.frame.xy) / u.frame.zw;
    return V(vec4f(l.x * 2.0 - 1.0, 1.0 - l.y * 2.0, 0.0, 1.0), c);
}
fn enc(x: vec3f) -> vec3f {
    return select(1.055 * pow(x, vec3f(1.0 / 2.4)) - 0.055, x * 12.92, x <= vec3f(0.0031308));
}
@fragment fn fs(v: V) -> @location(0) vec4f {
    let s = textureSample(view, samp, v.uv);
    let k = clip_alpha(v.pos.xy + u.frame.xy, u.clip, u.fade);
    var rgb = s.rgb;
    if (u.flags.x > 0.5 && s.a > 0.0) { rgb = enc(rgb / s.a) * s.a; }
    return vec4f(rgb, s.a) * k;
}
";

/// What the view draws this frame, from the panel's state.
pub struct Scene {
    pub model: Arc<Model>,
    pub world: [f32; 16],
    pub layout: model::Layout,
    pub pixels: rune::Pixels,
}

struct Uploaded {
    model: usize,
    meshes: Vec<(wgpu::Buffer, wgpu::Buffer, u32, usize)>,
    materials: Vec<(wgpu::BindGroup, bool)>,
}

struct Target {
    size: (u32, u32),
    msaa: wgpu::TextureView,
    depth: wgpu::TextureView,
    resolve: wgpu::TextureView,
}

struct Gpu {
    key: DeviceKey,
    opaque: wgpu::RenderPipeline,
    blended: wgpu::RenderPipeline,
    frame_layout: wgpu::BindGroupLayout,
    material_layout: wgpu::BindGroupLayout,
    composite_layout: wgpu::BindGroupLayout,
    composite_module: wgpu::ShaderModule,
    composites: Vec<(wgpu::TextureFormat, wgpu::RenderPipeline)>,
    sampler: wgpu::Sampler,
    frame: wgpu::Buffer,
    composite_uniform: wgpu::Buffer,
    white: wgpu::TextureView,
    screen: Option<(u64, (u32, u32), wgpu::Texture, wgpu::TextureView)>,
    uploaded: Option<Uploaded>,
    target: Option<Target>,
}

/// Rune's default frame, plus the 3D view wherever the device panel shows one.
#[derive(Default)]
pub struct Viewport {
    gpu: Option<Gpu>,
}

/// The slot's record this frame, if the panel shows the 3D view.
fn slot(instances: &[Instance]) -> Option<&Instance> {
    instances.iter().find(|i| i.params[1] == KIND_EFFECT && ((i.border[0] as u32 & 0xFFFF) | (i.border[1] as u32 & 0xFFFF) << 16) == MARK)
}

fn floats(data: &[f32]) -> Vec<u8> {
    data.iter().flat_map(|f| f.to_le_bytes()).collect()
}

fn texture(device: &wgpu::Device, queue: &wgpu::Queue, (width, height): (u32, u32), rgba: &[u8]) -> (wgpu::Texture, wgpu::TextureView) {
    let size = wgpu::Extent3d { width, height, depth_or_array_layers: 1 };
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("device-3d texture"),
        size,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: COLOR,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    write(queue, &texture, (width, height), rgba);
    let view = texture.create_view(&Default::default());
    (texture, view)
}

fn write(queue: &wgpu::Queue, texture: &wgpu::Texture, (width, height): (u32, u32), rgba: &[u8]) {
    queue.write_texture(
        wgpu::TexelCopyTextureInfo { texture, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
        rgba,
        wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(width * 4), rows_per_image: Some(height) },
        wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
    );
}

fn uniform_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
        ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None },
        count: None,
    }
}

fn texture_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Float { filterable: true }, view_dimension: wgpu::TextureViewDimension::D2, multisampled: false },
        count: None,
    }
}

fn sampler_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry { binding, visibility: wgpu::ShaderStages::FRAGMENT, ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering), count: None }
}

fn buffer(device: &wgpu::Device, size: u64, usage: wgpu::BufferUsages) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor { label: Some("device-3d buffer"), size, usage: usage | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false })
}

impl Gpu {
    fn new(device: &wgpu::Device, queue: &wgpu::Queue, key: DeviceKey) -> Gpu {
        let frame_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor { label: Some("device-3d frame"), entries: &[uniform_entry(0), texture_entry(1), sampler_entry(2)] });
        let material_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor { label: Some("device-3d material"), entries: &[uniform_entry(0), texture_entry(1)] });
        let composite_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor { label: Some("device-3d composite"), entries: &[uniform_entry(0), texture_entry(1), sampler_entry(2)] });
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("device-3d mesh"), source: wgpu::ShaderSource::Wgsl(MESH_WGSL.into()) });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: None, bind_group_layouts: &[Some(&frame_layout), Some(&material_layout)], immediate_size: 0 });
        let attributes = wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3, 2 => Float32x2, 3 => Float32x4];
        let pipeline = |blend: bool| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("device-3d mesh"),
                layout: Some(&layout),
                vertex: wgpu::VertexState {
                    module: &module,
                    entry_point: Some("vs"),
                    compilation_options: Default::default(),
                    buffers: &[wgpu::VertexBufferLayout { array_stride: (VERTEX_FLOATS * 4) as u64, step_mode: wgpu::VertexStepMode::Vertex, attributes: &attributes }],
                },
                primitive: wgpu::PrimitiveState { cull_mode: None, ..Default::default() },
                depth_stencil: Some(wgpu::DepthStencilState { format: DEPTH, depth_write_enabled: Some(!blend), depth_compare: Some(wgpu::CompareFunction::LessEqual), stencil: Default::default(), bias: Default::default() }),
                multisample: wgpu::MultisampleState { count: SAMPLES, ..Default::default() },
                fragment: Some(wgpu::FragmentState {
                    module: &module,
                    entry_point: Some("fs"),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState { format: COLOR, blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING), write_mask: wgpu::ColorWrites::ALL })],
                }),
                multiview_mask: None,
                cache: None,
            })
        };
        let (opaque, blended) = (pipeline(false), pipeline(true));
        let composite_module = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("device-3d composite"), source: wgpu::ShaderSource::Wgsl(format!("{CLIP_WGSL}\n{COMPOSITE_WGSL}").into()) });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("device-3d"),
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::Repeat,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let (_, white) = texture(device, queue, (1, 1), &[255; 4]);
        Gpu {
            key,
            opaque,
            blended,
            frame_layout,
            material_layout,
            composite_layout,
            composite_module,
            composites: Vec::new(),
            sampler,
            frame: buffer(device, 160, wgpu::BufferUsages::UNIFORM),
            composite_uniform: buffer(device, 80, wgpu::BufferUsages::UNIFORM),
            white,
            screen: None,
            uploaded: None,
            target: None,
        }
    }

    fn upload(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, model: &Arc<Model>) {
        let id = Arc::as_ptr(model) as usize;
        if self.uploaded.as_ref().is_some_and(|u| u.model == id) {
            return;
        }
        let textures: Vec<wgpu::TextureView> = model.images.iter().map(|i| texture(device, queue, (i.width, i.height), &i.rgba).1).collect();
        let materials = model
            .materials
            .iter()
            .map(|m| {
                let uniform = buffer(device, 32, wgpu::BufferUsages::UNIFORM);
                let kind = if m.screen { 2.0 } else if m.unlit { 1.0 } else { 0.0 };
                let view = m.texture.and_then(|t| textures.get(t));
                queue.write_buffer(&uniform, 0, &floats(&[m.base[0], m.base[1], m.base[2], m.base[3], m.metallic, m.roughness, kind, if view.is_some() { 1.0 } else { 0.0 }]));
                let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("device-3d material"),
                    layout: &self.material_layout,
                    entries: &[
                        wgpu::BindGroupEntry { binding: 0, resource: uniform.as_entire_binding() },
                        wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(view.unwrap_or(&self.white)) },
                    ],
                });
                (group, m.blend)
            })
            .collect();
        let meshes = model
            .meshes
            .iter()
            .filter(|m| !m.indices.is_empty())
            .map(|m| {
                let mut data = Vec::with_capacity(m.positions.len() * VERTEX_FLOATS);
                for i in 0..m.positions.len() {
                    data.extend_from_slice(&m.positions[i]);
                    data.extend_from_slice(m.normals.get(i).unwrap_or(&[0., 0., 1.]));
                    data.extend_from_slice(m.uvs.get(i).unwrap_or(&[0., 0.]));
                    data.extend_from_slice(m.colors.get(i).unwrap_or(&[1.; 4]));
                }
                let vertices = buffer(device, (data.len() * 4) as u64, wgpu::BufferUsages::VERTEX);
                queue.write_buffer(&vertices, 0, &floats(&data));
                let indices = buffer(device, (m.indices.len() * 4).next_multiple_of(4) as u64, wgpu::BufferUsages::INDEX);
                queue.write_buffer(&indices, 0, &m.indices.iter().flat_map(|i| i.to_le_bytes()).collect::<Vec<u8>>());
                (vertices, indices, m.indices.len() as u32, m.material)
            })
            .collect();
        self.uploaded = Some(Uploaded { model: id, meshes, materials });
    }

    fn screen(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, pixels: &rune::Pixels) {
        let gpu_screen = &mut self.screen;
        pixels.read(|width, height, generation, rgba| {
            if width == 0 || height == 0 || rgba.len() != (width * height * 4) as usize {
                return;
            }
            match gpu_screen {
                Some((seen, _, _, _)) if *seen == generation => {}
                Some((seen, size, texture, _)) if *size == (width, height) => {
                    write(queue, texture, (width, height), rgba);
                    *seen = generation;
                }
                _ => {
                    let (texture, view) = texture(device, queue, (width, height), rgba);
                    *gpu_screen = Some((generation, (width, height), texture, view));
                }
            }
        });
    }

    fn target(&mut self, device: &wgpu::Device, size: (u32, u32)) -> &Target {
        if self.target.as_ref().is_none_or(|t| t.size != size) {
            let make = |format: wgpu::TextureFormat, samples: u32, usage: wgpu::TextureUsages| {
                device
                    .create_texture(&wgpu::TextureDescriptor {
                        label: Some("device-3d target"),
                        size: wgpu::Extent3d { width: size.0, height: size.1, depth_or_array_layers: 1 },
                        mip_level_count: 1,
                        sample_count: samples,
                        dimension: wgpu::TextureDimension::D2,
                        format,
                        usage,
                        view_formats: &[],
                    })
                    .create_view(&Default::default())
            };
            self.target = Some(Target {
                size,
                msaa: make(COLOR, SAMPLES, wgpu::TextureUsages::RENDER_ATTACHMENT),
                depth: make(DEPTH, SAMPLES, wgpu::TextureUsages::RENDER_ATTACHMENT),
                resolve: make(COLOR, 1, wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING),
            });
        }
        self.target.as_ref().expect("just made")
    }

    /// Draws `scene` into the offscreen target of `size` device pixels.
    fn draw(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, encoder: &mut wgpu::CommandEncoder, scene: &Scene, size: (u32, u32)) -> bool {
        self.upload(device, queue, &scene.model);
        self.screen(device, queue, &scene.pixels);
        let Some((_, _, _, screen)) = &self.screen else { return false };
        let camera = model::camera(&scene.model, &scene.world, size.0 as f32 / size.1 as f32);
        let mut data = Vec::with_capacity(40);
        data.extend_from_slice(&camera.view_proj);
        data.extend_from_slice(&scene.world);
        data.extend_from_slice(&[camera.eye[0], camera.eye[1], camera.eye[2], 1.]);
        let [w, h, _] = scene.model.screen;
        data.extend_from_slice(&[w, h, if scene.layout.raw_landscape { 1. } else { 0. }, scene.layout.rotation]);
        queue.write_buffer(&self.frame, 0, &floats(&data));
        let frame = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("device-3d frame"),
            layout: &self.frame_layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: self.frame.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(screen) },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::Sampler(&self.sampler) },
            ],
        });
        self.target(device, size);
        let (Some(target), Some(uploaded)) = (&self.target, &self.uploaded) else { return false };
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("device-3d"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &target.msaa,
                depth_slice: None,
                resolve_target: Some(&target.resolve),
                ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT), store: wgpu::StoreOp::Discard },
            })],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: &target.depth,
                depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(1.0), store: wgpu::StoreOp::Discard }),
                stencil_ops: None,
            }),
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_bind_group(0, &frame, &[]);
        for blended in [false, true] {
            pass.set_pipeline(if blended { &self.blended } else { &self.opaque });
            for (vertices, indices, count, material) in &uploaded.meshes {
                let Some((group, blend)) = uploaded.materials.get(*material) else { continue };
                if *blend != blended {
                    continue;
                }
                pass.set_bind_group(1, group, &[]);
                pass.set_vertex_buffer(0, vertices.slice(..));
                pass.set_index_buffer(indices.slice(..), wgpu::IndexFormat::Uint32);
                pass.draw_indexed(0..*count, 0, 0..1);
            }
        }
        true
    }

    /// Places the drawn view at the slot.
    fn composite(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, pass: &mut wgpu::RenderPass<'_>, site: &EffectSite<'_>) {
        let Some(target) = &self.target else { return };
        if !self.composites.iter().any(|(f, _)| *f == site.format) {
            let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: None, bind_group_layouts: &[Some(&self.composite_layout)], immediate_size: 0 });
            let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("device-3d composite"),
                layout: Some(&layout),
                vertex: wgpu::VertexState { module: &self.composite_module, entry_point: Some("vs"), compilation_options: Default::default(), buffers: &[] },
                primitive: wgpu::PrimitiveState { topology: wgpu::PrimitiveTopology::TriangleStrip, ..Default::default() },
                depth_stencil: None,
                multisample: Default::default(),
                fragment: Some(wgpu::FragmentState {
                    module: &self.composite_module,
                    entry_point: Some("fs"),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState { format: site.format, blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING), write_mask: wgpu::ColorWrites::ALL })],
                }),
                multiview_mask: None,
                cache: None,
            });
            self.composites.push((site.format, pipeline));
        }
        let i = site.instance;
        let mut data = [0f32; 20];
        data[0..4].copy_from_slice(&i.rect);
        data[4..8].copy_from_slice(&i.clip);
        for k in 0..4 {
            data[8 + k] = f32::from_bits(i.fade[k]);
        }
        data[12..14].copy_from_slice(&site.origin);
        data[14] = site.size[0] as f32;
        data[15] = site.size[1] as f32;
        data[16] = if site.srgb_blend { 1.0 } else { 0.0 };
        queue.write_buffer(&self.composite_uniform, 0, &floats(&data));
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("device-3d composite"),
            layout: &self.composite_layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: self.composite_uniform.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&target.resolve) },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::Sampler(&self.sampler) },
            ],
        });
        let Some((_, pipeline)) = self.composites.iter().find(|(f, _)| *f == site.format) else { return };
        pass.set_pipeline(pipeline);
        pass.set_bind_group(0, &group, &[]);
        pass.draw(0..4, 0..1);
    }
}

impl Compositor<App> for Viewport {
    fn render(&mut self, f: &mut Frame<'_, App>) {
        let Some(rect) = slot(&f.rt.instances).map(|i| i.rect) else { return f.draw_ui() };
        let Some(scene) = super::scene_3d(&f.rt.state) else { return f.draw_ui() };
        let size = (rect[2].round().max(1.) as u32, rect[3].round().max(1.) as u32);
        let (device, queue) = (f.device, f.queue);
        let key = f.device_key();
        if self.gpu.as_ref().is_none_or(|g| g.key != key) {
            self.gpu = Some(Gpu::new(device, queue, key));
        }
        let gpu = self.gpu.as_mut().expect("just made");
        let drew = gpu.draw(device, queue, f.encoder, &scene, size);
        let clear = f.rt.clear_color();
        f.ui.prepare(device, queue, &mut f.rt.text.atlas, &f.rt.instances, f.size);
        f.ui.draw_with(f.encoder, f.target, clear, &mut |pass, site| {
            if drew && site.mark == MARK {
                gpu.composite(device, queue, pass, site);
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_3d_shaders_validate() {
        for source in [MESH_WGSL.to_owned(), format!("{CLIP_WGSL}\n{COMPOSITE_WGSL}")] {
            let module = naga::front::wgsl::parse_str(&source).unwrap_or_else(|e| panic!("{}", e.emit_to_string(&source)));
            naga::valid::Validator::new(naga::valid::ValidationFlags::all(), naga::valid::Capabilities::all())
                .validate(&module)
                .unwrap_or_else(|e| panic!("{e:?}"));
        }
    }
}
