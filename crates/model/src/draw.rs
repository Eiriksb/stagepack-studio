//! The drawer: a thread of its own with a graphics device of its own, drawing models off-screen
//! and reading the pictures back. Asking it for anything returns at once; every wait on the
//! graphics card happens on its thread. A device lost is made again, and every model uploaded
//! to it uploaded again, on the next draw.

use std::collections::HashMap;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use crate::device::{self, AdapterKind, COLOUR, DEPTH, Gpu};
use crate::maths::{cross, minus, unit};
use crate::view::MOST_PITCH;
use crate::{CantDraw, Model, ModelFacts, Picture, View};

/// How a drawer is made.
#[derive(Clone, Debug, Default)]
pub struct DrawerOptions {
    /// Draws only on an adapter of this kind; on the best there is when `None`.
    pub only: Option<AdapterKind>,
    /// For tests: finds no adapter at all, as a computer with no graphics would.
    pub no_adapter: bool,
    /// For tests: makes the device this much later, as a slow one would be.
    pub device_delay: Duration,
}

/// How long the drawer waits for the card to finish a picture before taking the device as lost.
const MOST_WAIT: Duration = Duration::from_secs(20);

static NEXT_ID: AtomicU64 = AtomicU64::new(1);

fn next_id() -> u64 {
    NEXT_ID.fetch_add(1, Ordering::Relaxed)
}

type Ready = Arc<Mutex<Option<Result<AdapterKind, CantDraw>>>>;

/// Draws models off-screen, on a thread of its own with a device of its own. One is enough for
/// an app; copies of it share the one thread. The thread ends when the last copy is dropped.
#[derive(Clone)]
pub struct Drawer {
    shared: Arc<Shared>,
}

struct Shared {
    id: u64,
    jobs: Sender<Job>,
    ready: Ready,
    /// Says, by closing, that the thread has ended and let go of its device.
    ended: Mutex<Receiver<()>>,
}

impl Drop for Shared {
    fn drop(&mut self) {
        let _ = self.jobs.send(Job::Stop);
    }
}

/// A model uploaded to a drawer, there until this is dropped.
pub struct Scene {
    id: u64,
    drawer: u64,
    facts: ModelFacts,
    jobs: Sender<Job>,
}

impl Scene {
    /// What the model uploaded is.
    pub fn facts(&self) -> &ModelFacts {
        &self.facts
    }
}

impl Drop for Scene {
    fn drop(&mut self) {
        let _ = self.jobs.send(Job::Release(self.id));
    }
}

enum Job {
    Upload(u64, Box<Model>),
    Release(u64),
    Draw { scene: u64, view: View, width: u32, height: u32, answer: Sender<Result<Picture, CantDraw>> },
    Lose,
    Stop,
}

impl Drawer {
    /// A drawer on the best adapter there is: a discrete card, else an integrated one, else a
    /// software one. Returns at once; the device is made on the drawer's thread.
    pub fn start() -> Drawer {
        Drawer::start_with(DrawerOptions::default())
    }

    pub fn start_with(options: DrawerOptions) -> Drawer {
        let (jobs, receiver) = mpsc::channel();
        let ready: Ready = Arc::new(Mutex::new(None));
        let theirs = ready.clone();
        let (ending, ended) = mpsc::channel();
        let started = std::thread::Builder::new().name("model drawer".into()).spawn(move || run(options, receiver, theirs, ending));
        if started.is_err() {
            *ready.lock().unwrap_or_else(|e| e.into_inner()) = Some(Err(CantDraw::NoAdapter));
        }
        Drawer { shared: Arc::new(Shared { id: next_id(), jobs, ready, ended: Mutex::new(ended) }) }
    }

    /// `None` while the device is being made; then the kind of adapter drawn with, or why
    /// nothing can be.
    pub fn ready(&self) -> Option<Result<AdapterKind, CantDraw>> {
        *self.shared.ready.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Uploads `model` to the drawer's device, on the drawer's thread.
    pub fn upload(&self, model: Model) -> Scene {
        let id = next_id();
        let facts = model.facts;
        let _ = self.shared.jobs.send(Job::Upload(id, Box::new(model)));
        Scene { id, drawer: self.shared.id, facts, jobs: self.shared.jobs.clone() }
    }

    /// Draws `scene` seen from `view` into a picture `width` by `height`, with depth and a
    /// simple light, and reads it back. The answer comes on the receiver; this never waits.
    pub fn draw(&self, scene: &Scene, view: View, width: u32, height: u32) -> Receiver<Result<Picture, CantDraw>> {
        let (answer, receiver) = mpsc::channel();
        if scene.drawer != self.shared.id {
            // A scene is only on the device of the drawer it was uploaded to.
            let _ = answer.send(Err(CantDraw::CouldNotRead));
        } else if let Err(mpsc::SendError(Job::Draw { answer, .. })) =
            self.shared.jobs.send(Job::Draw { scene: scene.id, view, width, height, answer })
        {
            let _ = answer.send(Err(CantDraw::NoAdapter));
        }
        receiver
    }

    /// Ends the drawer's thread, for every copy of this drawer, and waits up to `within` for it to
    /// let go of its device: true if it did. A program ending while a device is being let go of
    /// can crash in the graphics driver, so call this before the program ends; it waits, so
    /// never call it on a window's thread while the window is in use.
    pub fn close(self, within: Duration) -> bool {
        let _ = self.shared.jobs.send(Job::Stop);
        let ended = self.shared.ended.lock().unwrap_or_else(|e| e.into_inner());
        matches!(ended.recv_timeout(within), Err(mpsc::RecvTimeoutError::Disconnected))
    }

    /// For tests: loses the device, as a card being reset would. The next draw makes it again.
    pub fn lose_device(&self) {
        let _ = self.shared.jobs.send(Job::Lose);
    }
}

fn run(options: DrawerOptions, jobs: Receiver<Job>, ready: Ready, ending: Sender<()>) {
    // Dropped last, once the device has been let go of.
    let _ending = ending;
    std::thread::sleep(options.device_delay);
    let mut drawing = Drawing { only: options.only, card: None, scenes: HashMap::new(), ready };
    if !options.no_adapter {
        drawing.make();
    }
    drawing.say_ready();
    while let Ok(job) = jobs.recv() {
        match job {
            Job::Upload(id, model) => {
                let kept = match &drawing.card {
                    Some(card) => Kept { on_card: upload_guarded(card, &model), model: *model },
                    // With no device, and none to be made, the model is never drawn: only that
                    // it's there is kept.
                    None => Kept { model: Model { meshes: Vec::new(), points: None, texture: None, facts: model.facts }, on_card: None },
                };
                drawing.scenes.insert(id, kept);
            }
            Job::Release(id) => {
                drawing.scenes.remove(&id);
            }
            Job::Draw { scene, view, width, height, answer } => {
                let _ = answer.send(drawing.draw(scene, view, width, height));
            }
            Job::Lose => {
                if let Some(card) = &drawing.card {
                    card.gpu.lose();
                }
            }
            Job::Stop => break,
        }
    }
}

/// What the drawer's thread holds.
struct Drawing {
    only: Option<AdapterKind>,
    /// The device and what's made on it, or `None` where none can be had.
    card: Option<Card>,
    scenes: HashMap<u64, Kept>,
    ready: Ready,
}

/// A model, kept so that it can be uploaded again to a device made after one was lost.
struct Kept {
    model: Model,
    /// `None` until it's uploaded to the device there is now.
    on_card: Option<Result<OnCard, CantDraw>>,
}

struct Card {
    gpu: Gpu,
    kit: Kit,
}

/// Why a picture wasn't drawn: the device failed, or it can't be drawn on this one.
enum Failure {
    Lost,
    Cant(CantDraw),
}

impl Drawing {
    fn make(&mut self) {
        self.card = None;
        let only = self.only;
        if let Ok(Ok(gpu)) = catch_unwind(move || device::make(only))
            && let Ok(kit) = catch_unwind(AssertUnwindSafe(|| Kit::new(&gpu)))
        {
            self.card = Some(Card { gpu, kit });
        }
    }

    fn say_ready(&self) {
        let ready = self.card.as_ref().map(|card| card.gpu.kind).ok_or(CantDraw::NoAdapter);
        *self.ready.lock().unwrap_or_else(|e| e.into_inner()) = Some(ready);
    }

    /// Makes the device again if it was lost, uploading every model again when it's next drawn.
    fn mend(&mut self) {
        if self.card.as_ref().is_some_and(|card| card.gpu.broken()) {
            for kept in self.scenes.values_mut() {
                kept.on_card = None;
            }
            self.make();
            self.say_ready();
            if self.card.is_none() {
                for kept in self.scenes.values_mut() {
                    kept.model = Model { meshes: Vec::new(), points: None, texture: None, facts: kept.model.facts };
                }
            }
        }
    }

    fn draw(&mut self, scene: u64, view: View, width: u32, height: u32) -> Result<Picture, CantDraw> {
        if !self.scenes.contains_key(&scene) {
            return Err(CantDraw::CouldNotRead);
        }
        // Once to draw, and once more on a device made again if it was lost while drawing.
        for _ in 0..2 {
            self.mend();
            let Some(card) = &mut self.card else { return Err(CantDraw::NoAdapter) };
            let most = card.gpu.limits.max_texture_dimension_2d;
            if width > most || height > most {
                return Err(CantDraw::TooLarge);
            }
            if width == 0 || height == 0 {
                return Ok(Picture { width, height, bgra: Vec::new(), drawing: Duration::ZERO, reading_back: Duration::ZERO });
            }
            let Some(kept) = self.scenes.get_mut(&scene) else { return Err(CantDraw::CouldNotRead) };
            if kept.on_card.is_none() {
                kept.on_card = upload_guarded(card, &kept.model);
            }
            match &kept.on_card {
                Some(Ok(on_card)) => {
                    match catch_unwind(AssertUnwindSafe(|| render(&card.gpu, &mut card.kit, on_card, view, width, height))) {
                        Ok(Ok(picture)) => return Ok(picture),
                        Ok(Err(Failure::Cant(why))) => return Err(why),
                        Ok(Err(Failure::Lost)) | Err(_) => card.gpu.set_broken(),
                    }
                }
                Some(Err(why)) => return Err(*why),
                // Uploading found the device lost.
                None => card.gpu.set_broken(),
            }
        }
        Err(CantDraw::NoAdapter)
    }
}

/// Uploads `model`: `None` if the device was found lost doing it.
fn upload_guarded(card: &Card, model: &Model) -> Option<Result<OnCard, CantDraw>> {
    match catch_unwind(AssertUnwindSafe(|| upload(card, model))) {
        Ok(Ok(on_card)) if !card.gpu.broken() => Some(Ok(on_card)),
        Ok(Err(Failure::Cant(why))) => Some(Err(why)),
        _ => {
            card.gpu.set_broken();
            None
        }
    }
}

/// The pipelines and pictures a device draws with.
struct Kit {
    meshes: wgpu::RenderPipeline,
    meshes_both_sides: wgpu::RenderPipeline,
    points: wgpu::RenderPipeline,
    texture_layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    frame: wgpu::Buffer,
    frame_group: wgpu::BindGroup,
    /// Coloured from for a mesh with no texture.
    white: wgpu::BindGroup,
    targets: Option<Targets>,
}

/// What a picture of one size is drawn into and read back through.
struct Targets {
    width: u32,
    height: u32,
    /// Drawn into, with several samples a pixel, where the device can.
    many: Option<wgpu::TextureView>,
    /// The picture, one sample a pixel.
    colour: wgpu::Texture,
    colour_view: wgpu::TextureView,
    depth: wgpu::TextureView,
    read_back: wgpu::Buffer,
    /// Each row's bytes as the card copies them out, rounded up to what it asks.
    row: u32,
}

/// The view and light, as the shaders take them.
const FRAME_BYTES: u64 = 80;
const MESH_STRIDE: u64 = 48;
const POINT_STRIDE: u64 = 16;

impl Kit {
    fn new(gpu: &Gpu) -> Kit {
        let device = &gpu.device;
        let frame_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("frame"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: wgpu::BufferSize::new(FRAME_BYTES),
                },
                count: None,
            }],
        });
        let texture_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("texture"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let frame = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("frame"),
            size: FRAME_BYTES,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let frame_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("frame"),
            layout: &frame_layout,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: frame.as_entire_binding() }],
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("texture"),
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::Repeat,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let white = texture_group(gpu, &texture_layout, &sampler, 1, 1, &[255; 4]);

        let mesh_shader = device.create_shader_module(wgpu::include_wgsl!("shaders/mesh.wgsl"));
        let points_shader = device.create_shader_module(wgpu::include_wgsl!("shaders/points.wgsl"));
        let mesh_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("meshes"),
            bind_group_layouts: &[Some(&frame_layout), Some(&texture_layout)],
            immediate_size: 0,
        });
        let points_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("points"),
            bind_group_layouts: &[Some(&frame_layout)],
            immediate_size: 0,
        });
        let mesh_attributes = wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3, 2 => Float32x2, 3 => Float32x4];
        let point_attributes = wgpu::vertex_attr_array![0 => Float32x3, 1 => Unorm8x4];
        let pipeline = |label, layout, shader, stride, attributes: &[wgpu::VertexAttribute], topology, cull_mode| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(layout),
                vertex: wgpu::VertexState {
                    module: shader,
                    entry_point: Some("vertex"),
                    compilation_options: Default::default(),
                    buffers: &[Some(wgpu::VertexBufferLayout {
                        array_stride: stride,
                        step_mode: wgpu::VertexStepMode::Vertex,
                        attributes,
                    })],
                },
                primitive: wgpu::PrimitiveState {
                    topology,
                    front_face: wgpu::FrontFace::Ccw,
                    cull_mode,
                    ..Default::default()
                },
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: DEPTH,
                    depth_write_enabled: Some(true),
                    depth_compare: Some(wgpu::CompareFunction::Less),
                    stencil: Default::default(),
                    bias: Default::default(),
                }),
                multisample: wgpu::MultisampleState { count: gpu.samples, ..Default::default() },
                fragment: Some(wgpu::FragmentState {
                    module: shader,
                    entry_point: Some("fragment"),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState { format: COLOUR, blend: None, write_mask: wgpu::ColorWrites::ALL })],
                }),
                multiview_mask: None,
                cache: None,
            })
        };
        let triangles = wgpu::PrimitiveTopology::TriangleList;
        Kit {
            meshes: pipeline("meshes", &mesh_layout, &mesh_shader, MESH_STRIDE, &mesh_attributes, triangles, Some(wgpu::Face::Back)),
            meshes_both_sides: pipeline("meshes seen from behind too", &mesh_layout, &mesh_shader, MESH_STRIDE, &mesh_attributes, triangles, None),
            points: pipeline("points", &points_layout, &points_shader, POINT_STRIDE, &point_attributes, wgpu::PrimitiveTopology::PointList, None),
            texture_layout,
            sampler,
            frame,
            frame_group,
            white,
            targets: None,
        }
    }

    fn targets(&mut self, gpu: &Gpu, width: u32, height: u32) -> &Targets {
        if self.targets.as_ref().is_none_or(|t| (t.width, t.height) != (width, height)) {
            self.targets = None;
            let size = wgpu::Extent3d { width, height, depth_or_array_layers: 1 };
            let texture = |label, format, samples, usage| {
                gpu.device.create_texture(&wgpu::TextureDescriptor {
                    label: Some(label),
                    size,
                    mip_level_count: 1,
                    sample_count: samples,
                    dimension: wgpu::TextureDimension::D2,
                    format,
                    usage,
                    view_formats: &[],
                })
            };
            let drawn = wgpu::TextureUsages::RENDER_ATTACHMENT;
            let many = (gpu.samples > 1).then(|| texture("drawn", COLOUR, gpu.samples, drawn).create_view(&Default::default()));
            let colour = texture("picture", COLOUR, 1, drawn | wgpu::TextureUsages::COPY_SRC);
            let colour_view = colour.create_view(&Default::default());
            let depth = texture("depth", DEPTH, gpu.samples, drawn).create_view(&Default::default());
            let row = (width * 4).next_multiple_of(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT);
            let read_back = gpu.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("read back"),
                size: u64::from(row) * u64::from(height),
                usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            self.targets = Some(Targets { width, height, many, colour, colour_view, depth, read_back, row });
        }
        self.targets.as_ref().expect("just made")
    }
}

fn texture_group(gpu: &Gpu, layout: &wgpu::BindGroupLayout, sampler: &wgpu::Sampler, width: u32, height: u32, rgba: &[u8]) -> wgpu::BindGroup {
    let size = wgpu::Extent3d { width, height, depth_or_array_layers: 1 };
    let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("model texture"),
        size,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8UnormSrgb,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    gpu.queue.write_texture(
        wgpu::TexelCopyTextureInfo { texture: &texture, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
        rgba,
        wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(width * 4), rows_per_image: Some(height) },
        size,
    );
    let view = texture.create_view(&Default::default());
    gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("model texture"),
        layout,
        entries: &[
            wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&view) },
            wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(sampler) },
        ],
    })
}

/// A model on the device.
struct OnCard {
    meshes: Vec<MeshOnCard>,
    points: Option<(wgpu::Buffer, u32)>,
    texture: Option<wgpu::BindGroup>,
    /// The middle of the model and the radius of a sphere round it, so that nothing of it is cut
    /// off by being too near or too far.
    middle: [f32; 3],
    radius: f32,
}

struct MeshOnCard {
    vertices: wgpu::Buffer,
    indices: wgpu::Buffer,
    count: u32,
    textured: bool,
    double_sided: bool,
}

/// A buffer of `count` items of `stride` bytes, each written by `fill`, put straight into the
/// device's memory a piece at a time rather than made whole first.
fn filled(gpu: &Gpu, label: &str, count: usize, stride: usize, usage: wgpu::BufferUsages, fill: impl Fn(usize, &mut [u8])) -> Result<wgpu::Buffer, Failure> {
    let size = count as u64 * stride as u64;
    if size > gpu.limits.max_buffer_size {
        return Err(Failure::Cant(CantDraw::TooLarge));
    }
    let buffer = gpu.device.create_buffer(&wgpu::BufferDescriptor { label: Some(label), size, usage, mapped_at_creation: true });
    {
        let mut mapped = buffer.slice(..).get_mapped_range_mut().map_err(|_| Failure::Lost)?;
        let each = (PIECE / stride).max(1);
        let mut piece = vec![0u8; each * stride];
        for first in (0..count).step_by(each) {
            let items = each.min(count - first);
            for (i, item) in piece.chunks_exact_mut(stride).take(items).enumerate() {
                fill(first + i, item);
            }
            let at = first * stride;
            mapped.slice(at..at + items * stride).copy_from_slice(&piece[..items * stride]);
        }
    }
    buffer.unmap();
    Ok(buffer)
}

/// How much of a buffer is made at a time before it's put on the device.
const PIECE: usize = 64 * 1024;

fn put(to: &mut [u8], values: &[f32]) {
    for (bytes, value) in to.as_chunks_mut::<4>().0.iter_mut().zip(values) {
        bytes.copy_from_slice(&value.to_ne_bytes());
    }
}

fn upload(card: &Card, model: &Model) -> Result<OnCard, Failure> {
    let gpu = &card.gpu;
    let mut meshes = Vec::new();
    for mesh in &model.meshes {
        if mesh.indices.is_empty() || mesh.positions.is_empty() {
            continue;
        }
        let count = u32::try_from(mesh.indices.len()).map_err(|_| Failure::Cant(CantDraw::TooLarge))?;
        let vertices = filled(gpu, "mesh", mesh.positions.len(), MESH_STRIDE as usize, wgpu::BufferUsages::VERTEX, |i, corner| {
            put(&mut corner[0..12], &mesh.positions[i]);
            put(&mut corner[12..24], mesh.normals.get(i).unwrap_or(&[0.0, 0.0, 1.0]));
            put(&mut corner[24..32], mesh.texture_coords.get(i).unwrap_or(&[0.0, 0.0]));
            put(&mut corner[32..48], mesh.colours.get(i).unwrap_or(&[1.0; 4]));
        })?;
        let indices = filled(gpu, "mesh indices", mesh.indices.len(), 4, wgpu::BufferUsages::INDEX, |i, bytes| {
            bytes.copy_from_slice(&mesh.indices[i].to_ne_bytes());
        })?;
        meshes.push(MeshOnCard { vertices, indices, count, textured: mesh.textured, double_sided: mesh.double_sided });
    }
    let points = match &model.points {
        Some(points) if !points.positions.is_empty() => {
            let count = u32::try_from(points.positions.len()).map_err(|_| Failure::Cant(CantDraw::TooLarge))?;
            let buffer = filled(gpu, "points", points.positions.len(), POINT_STRIDE as usize, wgpu::BufferUsages::VERTEX, |i, point| {
                put(&mut point[0..12], &points.positions[i]);
                point[12..16].copy_from_slice(points.colours.get(i).unwrap_or(&[255; 4]));
            })?;
            Some((buffer, count))
        }
        _ => None,
    };
    let texture = match &model.texture {
        Some(texture) if meshes.iter().any(|mesh| mesh.textured) => {
            let most = gpu.limits.max_texture_dimension_2d;
            if texture.width > most || texture.height > most {
                return Err(Failure::Cant(CantDraw::TooLarge));
            }
            if texture.width == 0 || texture.height == 0 || texture.rgba.len() as u64 != u64::from(texture.width) * u64::from(texture.height) * 4 {
                return Err(Failure::Cant(CantDraw::Damaged));
            }
            Some(texture_group(gpu, &card.kit.texture_layout, &card.kit.sampler, texture.width, texture.height, &texture.rgba))
        }
        _ => None,
    };
    let (low, high) = model.facts.bounds;
    let mut middle = [0, 1, 2].map(|i| (low[i] + high[i]) / 2.0);
    let half = [0, 1, 2].map(|i| (high[i] - low[i]) / 2.0);
    let mut radius = (half[0] * half[0] + half[1] * half[1] + half[2] * half[2]).sqrt();
    if !middle.iter().all(|v| v.is_finite()) || !radius.is_finite() {
        (middle, radius) = ([0.0; 3], 1.0);
    }
    Ok(OnCard { meshes, points, texture, middle, radius })
}

type Matrix = [f32; 16];

fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

/// The view and projection, by columns, and where the light comes from in the model's space.
fn frame(view: View, middle: [f32; 3], radius: f32, width: u32, height: u32) -> (Matrix, [f32; 3]) {
    let distance = if view.distance.is_finite() && view.distance > 0.0 { view.distance } else { 1.0 };
    let target = if view.target.iter().all(|v| v.is_finite()) { view.target } else { middle };
    let view = View {
        yaw: if view.yaw.is_finite() { view.yaw } else { 0.0 },
        pitch: if view.pitch.is_finite() { view.pitch.clamp(-MOST_PITCH, MOST_PITCH) } else { 0.0 },
        distance,
        target,
    };
    let eye = view.eye();
    let forward = unit(minus(target, eye)).unwrap_or([0.0, 0.0, -1.0]);
    let side = unit(cross(forward, [0.0, 1.0, 0.0])).unwrap_or([1.0, 0.0, 0.0]);
    let up = cross(side, forward);
    let look: Matrix = [
        side[0], up[0], -forward[0], 0.0,
        side[1], up[1], -forward[1], 0.0,
        side[2], up[2], -forward[2], 0.0,
        -dot(side, eye), -dot(up, eye), dot(forward, eye), 1.0,
    ];
    // Near and far enough to hold the whole of the model's sphere, from wherever the eye is.
    let from_middle = minus(middle, eye);
    let reach = dot(from_middle, from_middle).sqrt();
    let far = (reach + radius * 1.05).max(distance * 1e-3).max(1e-6);
    let near = (reach - radius * 1.05).max(far * 1e-4);
    let focal = 1.0 / (View::FIELD_OF_VIEW / 2.0).to_radians().tan();
    let shorter = width.min(height) as f32;
    let (sx, sy) = (focal * shorter / width as f32, focal * shorter / height as f32);
    let depth = far / (near - far);
    let projection: Matrix = [
        sx, 0.0, 0.0, 0.0,
        0.0, sy, 0.0, 0.0,
        0.0, 0.0, depth, -1.0,
        0.0, 0.0, near * depth, 0.0,
    ];
    let mut both = [0.0; 16];
    for column in 0..4 {
        for row in 0..4 {
            both[column * 4 + row] = (0..4).map(|k| projection[k * 4 + row] * look[column * 4 + k]).sum();
        }
    }
    // From beside the eye, up and to the left.
    let light = [-0.25, 0.35, 1.0];
    let size = dot(light, light).sqrt();
    let light = [0, 1, 2].map(|i| (side[i] * light[0] + up[i] * light[1] - forward[i] * light[2]) / size);
    (both, light)
}

fn wait(gpu: &Gpu, submitted: wgpu::SubmissionIndex) -> Result<(), Failure> {
    gpu.device.poll(wgpu::PollType::Wait { submission_index: Some(submitted), timeout: Some(MOST_WAIT) }).map_err(|_| Failure::Lost)?;
    if gpu.broken() { Err(Failure::Lost) } else { Ok(()) }
}

fn render(gpu: &Gpu, kit: &mut Kit, on_card: &OnCard, view: View, width: u32, height: u32) -> Result<Picture, Failure> {
    let started = Instant::now();
    let (matrix, light) = frame(view, on_card.middle, on_card.radius, width, height);
    let mut bytes = [0u8; FRAME_BYTES as usize];
    put(&mut bytes[0..64], &matrix);
    put(&mut bytes[64..80], &[light[0], light[1], light[2], 0.0]);
    gpu.queue.write_buffer(&kit.frame, 0, &bytes);
    kit.targets(gpu, width, height);
    let targets = kit.targets.as_ref().expect("just made");

    let mut encoder = gpu.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("draw") });
    {
        let (view, resolve_target) = match &targets.many {
            Some(many) => (many, Some(&targets.colour_view)),
            None => (&targets.colour_view, None),
        };
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("model"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view,
                depth_slice: None,
                resolve_target,
                ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT), store: wgpu::StoreOp::Store },
            })],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: &targets.depth,
                depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(1.0), store: wgpu::StoreOp::Discard }),
                stencil_ops: None,
            }),
            ..Default::default()
        });
        pass.set_bind_group(0, &kit.frame_group, &[]);
        for mesh in &on_card.meshes {
            pass.set_pipeline(if mesh.double_sided { &kit.meshes_both_sides } else { &kit.meshes });
            let texture = on_card.texture.as_ref().filter(|_| mesh.textured).unwrap_or(&kit.white);
            pass.set_bind_group(1, texture, &[]);
            pass.set_vertex_buffer(0, mesh.vertices.slice(..));
            pass.set_index_buffer(mesh.indices.slice(..), wgpu::IndexFormat::Uint32);
            pass.draw_indexed(0..mesh.count, 0, 0..1);
        }
        if let Some((points, count)) = &on_card.points {
            pass.set_pipeline(&kit.points);
            pass.set_vertex_buffer(0, points.slice(..));
            pass.draw(0..*count, 0..1);
        }
    }
    wait(gpu, gpu.queue.submit([encoder.finish()]))?;
    let drawing = started.elapsed();

    let started = Instant::now();
    let mut encoder = gpu.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("read back") });
    encoder.copy_texture_to_buffer(
        targets.colour.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &targets.read_back,
            layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(targets.row), rows_per_image: Some(height) },
        },
        wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
    );
    let submitted = gpu.queue.submit([encoder.finish()]);
    let (mapped, is_mapped) = mpsc::channel();
    targets.read_back.slice(..).map_async(wgpu::MapMode::Read, move |result| {
        let _ = mapped.send(result.is_ok());
    });
    wait(gpu, submitted)?;
    if is_mapped.try_recv() != Ok(true) {
        return Err(Failure::Lost);
    }
    let mut bgra = vec![0u8; width as usize * height as usize * 4];
    {
        let view = targets.read_back.slice(..).get_mapped_range().map_err(|_| Failure::Lost)?;
        let threads = if gpu.kind.copies_on_threads() { std::thread::available_parallelism().map_or(4, |n| n.get()).min(8) } else { 1 };
        copy_rows(&view, targets.row as usize, width as usize, &mut bgra, threads);
    }
    targets.read_back.unmap();
    Ok(Picture { width, height, bgra, drawing, reading_back: started.elapsed() })
}

/// Copies the picture's rows out of what the card gave, on `threads` threads, making its
/// colours straight where an edge left them partly covered.
fn copy_rows(from: &[u8], from_row: usize, width: usize, to: &mut [u8], threads: usize) {
    let row = width * 4;
    let rows = to.len() / row;
    let each = rows.div_ceil(threads.max(1)).max(1);
    if threads <= 1 || rows < 2 {
        copy_some(from, from_row, row, to, 0);
        return;
    }
    std::thread::scope(|scope| {
        for (i, part) in to.chunks_mut(each * row).enumerate() {
            scope.spawn(move || copy_some(from, from_row, row, part, i * each));
        }
    });
}

fn copy_some(from: &[u8], from_row: usize, row: usize, to: &mut [u8], first: usize) {
    for (i, line) in to.chunks_exact_mut(row).enumerate() {
        let at = (first + i) * from_row;
        line.copy_from_slice(&from[at..at + row]);
        for pixel in line.as_chunks_mut::<4>().0 {
            if pixel[3] != 0 && pixel[3] != 255 {
                straighten(pixel);
            }
        }
    }
}

/// The colour of a pixel the model partly covers, its samples averaged with the clear around it,
/// divided by how much it covers, in linear colour as they were averaged.
fn straighten(pixel: &mut [u8; 4]) {
    static LINEAR: OnceLock<[f32; 256]> = OnceLock::new();
    let linear = LINEAR.get_or_init(|| {
        std::array::from_fn(|i| {
            let c = i as f32 / 255.0;
            if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
        })
    });
    let cover = pixel[3] as f32 / 255.0;
    for channel in &mut pixel[0..3] {
        let c = (linear[*channel as usize] / cover).min(1.0);
        let encoded = if c <= 0.003_130_8 { c * 12.92 } else { 1.055 * c.powf(1.0 / 2.4) - 0.055 };
        *channel = (encoded * 255.0 + 0.5) as u8;
    }
}
