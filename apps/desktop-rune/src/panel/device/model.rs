//! The device panel's 3D phone as triangle meshes: web's GLB device models (`modelScene.ts`) and
//! its procedural phone (`phoneScene.ts`, `shapeProfile.ts`), with the camera framing, pose and
//! screen projection of `phoneViewer.ts` and `deviceFraming.ts`.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

use serde_json::Value;

/// Every display is this tall in model units, centred on the origin.
pub const SCREEN_HEIGHT: f32 = 2.2;
/// `PerspectiveCamera(32, …)`: the vertical field of view.
pub const FOV_Y: f32 = 32.0 * std::f32::consts::PI / 180.0;

#[derive(Clone, Debug, PartialEq)]
pub struct Material {
    /// Linear RGBA.
    pub base: [f32; 4],
    pub metallic: f32,
    pub roughness: f32,
    pub texture: Option<usize>,
    pub blend: bool,
    /// Drawn as its colour, without lighting.
    pub unlit: bool,
    /// Shows the device's screen.
    pub screen: bool,
    /// A screen shown at its mesh's own UVs (u across, v up) rather than across `Model::screen`.
    pub mapped: bool,
}

impl Material {
    fn lit(hex: u32, metallic: f32, roughness: f32) -> Material {
        Material { base: linear(hex), metallic, roughness, texture: None, blend: false, unlit: false, screen: false, mapped: false }
    }
}

#[derive(Default)]
pub struct Mesh {
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    pub uvs: Vec<[f32; 2]>,
    pub colors: Vec<[f32; 4]>,
    pub indices: Vec<u32>,
    pub material: usize,
    /// The display, which shows the device's screen.
    pub screen: bool,
}

pub struct Image {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

pub struct Model {
    pub meshes: Vec<Mesh>,
    pub materials: Vec<Material>,
    pub images: Vec<Image>,
    /// The display's width and height, and the z of its face.
    pub screen: [f32; 3],
    /// Displays that aren't one flat face at `screen` (a foldable's), for touches.
    pub faces: Vec<Face>,
    pub min: [f32; 3],
    pub max: [f32; 3],
}

impl Model {
    fn bounds(&mut self) {
        let (mut min, mut max) = ([f32::INFINITY; 3], [f32::NEG_INFINITY; 3]);
        for p in self.meshes.iter().flat_map(|m| &m.positions) {
            for k in 0..3 {
                min[k] = min[k].min(p[k]);
                max[k] = max[k].max(p[k]);
            }
        }
        (self.min, self.max) = (min, max);
    }
}

/// A flat display: `center ± right ± up` in the model's space, showing `across` of the screen's
/// width from its left edge to its right.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Face {
    pub center: [f32; 3],
    pub right: [f32; 3],
    pub up: [f32; 3],
    pub across: [f32; 2],
}

/// A three.js colour: sRGB hex, linear components.
fn linear(hex: u32) -> [f32; 4] {
    let c = |shift: u32| {
        let v = ((hex >> shift) & 0xff) as f32 / 255.0;
        if v <= 0.04045 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
    };
    [c(16), c(8), c(0), 1.0]
}

// Web's device models.

/// `resolveDeviceModelId`: the devices web draws from a model rather than the procedural phone.
pub fn model_id(android: bool, name: &str) -> Option<&'static str> {
    if android {
        return None;
    }
    [("iPhone Duo", "iphone-duo"), ("iPhone 18 Pro Max", "iphone-18-pro-max"), ("iPhone 18 Pro", "iphone-18-pro"), ("iPad Pro 13-inch (M5)", "ipad-pro-13-m5")]
        .into_iter()
        .find(|(n, _)| n.eq_ignore_ascii_case(name))
        .map(|(_, id)| id)
}

/// Web ships these in its bundle and fetches one when the 3D view first shows it; here they ship
/// in the binary, mapped but untouched until then.
fn glb(id: &str) -> Option<&'static [u8]> {
    Some(match id {
        "iphone-18-pro" => include_bytes!("../../../../web/src/components/device/models/iphone-18-pro.glb"),
        "iphone-18-pro-max" => include_bytes!("../../../../web/src/components/device/models/iphone-18-pro-max.glb"),
        "ipad-pro-13-m5" => include_bytes!("../../../../web/src/components/device/models/ipad-pro-13-m5.glb"),
        "ipad-pro-13-m5-magic-keyboard" => include_bytes!("../../../../web/src/components/device/models/ipad-pro-13-m5-magic-keyboard.glb"),
        _ => return None,
    })
}

/// Web's `deviceKeyboard`: the iPad Pro 13-inch with its Magic Keyboard attached, which web adds
/// to the iPad's orientation group, so it turns with the iPad.
pub const KEYBOARD: &str = "ipad-pro-13-m5+magic-keyboard";

fn load(id: &str) -> Option<Result<Model, String>> {
    if id == KEYBOARD {
        let (ipad, keyboard) = (glb("ipad-pro-13-m5")?, glb("ipad-pro-13-m5-magic-keyboard")?);
        return Some(parse_glb(ipad).and_then(|ipad| Ok(attach(ipad, parse(keyboard, false).map_err(|e| format!("Magic Keyboard: {e}"))?))));
    }
    glb(id).map(parse_glb)
}

/// `model` with `accessory`'s meshes added in its own space.
fn attach(mut model: Model, accessory: Model) -> Model {
    let (materials, images) = (model.materials.len(), model.images.len());
    model.images.extend(accessory.images);
    model.materials.extend(accessory.materials.into_iter().map(|m| Material { texture: m.texture.map(|t| t + images), ..m }));
    model.meshes.extend(accessory.meshes.into_iter().map(|m| Mesh { material: m.material + materials, screen: false, ..m }));
    model.bounds();
    model
}

enum Slot {
    Loading,
    Ready(Arc<Model>),
    Failed,
}

fn models() -> &'static Mutex<HashMap<&'static str, Slot>> {
    static MODELS: OnceLock<Mutex<HashMap<&'static str, Slot>>> = OnceLock::new();
    MODELS.get_or_init(Mutex::default)
}

/// The parsed model for `id`, once loaded: the first ask parses it on a thread and wakes when
/// it's ready (or at once with `now`, for scenes). A model that fails keeps the procedural phone,
/// as web's `onModelError` does.
pub fn model(id: &'static str, now: bool, wake: impl Fn() + Send + 'static) -> Option<Arc<Model>> {
    if id != KEYBOARD {
        glb(id)?;
    }
    let mut slots = models().lock().unwrap_or_else(|e| e.into_inner());
    match slots.get(id) {
        Some(Slot::Ready(model)) => return Some(model.clone()),
        Some(Slot::Loading | Slot::Failed) => return None,
        None => {}
    }
    if now {
        let slot = load(id)?.map(Arc::new).map_or(Slot::Failed, Slot::Ready);
        let ready = if let Slot::Ready(m) = &slot { Some(m.clone()) } else { None };
        slots.insert(id, slot);
        return ready;
    }
    slots.insert(id, Slot::Loading);
    drop(slots);
    std::thread::spawn(move || {
        let slot = load(id).and_then(Result::ok).map(Arc::new).map_or(Slot::Failed, Slot::Ready);
        models().lock().unwrap_or_else(|e| e.into_inner()).insert(id, slot);
        wake();
    });
    None
}

type Mat4 = [f32; 16];

const IDENTITY: Mat4 = [1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1.];

/// Column-major `a × b`.
pub fn mul(a: &Mat4, b: &Mat4) -> Mat4 {
    let mut m = [0.0; 16];
    for c in 0..4 {
        for r in 0..4 {
            m[c * 4 + r] = (0..4).map(|k| a[k * 4 + r] * b[c * 4 + k]).sum();
        }
    }
    m
}

fn trs(t: [f32; 3], q: [f32; 4], s: [f32; 3]) -> Mat4 {
    let [x, y, z, w] = q;
    let r = [
        1. - 2. * (y * y + z * z),
        2. * (x * y + z * w),
        2. * (x * z - y * w),
        2. * (x * y - z * w),
        1. - 2. * (x * x + z * z),
        2. * (y * z + x * w),
        2. * (x * z + y * w),
        2. * (y * z - x * w),
        1. - 2. * (x * x + y * y),
    ];
    [r[0] * s[0], r[1] * s[0], r[2] * s[0], 0., r[3] * s[1], r[4] * s[1], r[5] * s[1], 0., r[6] * s[2], r[7] * s[2], r[8] * s[2], 0., t[0], t[1], t[2], 1.]
}

pub fn point(m: &Mat4, p: [f32; 3]) -> [f32; 3] {
    [0, 1, 2].map(|r| m[r] * p[0] + m[4 + r] * p[1] + m[8 + r] * p[2] + m[12 + r])
}

fn normalize(v: [f32; 3]) -> [f32; 3] {
    let l = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    if l > 0. { v.map(|c| c / l) } else { [0., 0., 1.] }
}

/// The inverse transpose of `m`'s upper 3×3, which carries normals.
fn normal_matrix(m: &Mat4) -> [f32; 9] {
    let a = [m[0], m[1], m[2], m[4], m[5], m[6], m[8], m[9], m[10]];
    // Cofactors of a column-major 3×3; the inverse transpose is the cofactor matrix over det.
    let c = [
        a[4] * a[8] - a[5] * a[7],
        a[5] * a[6] - a[3] * a[8],
        a[3] * a[7] - a[4] * a[6],
        a[2] * a[7] - a[1] * a[8],
        a[0] * a[8] - a[2] * a[6],
        a[1] * a[6] - a[0] * a[7],
        a[1] * a[5] - a[2] * a[4],
        a[2] * a[3] - a[0] * a[5],
        a[0] * a[4] - a[1] * a[3],
    ];
    let det = a[0] * c[0] + a[1] * c[1] + a[2] * c[2];
    let det = if det.abs() > 1e-12 { det } else { 1. };
    c.map(|v| v / det)
}

fn f32s(v: &Value) -> Vec<f32> {
    v.as_array().into_iter().flatten().filter_map(Value::as_f64).map(|f| f as f32).collect()
}

struct Glb<'a> {
    json: Value,
    bin: &'a [u8],
}

impl Glb<'_> {
    fn view(&self, index: usize) -> Result<(&[u8], usize), String> {
        let view = &self.json["bufferViews"][index];
        let offset = view["byteOffset"].as_u64().unwrap_or(0) as usize;
        let length = view["byteLength"].as_u64().ok_or("A buffer view has no length")? as usize;
        let bytes = self.bin.get(offset..offset + length).ok_or("A buffer view is out of bounds")?;
        Ok((bytes, view["byteStride"].as_u64().unwrap_or(0) as usize))
    }

    /// An accessor's elements as floats (normalized integers scaled to 0…1 or -1…1) and its width.
    fn floats(&self, index: usize) -> Result<(Vec<f32>, usize), String> {
        let a = &self.json["accessors"][index];
        if a.get("sparse").is_some() {
            return Err("Sparse accessors are not supported".into());
        }
        let width = match a["type"].as_str() {
            Some("SCALAR") => 1,
            Some("VEC2") => 2,
            Some("VEC3") => 3,
            Some("VEC4") => 4,
            _ => return Err("An accessor has an unsupported type".into()),
        };
        let count = a["count"].as_u64().ok_or("An accessor has no count")? as usize;
        let kind = a["componentType"].as_u64().unwrap_or(0);
        let size = match kind {
            5120 | 5121 => 1,
            5122 | 5123 => 2,
            5125 | 5126 => 4,
            _ => return Err("An accessor has an unsupported component type".into()),
        };
        let normalized = a["normalized"] == true;
        let Some(view) = a["bufferView"].as_u64() else { return Ok((vec![0.0; count * width], width)) };
        let (bytes, stride) = self.view(view as usize)?;
        let stride = if stride == 0 { size * width } else { stride };
        let offset = a["byteOffset"].as_u64().unwrap_or(0) as usize;
        let mut out = Vec::with_capacity(count * width);
        for i in 0..count {
            for k in 0..width {
                let at = offset + i * stride + k * size;
                let b = bytes.get(at..at + size).ok_or("An accessor is out of bounds")?;
                out.push(match (kind, normalized) {
                    (5126, _) => f32::from_le_bytes([b[0], b[1], b[2], b[3]]),
                    (5121, true) => b[0] as f32 / 255.0,
                    (5121, false) => b[0] as f32,
                    (5120, true) => (b[0] as i8 as f32 / 127.0).max(-1.0),
                    (5120, false) => b[0] as i8 as f32,
                    (5123, true) => u16::from_le_bytes([b[0], b[1]]) as f32 / 65535.0,
                    (5123, false) => u16::from_le_bytes([b[0], b[1]]) as f32,
                    (5122, true) => (i16::from_le_bytes([b[0], b[1]]) as f32 / 32767.0).max(-1.0),
                    (5122, false) => i16::from_le_bytes([b[0], b[1]]) as f32,
                    _ => u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as f32,
                });
            }
        }
        Ok((out, width))
    }

    fn indices(&self, index: usize) -> Result<Vec<u32>, String> {
        let a = &self.json["accessors"][index];
        let count = a["count"].as_u64().ok_or("An accessor has no count")? as usize;
        let kind = a["componentType"].as_u64().unwrap_or(0);
        let size = match kind {
            5121 => 1,
            5123 => 2,
            5125 => 4,
            _ => return Err("Indices have an unsupported component type".into()),
        };
        let (bytes, stride) = self.view(a["bufferView"].as_u64().ok_or("Indices have no buffer view")? as usize)?;
        let stride = if stride == 0 { size } else { stride };
        let offset = a["byteOffset"].as_u64().unwrap_or(0) as usize;
        (0..count)
            .map(|i| {
                let at = offset + i * stride;
                let b = bytes.get(at..at + size).ok_or("Indices are out of bounds")?;
                Ok(match size {
                    1 => b[0] as u32,
                    2 => u16::from_le_bytes([b[0], b[1]]) as u32,
                    _ => u32::from_le_bytes([b[0], b[1], b[2], b[3]]),
                })
            })
            .collect()
    }
}

/// A binary glTF as world-space meshes, its base colour textures decoded. Web's loader keeps
/// clearcoat and the other PBR maps; this keeps base colour, metalness and roughness.
pub fn parse_glb(bytes: &[u8]) -> Result<Model, String> {
    parse(bytes, true)
}

/// A glTF that has a normalized display (a device) or none (an accessory).
fn parse(bytes: &[u8], display: bool) -> Result<Model, String> {
    let word = |at: usize| bytes.get(at..at + 4).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as usize);
    if bytes.get(0..4) != Some(b"glTF") || word(4) != Some(2) {
        return Err("Not a binary glTF 2.0 file".into());
    }
    let json_len = word(12).ok_or("Truncated glTF")?;
    let json: Value = serde_json::from_slice(bytes.get(20..20 + json_len).ok_or("Truncated glTF")?).map_err(|e| e.to_string())?;
    let bin_at = 20 + json_len;
    let bin_len = word(bin_at).unwrap_or(0);
    let bin = bytes.get(bin_at + 8..bin_at + 8 + bin_len).unwrap_or(&[]);
    let glb = Glb { json, bin };
    let json = &glb.json;

    let mut images = Vec::new();
    for image in json["images"].as_array().into_iter().flatten() {
        let view = image["bufferView"].as_u64().ok_or("An image is not embedded")?;
        let (data, _) = glb.view(view as usize)?;
        let decoded = image::load_from_memory(data).map_err(|e| e.to_string())?.to_rgba8();
        images.push(Image { width: decoded.width(), height: decoded.height(), rgba: decoded.into_raw() });
    }
    let texture_image = |t: &Value| -> Option<usize> {
        let texture = &json["textures"][t["index"].as_u64()? as usize];
        texture["extensions"]["EXT_texture_webp"]["source"].as_u64().or(texture["source"].as_u64()).map(|i| i as usize).filter(|i| *i < images.len())
    };
    let mut materials: Vec<Material> = json["materials"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|m| {
            let pbr = &m["pbrMetallicRoughness"];
            let base = f32s(&pbr["baseColorFactor"]);
            Material {
                base: if base.len() == 4 { [base[0], base[1], base[2], base[3]] } else { [1.0; 4] },
                metallic: pbr["metallicFactor"].as_f64().unwrap_or(1.0) as f32,
                roughness: pbr["roughnessFactor"].as_f64().unwrap_or(1.0) as f32,
                texture: texture_image(&pbr["baseColorTexture"]),
                blend: m["alphaMode"] == "BLEND",
                unlit: false,
                screen: false,
                mapped: false,
            }
        })
        .collect();
    let fallback = materials.len();
    materials.push(Material::lit(0xffffff, 1.0, 1.0));

    let mut meshes = Vec::new();
    let scene = &json["scenes"][json["scene"].as_u64().unwrap_or(0) as usize];
    let mut stack: Vec<(usize, Mat4)> = scene["nodes"].as_array().into_iter().flatten().filter_map(Value::as_u64).map(|n| (n as usize, IDENTITY)).collect();
    let mut visited = 0;
    while let Some((index, parent)) = stack.pop() {
        visited += 1;
        if visited > 10_000 {
            return Err("The node graph has a cycle".into());
        }
        let node = &json["nodes"][index];
        let local: Mat4 = match f32s(&node["matrix"]).as_slice() {
            m if m.len() == 16 => std::array::from_fn(|i| m[i]),
            _ => {
                let (t, r, s) = (f32s(&node["translation"]), f32s(&node["rotation"]), f32s(&node["scale"]));
                trs(
                    if t.len() == 3 { [t[0], t[1], t[2]] } else { [0.0; 3] },
                    if r.len() == 4 { [r[0], r[1], r[2], r[3]] } else { [0., 0., 0., 1.] },
                    if s.len() == 3 { [s[0], s[1], s[2]] } else { [1.0; 3] },
                )
            }
        };
        let world = mul(&parent, &local);
        for child in node["children"].as_array().into_iter().flatten().filter_map(Value::as_u64) {
            stack.push((child as usize, world));
        }
        let Some(mesh) = node["mesh"].as_u64() else { continue };
        let screen = node["name"] == "device-screen";
        let normals = normal_matrix(&world);
        for primitive in json["meshes"][mesh as usize]["primitives"].as_array().into_iter().flatten() {
            if primitive["mode"].as_u64().unwrap_or(4) != 4 {
                continue;
            }
            let attributes = &primitive["attributes"];
            let Some(position) = attributes["POSITION"].as_u64() else { continue };
            let material = primitive["material"].as_u64().map_or(fallback, |m| (m as usize).min(fallback));
            let (positions, _) = glb.floats(position as usize)?;
            let count = positions.len() / 3;
            let mut out = Mesh { material, screen, ..Default::default() };
            out.positions = positions.chunks_exact(3).map(|p| point(&world, [p[0], p[1], p[2]])).collect();
            if let Some(normal) = attributes["NORMAL"].as_u64() {
                let (n, _) = glb.floats(normal as usize)?;
                out.normals = n.chunks_exact(3).map(|n| normalize([0, 1, 2].map(|r| normals[r] * n[0] + normals[3 + r] * n[1] + normals[6 + r] * n[2]))).collect();
            }
            let set = json["materials"][material]["pbrMetallicRoughness"]["baseColorTexture"]["texCoord"].as_u64().unwrap_or(0);
            if let Some(uv) = attributes[format!("TEXCOORD_{set}")].as_u64() {
                out.uvs = glb.floats(uv as usize)?.0.chunks_exact(2).map(|t| [t[0], t[1]]).collect();
            }
            if let Some(color) = attributes["COLOR_0"].as_u64() {
                let (c, width) = glb.floats(color as usize)?;
                out.colors = c.chunks_exact(width).map(|c| [c[0], c[1], c[2], if width == 4 { c[3] } else { 1.0 }]).collect();
            }
            out.indices = match primitive["indices"].as_u64() {
                Some(indices) => glb.indices(indices as usize)?,
                None => (0..count as u32).collect(),
            };
            if out.indices.iter().any(|&i| i as usize >= count) {
                return Err("A mesh indexes past its vertices".into());
            }
            if out.normals.len() != count {
                out.normals = face_normals(&out.positions, &out.indices);
            }
            meshes.push(out);
        }
    }
    let (mut min, mut max) = ([f32::INFINITY; 3], [f32::NEG_INFINITY; 3]);
    for p in meshes.iter().filter(|m| m.screen).flat_map(|m| &m.positions) {
        for k in 0..3 {
            min[k] = min[k].min(p[k]);
            max[k] = max[k].max(p[k]);
        }
    }
    let (width, height) = (max[0] - min[0], max[1] - min[1]);
    if !display {
        let mut model = Model { meshes, materials, images, screen: [0.; 3], faces: Vec::new(), min: [0.; 3], max: [0.; 3] };
        model.bounds();
        return Some(model).filter(|m| m.min[0].is_finite()).ok_or_else(|| "Device accessory has invalid bounds".to_owned());
    }
    // `createImportedPhoneScene`'s check: one flat display, 2.2 tall, centred on the origin.
    if !width.is_finite() || width <= 0. || (height - SCREEN_HEIGHT).abs() > 0.001 || (min[0] + max[0]).abs() > 0.001 || (min[1] + max[1]).abs() > 0.001 || max[2] - min[2] > 0.001 {
        return Err("Device model display is not normalized".into());
    }
    let screen = materials.len();
    materials.push(Material { unlit: true, screen: true, ..Material::lit(0xffffff, 0., 1.) });
    for mesh in meshes.iter_mut().filter(|m| m.screen) {
        mesh.material = screen;
    }
    let mut model = Model { meshes, materials, images, screen: [width, height, max[2]], faces: Vec::new(), min: [0.; 3], max: [0.; 3] };
    model.bounds();
    Ok(model)
}

fn face_normals(positions: &[[f32; 3]], indices: &[u32]) -> Vec<[f32; 3]> {
    let mut normals = vec![[0.0f32; 3]; positions.len()];
    for t in indices.chunks_exact(3) {
        let [a, b, c] = [t[0], t[1], t[2]].map(|i| positions[i as usize]);
        let (u, v) = ([b[0] - a[0], b[1] - a[1], b[2] - a[2]], [c[0] - a[0], c[1] - a[1], c[2] - a[2]]);
        let n = [u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0]];
        for i in t {
            for k in 0..3 {
                normals[*i as usize][k] += n[k];
            }
        }
    }
    normals.into_iter().map(normalize).collect()
}

// Web's procedural phone.

/// `DeviceShapeProfile`: what the procedural phone looks like.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Profile {
    pub id: &'static str,
    bezel: f32,
    body_radius: f32,
    screen_radius: f32,
    depth: f32,
    back: u32,
    buttons: &'static [(char, f32, f32)],
    camera: [f32; 5],
    lenses: &'static [[f32; 2]],
    flash: Option<[f32; 2]>,
}

const IOS_PHONE: Profile = Profile {
    id: "ios-phone",
    bezel: 0.055,
    body_radius: 0.15,
    screen_radius: 0.105,
    depth: 0.085,
    back: 0x424b5d,
    buttons: &[('r', 0.35, 0.3), ('l', 0.48, 0.18), ('l', 0.22, 0.18)],
    camera: [0.39, 0.44, 0.25, 0.29, 0.068],
    lenses: &[[-0.08, 0.095], [0.08, -0.095]],
    flash: Some([0.085, 0.11]),
};

const IOS_TABLET: Profile = Profile {
    id: "ios-tablet",
    bezel: 0.065,
    body_radius: 0.105,
    screen_radius: 0.045,
    depth: 0.055,
    back: 0x9ca5af,
    buttons: &[('t', 0.5, 0.15), ('r', 0.78, 0.13), ('r', 0.58, 0.13)],
    camera: [0.19, 0.19, 0.15, 0.15, 0.045],
    lenses: &[[0., 0.]],
    flash: None,
};

const ANDROID_PHONE: Profile = Profile {
    id: "android-phone",
    bezel: 0.035,
    body_radius: 0.115,
    screen_radius: 0.08,
    depth: 0.085,
    back: 0x344449,
    buttons: &[('r', 0.2, 0.24), ('r', 0.65, 0.32)],
    camera: [0.24, 0.47, 0.18, 0.29, 0.056],
    lenses: &[[0., 0.11], [0., -0.11]],
    flash: Some([0.1, 0.]),
};

const ANDROID_TABLET: Profile = Profile { id: "android-tablet", back: 0x697b80, ..IOS_TABLET };

/// `resolveDeviceShape`.
pub fn profile(android: bool, name: &str, portrait_aspect: f32) -> Profile {
    let word = |w: &str| name.split(|c: char| !c.is_ascii_alphanumeric()).any(|p| p.eq_ignore_ascii_case(w));
    let tablet = word("ipad") || word("tablet") || (!word("iphone") && !word("phone") && portrait_aspect >= 0.6);
    match (android, tablet) {
        (false, true) => IOS_TABLET,
        (false, false) => IOS_PHONE,
        (true, true) => ANDROID_TABLET,
        (true, false) => ANDROID_PHONE,
    }
}

/// `roundedPath`'s outline, counter-clockwise from the bottom edge, its corners as its quadratics.
fn rounded(width: f32, height: f32, radius: f32) -> Vec<[f32; 2]> {
    let (x, y, r) = (-width / 2., -height / 2., radius.min(width / 2.).min(height / 2.));
    let corners = [
        ([x + width - r, y], [x + width, y], [x + width, y + r]),
        ([x + width, y + height - r], [x + width, y + height], [x + width - r, y + height]),
        ([x + r, y + height], [x, y + height], [x, y + height - r]),
        ([x, y + r], [x, y], [x + r, y]),
    ];
    let mut out = Vec::new();
    for (a, c, b) in corners {
        for i in 0..=8 {
            let t = i as f32 / 8.;
            let u = 1. - t;
            out.push([u * u * a[0] + 2. * u * t * c[0] + t * t * b[0], u * u * a[1] + 2. * u * t * c[1] + t * t * b[1]]);
        }
    }
    out
}

struct Builder {
    meshes: Vec<Mesh>,
}

impl Builder {
    fn mesh(&mut self, material: usize) -> &mut Mesh {
        self.meshes.push(Mesh { material, ..Default::default() });
        self.meshes.last_mut().expect("just pushed")
    }

    /// A flat convex outline at `z`, facing +z or -z, offset by `at`.
    fn cap(&mut self, material: usize, outline: &[[f32; 2]], at: [f32; 2], z: f32, up: bool, screen: bool) {
        let m = self.mesh(material);
        m.screen = screen;
        m.positions.push([at[0], at[1], z]);
        for p in outline {
            m.positions.push([at[0] + p[0], at[1] + p[1], z]);
        }
        m.normals = vec![[0., 0., if up { 1. } else { -1. }]; m.positions.len()];
        let n = outline.len() as u32;
        for i in 0..n {
            let (a, b) = (1 + i, 1 + (i + 1) % n);
            m.indices.extend(if up { [0, a, b] } else { [0, b, a] });
        }
    }

    /// The side of `outline` extruded from `z0` up to `z1`, with outward normals.
    fn wall(&mut self, material: usize, outline: &[[f32; 2]], at: [f32; 2], z0: f32, z1: f32) {
        let m = self.mesh(material);
        let n = outline.len();
        for i in 0..n {
            let (a, b) = (outline[i], outline[(i + 1) % n]);
            let normal = normalize([b[1] - a[1], a[0] - b[0], 0.]);
            let base = m.positions.len() as u32;
            for (p, z) in [(a, z0), (b, z0), (b, z1), (a, z1)] {
                m.positions.push([at[0] + p[0], at[1] + p[1], z]);
                m.normals.push(normal);
            }
            m.indices.extend([base, base + 1, base + 2, base, base + 2, base + 3]);
        }
    }

    fn slab(&mut self, material: usize, outline: &[[f32; 2]], at: [f32; 2], z0: f32, z1: f32) {
        self.wall(material, outline, at, z0, z1);
        self.cap(material, outline, at, z1, true, false);
        self.cap(material, outline, at, z0, false, false);
    }

    fn circle(radius: f32) -> Vec<[f32; 2]> {
        (0..32).map(|i| i as f32 / 32. * std::f32::consts::TAU).map(|a| [radius * a.cos(), radius * a.sin()]).collect()
    }
}

/// `createPhoneScene`: the body, face glass, back, buttons and rear camera around a display of
/// `aspect` (short side over long side).
pub fn phone(profile: &Profile, aspect: f32) -> Model {
    let materials = vec![
        Material::lit(0xb5bcc7, 0.88, 0.27),
        Material::lit(0x141820, 0.15, 0.2),
        Material::lit(profile.back, 0.45, 0.32),
        Material::lit(0x071326, 0.6, 0.12),
        Material { unlit: true, ..Material::lit(0xf2ead6, 0., 1.) },
        Material { unlit: true, screen: true, ..Material::lit(0xffffff, 0., 1.) },
    ];
    let (metal, glass, back, lens, flash, screen) = (0, 1, 2, 3, 4, 5);
    let screen_width = SCREEN_HEIGHT * aspect;
    let (width, height) = (screen_width + profile.bezel * 2., SCREEN_HEIGHT + profile.bezel * 2.);
    let back_z = 0.01 - profile.depth;
    let mut b = Builder { meshes: Vec::new() };
    let body = rounded(width, height, profile.body_radius);
    b.slab(metal, &body, [0., 0.], 0.025 - profile.depth, 0.025 + 0.012);
    b.cap(glass, &rounded(width - 0.014, height - 0.014, profile.body_radius - 0.01), [0., 0.], 0.04, true, false);
    b.cap(back, &rounded(width - 0.012, height - 0.012, profile.body_radius - 0.01), [0., 0.], back_z - 0.012, false, false);
    b.cap(screen, &rounded(screen_width, SCREEN_HEIGHT, profile.screen_radius), [0., 0.], 0.043, true, true);
    for &(edge, offset, length) in profile.buttons {
        let (w, h) = if edge == 't' { (length, 0.026) } else { (0.026, length) };
        let at = match edge {
            't' => [offset, height / 2. + 0.015],
            'l' => [-(width / 2. + 0.015), offset],
            _ => [width / 2. + 0.015, offset],
        };
        let mid = (0.025 + back_z) / 2.;
        let half = profile.depth * 0.65 / 2.;
        b.slab(metal, &rounded(w, h, 0.), at, mid - half, mid + half);
    }
    // The rear camera faces -z, so its x runs mirrored.
    let [cw, ch, inset_x, inset_y, lens_radius] = profile.camera;
    let camera = [width / 2. - inset_x, height / 2. - inset_y];
    let plate = back_z - 0.012;
    b.slab(back, &rounded(cw, ch, cw.min(ch) / 4.), camera, plate - 0.032, plate);
    for l in profile.lenses {
        let at = [camera[0] - l[0], camera[1] + l[1]];
        b.slab(metal, &Builder::circle(lens_radius + 0.014), at, plate - 0.032 - 0.022, plate - 0.032);
        b.cap(lens, &Builder::circle(lens_radius), at, plate - 0.032 - 0.0225, false, false);
    }
    if let Some(f) = profile.flash {
        b.cap(flash, &Builder::circle(0.022), [camera[0] - f[0], camera[1] + f[1]], plate - 0.0325, false, false);
    }
    let mut model = Model { meshes: b.meshes, materials, images: Vec::new(), screen: [screen_width, SCREEN_HEIGHT, 0.043], faces: Vec::new(), min: [0.; 3], max: [0.; 3] };
    model.bounds();
    model
}


// `androidFoldScene.ts`.

const FOLD_DEPTH: f32 = 0.075;
const FOLD_INSET: f32 = 0.026;
const CREASE: f32 = 0.004;
const PIVOT_Z: f32 = FOLD_DEPTH / 2. + 0.005;
/// `DEFAULT_FOLD_INNER_ASPECT`: the inner display's width over height when the frame can't say.
const FOLD_INNER_ASPECT: f32 = 2076. / 2152.;

/// `isFoldInnerAspect`: whether a frame of `width` over `height` is the unfolded inner display.
pub fn fold_inner_aspect(width: u32, height: u32) -> f32 {
    let aspect = width as f32 / height as f32;
    if aspect.is_finite() && aspect > 0.75 && aspect < 1.5 { aspect } else { FOLD_INNER_ASPECT }
}

fn quad(out: &mut Vec<[f32; 2]>, a: [f32; 2], c: [f32; 2], b: [f32; 2]) {
    for i in 1..=8 {
        let t = i as f32 / 8.;
        let u = 1. - t;
        out.push([u * u * a[0] + 2. * u * t * c[0] + t * t * b[0], u * u * a[1] + 2. * u * t * c[1] + t * t * b[1]]);
    }
}

/// `panelPath`: one half's outline, rounded on its outer side, counter-clockwise about its centre,
/// and that centre.
fn panel(half_width: f32, left: bool, inset: f32, radius: f32, hinge_inset: f32) -> (Vec<[f32; 2]>, [f32; 2]) {
    let (l, r) = if left { (-half_width + inset, -CREASE / 2. - hinge_inset) } else { (CREASE / 2. + hinge_inset, half_width - inset) };
    let (b, t) = (-SCREEN_HEIGHT / 2. + inset, SCREEN_HEIGHT / 2. - inset);
    let mut out = Vec::new();
    if left {
        out.extend([[r, b], [r, t], [l + radius, t]]);
        quad(&mut out, [l + radius, t], [l, t], [l, t - radius]);
        out.push([l, b + radius]);
        quad(&mut out, [l, b + radius], [l, b], [l + radius, b]);
    } else {
        out.extend([[l, b], [r - radius, b]]);
        quad(&mut out, [r - radius, b], [r, b], [r, b + radius]);
        out.push([r, t - radius]);
        quad(&mut out, [r, t - radius], [r, t], [r - radius, t]);
        out.push([l, t]);
    }
    let centre = [(l + r) / 2., 0.];
    (out.into_iter().map(|p| [p[0] - centre[0], p[1]]).collect(), centre)
}

/// Turns `p` by `angle` about the y axis through `(0, 0, z)`, as three.js's `rotation.y` does.
fn turn(p: [f32; 3], angle: f32, z: f32) -> [f32; 3] {
    let (s, c) = angle.sin_cos();
    [p[0] * c + (p[2] - z) * s, p[1], -p[0] * s + (p[2] - z) * c + z]
}

/// `ANDROID_FOLD_TURN_MS`: how long the 3D view takes to fold to a new hinge angle.
const FOLD_TURN: f32 = 0.85;

/// Web's `foldTurn`: the hinge angle the 3D view shows, eased to each new one it's given.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct FoldTurn {
    shown: Option<f32>,
    turn: Option<(f32, f32, std::time::Instant)>,
}

impl FoldTurn {
    /// The angle to show at `now` for a device at `target`; a new target turns from wherever the
    /// hinge shows, and the first one shows at once.
    pub fn show(&mut self, target: Option<f32>, now: std::time::Instant) -> Option<f32> {
        let Some(target) = target else {
            *self = FoldTurn::default();
            return None;
        };
        let shown = *self.shown.get_or_insert(target);
        if self.turn.map_or(shown, |(_, to, _)| to) != target {
            self.turn = Some((shown, target, now));
        }
        if let Some((from, to, start)) = self.turn {
            let progress = (now.saturating_duration_since(start).as_secs_f32() / FOLD_TURN).min(1.);
            self.shown = Some(from + (to - from) * progress * progress * (3. - 2. * progress));
            if progress >= 1. {
                self.turn = None;
            }
        }
        self.shown
    }

    pub fn moving(&self) -> bool {
        self.turn.is_some()
    }
}

/// `createAndroidFoldScene` at a hinge `angle` (0 closed, 180 open) around an inner display of
/// `inner_aspect`: two halves on a spine, the continuous inner screen bending with the left half
/// from 90° open, the cover screen before that.
pub fn fold(angle: f32, inner_aspect: f32) -> Model {
    let materials = vec![
        Material::lit(0xa3abb2, 0.9, 0.28),
        Material::lit(0xc4cad0, 0.95, 0.16),
        Material::lit(0x0b0d10, 0.1, 0.2),
        Material::lit(0x2c3237, 0.35, 0.52),
        Material::lit(0x1a1e22, 0.55, 0.3),
        Material::lit(0x061022, 0.6, 0.1),
        Material { unlit: true, ..Material::lit(0xf2ead6, 0., 1.) },
        Material { unlit: true, screen: true, mapped: true, ..Material::lit(0xffffff, 0., 1.) },
    ];
    let (metal, polished, bezel, back_glass, island, lens, flash, screen) = (0, 1, 2, 3, 4, 5, 6, 7);
    let angle = angle.clamp(0., 180.);
    let hinge = std::f32::consts::PI * (1. - angle / 180.);
    let inner = angle >= 90.;
    let screen_height = SCREEN_HEIGHT - 2. * FOLD_INSET;
    let half_width = inner_aspect * screen_height / 2. + FOLD_INSET;
    let (d, h) = (FOLD_DEPTH, SCREEN_HEIGHT);
    let mut b = Builder { meshes: Vec::new() };
    let mut faces = Vec::new();

    let half = |b: &mut Builder, left: bool, back: usize| {
        let (outline, at) = panel(half_width, left, 0., 0.1, 0.);
        b.slab(metal, &outline, at, -d / 2., d / 2.);
        let (outline, at) = panel(half_width, left, 0.01, 0.095, 0.002);
        b.cap(bezel, &outline, at, d / 2. + 0.001, true, false);
        b.cap(back, &outline, at, -d / 2. - 0.001, false, false);
    };
    half(&mut b, true, bezel);
    // The cover display, on the left half's back.
    let (cover_w, cover_h) = (half_width - FOLD_INSET * 2. - 0.02, h - FOLD_INSET * 2. - 0.04);
    let cover_at = [-half_width / 2., 0., -d / 2. - 0.003];
    if !inner {
        b.cap(screen, &rounded(cover_w, cover_h, 0.07), [cover_at[0], cover_at[1]], cover_at[2], false, true);
        let m = b.meshes.last_mut().expect("just added");
        m.uvs = m.positions.iter().map(|p| [(cover_w / 2. - (p[0] - cover_at[0])) / cover_w, (p[1] + cover_h / 2.) / cover_h]).collect();
    }
    for m in &mut b.meshes {
        for p in &mut m.positions {
            *p = turn(*p, hinge, PIVOT_Z);
        }
        for n in &mut m.normals {
            *n = turn(*n, hinge, 0.);
        }
    }
    let inner_half = (half_width - FOLD_INSET - CREASE / 2.) / 2.;
    let inner_z = d / 2. + 0.003;
    let up = [0., h / 2. - FOLD_INSET, 0.];
    if inner {
        faces.push(Face { center: turn([-CREASE / 2. - inner_half, 0., inner_z], hinge, PIVOT_Z), right: turn([inner_half, 0., 0.], hinge, 0.), up, across: [0., 0.5] });
        faces.push(Face { center: [CREASE / 2. + inner_half, 0., inner_z], right: [inner_half, 0., 0.], up, across: [0.5, 1.] });
    } else {
        faces.push(Face { center: turn(cover_at, hinge, PIVOT_Z), right: turn([-cover_w / 2., 0., 0.], hinge, 0.), up: [0., cover_h / 2., 0.], across: [0., 1.] });
    }

    half(&mut b, false, back_glass);
    // The rear camera faces -z, so its x runs mirrored.
    let camera = [half_width - 0.07 - 0.23, h / 2. - 0.08 - 0.1];
    let z0 = -d / 2. - 0.002;
    b.slab(island, &rounded(0.46, 0.2, 0.07), camera, z0 - 0.026, z0);
    for (x, radius) in [(-0.14, 0.05), (-0.01, 0.05), (0.105, 0.036)] {
        let at = [camera[0] - x, camera[1]];
        b.slab(metal, &Builder::circle(radius + 0.012), at, z0 - 0.036, z0 - 0.024);
        b.cap(lens, &Builder::circle(radius), at, z0 - 0.0365, false, false);
    }
    b.cap(flash, &Builder::circle(0.018), [camera[0] - 0.185, camera[1] + 0.045], z0 - 0.0265, false, false);
    for (y, length) in [(0.52, 0.16), (0.2, 0.3)] {
        b.slab(metal, &rounded(0.02, length, 0.), [half_width + 0.008, y], -d * 0.225, d * 0.225);
    }

    // The spine: the back half of a cylinder about the hinge, turned half as far as the left half.
    let radius = PIVOT_Z + d / 2. - 0.002;
    let m = b.mesh(polished);
    let (start, sweep) = (std::f32::consts::FRAC_PI_2 + 0.15, std::f32::consts::PI - 0.3);
    for i in 0..=16 {
        let a = start + sweep * i as f32 / 16.;
        let normal = turn([a.sin(), 0., a.cos()], hinge / 2., 0.);
        for y in [-(h - 0.012) / 2., (h - 0.012) / 2.] {
            m.positions.push(turn([radius * a.sin(), y, radius * a.cos() + PIVOT_Z], hinge / 2., PIVOT_Z));
            m.normals.push(normal);
        }
        if i > 0 {
            let k = (i * 2) as u32;
            m.indices.extend([k - 2, k, k + 1, k - 2, k + 1, k - 1]);
        }
    }

    // The continuous inner screen, its left part bent with the left half.
    if inner {
        let width = 2. * (half_width - FOLD_INSET);
        let front = d / 2. + 0.004;
        let m = b.mesh(screen);
        m.screen = true;
        let (columns, rows) = (40, 48);
        for j in 0..=rows {
            let y = -screen_height / 2. + screen_height * j as f32 / rows as f32;
            let r = 0.076;
            let corner = (y.abs() - (screen_height / 2. - r)).max(0.);
            let limit = width / 2. - r + (r * r - corner * corner).max(0.).sqrt();
            for i in 0..=columns {
                let x = (-width / 2. + width * i as f32 / columns as f32).clamp(-limit, limit);
                let (p, n) = if x < 0. { (turn([x, y, front], hinge, PIVOT_Z), turn([0., 0., 1.], hinge, 0.)) } else { ([x, y, front], [0., 0., 1.]) };
                m.positions.push(p);
                m.normals.push(n);
                m.uvs.push([x / width + 0.5, y / screen_height + 0.5]);
            }
        }
        let stride = columns + 1;
        for j in 0..rows {
            for i in 0..columns {
                let k = j * stride + i;
                m.indices.extend([k, k + 1, k + stride + 1, k, k + stride + 1, k + stride]);
            }
        }
    }

    let mut model = Model { meshes: b.meshes, materials, images: Vec::new(), screen: [2. * (half_width - FOLD_INSET), screen_height, d / 2. + 0.004], faces, min: [0.; 3], max: [0.; 3] };
    model.bounds();
    model
}

// The view.

/// `phoneDisplayLayout`: how the framebuffer sits on the display.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Layout {
    pub aspect: f32,
    /// The screen's quarter turns: 0, -π/2 (landscape left), π/2 (landscape right) or π.
    pub rotation: f32,
    pub raw_landscape: bool,
}

pub fn layout(orientation: Option<&str>, width: u32, height: u32) -> Layout {
    let (w, h) = (if width > 0 { width as f32 } else { 900. }, if height > 0 { height as f32 } else { 1950. });
    use std::f32::consts::{FRAC_PI_2, PI};
    Layout {
        aspect: w.min(h) / w.max(h),
        rotation: match orientation {
            Some("landscape_left") => -FRAC_PI_2,
            Some("landscape_right") => FRAC_PI_2,
            Some("portrait_upside_down") => PI,
            _ => 0.,
        },
        raw_landscape: w > h,
    }
}

/// The root's `rotation` and the orientation group's `turn` about z.
pub fn world(rotation: super::motion::Quat, turn: f32) -> Mat4 {
    let (sz, cz) = turn.sin_cos();
    let turn: Mat4 = [cz, sz, 0., 0., -sz, cz, 0., 0., 0., 0., 1., 0., 0., 0., 0., 1.];
    mul(&super::motion::matrix(rotation), &turn)
}

/// The camera `deviceFraming.ts` settles on: looking down -z at the posed bounds' centre from far
/// enough that they fit `aspect` (width over height).
pub struct Camera {
    pub eye: [f32; 3],
    pub view_proj: Mat4,
    pub inverse: Mat4,
}

pub fn camera(model: &Model, world: &Mat4, aspect: f32) -> Camera {
    let (mut min, mut max) = ([f32::INFINITY; 3], [f32::NEG_INFINITY; 3]);
    for i in 0..8 {
        let corner = [0, 1, 2].map(|k| if i >> k & 1 == 1 { model.max[k] } else { model.min[k] });
        let p = point(world, corner);
        for k in 0..3 {
            min[k] = min[k].min(p[k]);
            max[k] = max[k].max(p[k]);
        }
    }
    let tan_y = (FOV_Y / 2.).tan();
    let tan_x = tan_y * aspect;
    let center = [(min[0] + max[0]) / 2., (min[1] + max[1]) / 2.];
    let size = [max[0] - min[0], max[1] - min[1]];
    let distance = (size[0] / tan_x).max(size[1] / tan_y) * 0.565 + max[2];
    let clearance = (size[0] / 2. / tan_x).max(size[1] / 2. / tan_y);
    let distance = distance.max(clearance * 1.02 + max[2]).max(1.);
    let eye = [center[0], center[1], distance];
    let (near, far) = (0.1f32, 30f32);
    let f = 1. / tan_y;
    // wgpu's clip space: z from 0 (near) to 1 (far).
    let proj: Mat4 = [f / aspect, 0., 0., 0., 0., f, 0., 0., 0., 0., far / (near - far), -1., 0., 0., near * far / (near - far), 0.];
    let view: Mat4 = [1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1., 0., -eye[0], -eye[1], -eye[2], 1.];
    let inverse_proj: Mat4 = [aspect / f, 0., 0., 0., 0., 1. / f, 0., 0., 0., 0., 0., (near - far) / (near * far), 0., 0., -1., 1. / near];
    let inverse_view: Mat4 = [1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1., 0., eye[0], eye[1], eye[2], 1.];
    Camera { eye, view_proj: mul(&proj, &view), inverse: mul(&inverse_view, &inverse_proj) }
}

/// `createDisplayProjection`: where a point of the viewport (0…1 each way, y down) lands on the
/// device's screen, as the screen's own 0…1 coordinates. `captured` keeps a touch going past the
/// display's edge by meeting its plane instead.
pub fn screen_point(model: &Model, world: &Mat4, camera: &Camera, layout: &Layout, x: f32, y: f32, captured: bool) -> Option<(f32, f32)> {
    let ndc = [x * 2. - 1., 1. - y * 2.];
    let far = {
        let p = [0, 1, 2, 3].map(|r| camera.inverse[r] * ndc[0] + camera.inverse[4 + r] * ndc[1] + camera.inverse[8 + r] * 1. + camera.inverse[12 + r]);
        [p[0] / p[3], p[1] / p[3], p[2] / p[3]]
    };
    let eye = camera.eye;
    // `world` is a rotation, so its transpose takes the ray into the model's space.
    let back = |v: [f32; 3]| [0, 1, 2].map(|c| world[c * 4] * v[0] + world[c * 4 + 1] * v[1] + world[c * 4 + 2] * v[2]);
    let origin = back(eye);
    let dir = back([far[0] - eye[0], far[1] - eye[1], far[2] - eye[2]]);
    if !model.faces.is_empty() {
        return face_point(&model.faces, origin, dir, layout, captured);
    }
    let [width, height, z] = model.screen;
    if dir[2].abs() < 1e-6 {
        return None;
    }
    let t = (z - origin[2]) / dir[2];
    if t <= 0. {
        return None;
    }
    let local = [origin[0] + dir[0] * t, origin[1] + dir[1] * t];
    let (u, v) = ((local[0] + width / 2.) / width, (local[1] + height / 2.) / height);
    if !captured && !((0. ..=1.).contains(&u) && (0. ..=1.).contains(&v)) {
        return None;
    }
    let (u, v) = (u.clamp(0., 1.), v.clamp(0., 1.));
    use std::f32::consts::{FRAC_PI_2, PI};
    Some(match layout.rotation {
        r if r == -FRAC_PI_2 => (v, u),
        r if r == FRAC_PI_2 => (1. - v, 1. - u),
        r if r == PI => (1. - u, v),
        _ => (u, 1. - v),
    })
}

fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

/// The fold's `screenPoint`: the nearest display the ray meets, else for a captured touch the
/// display whose plane it meets closest to its bounds.
fn face_point(faces: &[Face], origin: [f32; 3], dir: [f32; 3], layout: &Layout, captured: bool) -> Option<(f32, f32)> {
    let mut best: Option<(bool, f32, f32, f32, &Face)> = None;
    for face in faces {
        let (r, u) = (face.right, face.up);
        let normal = [r[1] * u[2] - r[2] * u[1], r[2] * u[0] - r[0] * u[2], r[0] * u[1] - r[1] * u[0]];
        let denom = dot(normal, dir);
        if denom.abs() < 1e-9 {
            continue;
        }
        let t = dot(normal, [face.center[0] - origin[0], face.center[1] - origin[1], face.center[2] - origin[2]]) / denom;
        if t <= 0. {
            continue;
        }
        let off = [origin[0] + dir[0] * t - face.center[0], origin[1] + dir[1] * t - face.center[1], origin[2] + dir[2] * t - face.center[2]];
        let (s, v) = (dot(off, r) / dot(r, r), dot(off, u) / dot(u, u));
        let inside = s.abs() <= 1. && v.abs() <= 1.;
        // A hit ranks by distance; a miss by how far outside the display it lands.
        let rank = if inside { t } else { s.abs().max(v.abs()) };
        if best.is_none_or(|(hit, best_rank, ..)| (inside && !hit) || (inside == hit && rank < best_rank)) {
            best = Some((inside, rank, s, v, face));
        }
    }
    let (inside, _, s, v, face) = best?;
    if !inside && !captured {
        return None;
    }
    let (s, v) = (s.clamp(-1., 1.), v.clamp(-1., 1.));
    let x = face.across[0] + (s + 1.) / 2. * (face.across[1] - face.across[0]);
    let y = (1. - v) / 2.;
    Some(if layout.rotation == std::f32::consts::PI { (1. - x, 1. - y) } else { (x, y) })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn web_models_load_with_one_normalized_display() {
        for id in ["iphone-18-pro", "iphone-18-pro-max", "ipad-pro-13-m5"] {
            let model = parse_glb(glb(id).unwrap()).unwrap_or_else(|e| panic!("{id}: {e}"));
            let screens: Vec<&Mesh> = model.meshes.iter().filter(|m| m.screen).collect();
            assert!(!screens.is_empty(), "{id} has a display");
            assert!((model.screen[1] - SCREEN_HEIGHT).abs() < 0.001);
            assert!(model.meshes.iter().all(|m| m.normals.len() == m.positions.len() && m.indices.len() % 3 == 0));
            assert!(model.meshes.iter().filter_map(|m| model.materials[m.material].texture).all(|t| model.images[t].rgba.len() == (model.images[t].width * model.images[t].height * 4) as usize));
            assert!(model.min[2] < model.screen[2] && model.screen[2] <= model.max[2]);
        }
        let ipad = load("ipad-pro-13-m5").unwrap().unwrap();
        let keyboard = load(KEYBOARD).unwrap().unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(keyboard.screen, ipad.screen, "the keyboard leaves the iPad's display where it was");
        assert!(keyboard.meshes.len() > ipad.meshes.len() && keyboard.meshes.iter().filter(|m| m.screen).count() == ipad.meshes.iter().filter(|m| m.screen).count());
        assert!(keyboard.meshes.iter().all(|m| m.material < keyboard.materials.len()) && keyboard.materials.iter().filter_map(|m| m.texture).all(|t| t < keyboard.images.len()));
        assert!((0..3).all(|k| keyboard.min[k] <= ipad.min[k] && keyboard.max[k] >= ipad.max[k]) && (0..3).any(|k| keyboard.min[k] < ipad.min[k] - 0.1 || keyboard.max[k] > ipad.max[k] + 0.1), "it adds to the iPad's bounds");
        assert_eq!(model_id(false, "iPhone 18 Pro"), Some("iphone-18-pro"));
        assert_eq!(model_id(true, "iPhone 18 Pro"), None);
        assert_eq!(model_id(false, "iPhone 16 Pro"), None);
        assert!(parse_glb(b"glTF\x01\0\0\0").is_err());
    }

    #[test]
    fn the_viewport_meets_the_screen_where_the_camera_shows_it() {
        let model = phone(&profile(false, "iPhone 16 Pro", 0.46), 0.46);
        assert_eq!(profile(true, "Pixel Tablet", 0.6).id, "android-tablet");
        assert_eq!(profile(false, "iPad Air", 0.7).id, "ios-tablet");
        let portrait = layout(Some("portrait"), 1179, 2556);
        let world = world(super::super::motion::IDENTITY, portrait.rotation);
        let cam = camera(&model, &world, 0.6);
        let centre = screen_point(&model, &world, &cam, &portrait, 0.5, 0.5, false).unwrap();
        assert!((centre.0 - 0.5).abs() < 0.01 && (centre.1 - 0.5).abs() < 0.01, "{centre:?}");
        let (x0, y0) = screen_point(&model, &world, &cam, &portrait, 0.45, 0.4, false).unwrap();
        assert!(x0 < 0.5 && y0 < 0.5, "up and left on the viewport is up and left on the screen");
        assert_eq!(screen_point(&model, &world, &cam, &portrait, 0.01, 0.5, false), None, "the bezel isn't the screen");
        assert!(screen_point(&model, &world, &cam, &portrait, 0.01, 0.5, true).is_some(), "a captured touch keeps going");
        // Landscape left: the screen's top runs along the phone's left side.
        let left = layout(Some("landscape_left"), 2556, 1179);
        let world = world(super::super::motion::IDENTITY, left.rotation);
        let cam = camera(&model, &world, 1.6);
        let (x, y) = screen_point(&model, &world, &cam, &left, 0.4, 0.5, false).unwrap();
        assert!(x < 0.5 && (y - 0.5).abs() < 0.01, "{x} {y}");
    }

    #[test]
    fn a_foldable_bends_its_inner_screen_and_closes_on_its_cover() {
        let aspect = fold_inner_aspect(2076, 2152);
        assert!((aspect - FOLD_INNER_ASPECT).abs() < 1e-6);
        assert_eq!(fold_inner_aspect(1080, 2424), FOLD_INNER_ASPECT, "a folded frame isn't the inner display");
        let inner = layout(None, 2076, 2152);
        let world = world(super::super::motion::IDENTITY, 0.);
        let open = fold(180., aspect);
        assert_eq!(open.faces.len(), 2, "open, the inner display's two halves take touches");
        let cam = camera(&open, &world, 1.);
        let (x, y) = screen_point(&open, &world, &cam, &inner, 0.5, 0.5, false).unwrap();
        assert!((x - 0.5).abs() < 0.02 && (y - 0.5).abs() < 0.02, "{x} {y}");
        let (x, y) = screen_point(&open, &world, &cam, &inner, 0.4, 0.4, false).unwrap();
        assert!(x < 0.5 && y < 0.5, "the left half is the screen's left: {x} {y}");
        assert!(screen_point(&open, &world, &cam, &inner, 0.6, 0.5, false).unwrap().0 > 0.5);
        assert_eq!(screen_point(&open, &world, &cam, &inner, 0.01, 0.5, false), None, "the frame isn't the screen");
        let mut turn = FoldTurn::default();
        let t0 = std::time::Instant::now();
        assert_eq!(turn.show(Some(180.), t0), Some(180.), "the first angle shows at once");
        assert!(!turn.moving());
        let ms = |n| t0 + std::time::Duration::from_millis(n);
        let a = turn.show(Some(0.), ms(100)).unwrap();
        assert!(turn.moving() && a == 180., "a new angle starts turning from the shown one");
        let b = turn.show(Some(0.), ms(400)).unwrap();
        assert!(b < 180. && b > 0., "{b}");
        let c = turn.show(Some(90.), ms(500)).unwrap();
        assert!((c - b).abs() < 1e-3, "a change mid-turn starts from where the hinge shows");
        assert_eq!(turn.show(Some(90.), ms(1400)), Some(90.));
        assert!(!turn.moving(), "and it ends");
        assert_eq!(turn.show(None, ms(1500)), None);
        let half = fold(90., aspect);
        assert!(half.max[2] > open.max[2] + 0.5, "half open, the left half stands toward the camera");
        let closed = fold(0., aspect);
        let screens: Vec<&Mesh> = closed.meshes.iter().filter(|m| m.screen).collect();
        assert_eq!((screens.len(), closed.faces.len()), (1, 1), "closed, only the cover display shows");
        assert!(screens[0].uvs.len() == screens[0].positions.len() && screens[0].uvs.iter().flatten().all(|c| (0. ..=1.).contains(c)));
        let cam = camera(&closed, &world, 0.5);
        let (x, y) = screen_point(&closed, &world, &cam, &inner, 0.5, 0.5, false).unwrap();
        assert!((x - 0.5).abs() < 0.1 && (y - 0.5).abs() < 0.05, "{x} {y}");
        assert!(screen_point(&closed, &world, &cam, &inner, 0.4, 0.5, false).unwrap().0 < x, "the cover reads left to right");
    }
}
