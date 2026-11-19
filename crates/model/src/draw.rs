//! The drawer: a thread of its own with a graphics device of its own, drawing models off-screen
//! and reading the pictures back. Asking it for anything returns at once; every wait on the
//! graphics card happens on its thread. A model is let go of once it's on the card; a device
//! lost is made again on the next draw, and each model on it must then be uploaded again.

use std::collections::{HashMap, VecDeque};
use std::fmt;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use crate::device::{self, AdapterKind, COLOUR, DEPTH, Gpu};
use crate::maths::{cross, minus, unit};
use crate::view::{MOST_PITCH, sphere};
use crate::{CantDraw, Model, ModelFacts, Picture, View};

/// How a drawer is made.
#[derive(Clone, Debug)]
pub struct DrawerOptions {
    /// Draws only on an adapter of this kind; on the best there is when `None`.
    pub only: Option<AdapterKind>,
    /// How long after failing to make a device it's tried again, at first: each time it fails
    /// again the wait doubles, up to `MOST_RETRY`.
    pub retry_after: Duration,
    /// For tests: finds no adapter at all, as a computer with no graphics would.
    #[cfg(feature = "test-hooks")]
    pub no_adapter: bool,
    /// For tests: makes the device this much later, as a slow one would be.
    #[cfg(feature = "test-hooks")]
    pub device_delay: Duration,
    /// For tests: holds a model in buffers of at most this many bytes, as a device that allows
    /// only small ones would.
    #[cfg(feature = "test-hooks")]
    pub most_buffer: Option<u64>,
}

impl Default for DrawerOptions {
    fn default() -> DrawerOptions {
        DrawerOptions {
            only: None,
            retry_after: Duration::from_secs(2),
            #[cfg(feature = "test-hooks")]
            no_adapter: false,
            #[cfg(feature = "test-hooks")]
            device_delay: Duration::ZERO,
            #[cfg(feature = "test-hooks")]
            most_buffer: None,
        }
    }
}

/// The longest wait before trying again to make a device.
pub const MOST_RETRY: Duration = Duration::from_secs(120);

/// How long the drawer waits for the card to finish a picture before taking the device as lost.
const MOST_WAIT: Duration = Duration::from_secs(20);

/// The most samples a picture is drawn with, all its pixels counted: 4,096 by 4,096 pixels of 4
/// samples each. Beyond it a picture is too large to draw.
const MOST_SAMPLES: u64 = 64 * 1024 * 1024;

static NEXT_ID: AtomicU64 = AtomicU64::new(1);

fn next_id() -> u64 {
    NEXT_ID.fetch_add(1, Ordering::Relaxed)
}

type Ready = Arc<Mutex<Option<Result<AdapterKind, CantDraw>>>>;

fn say(ready: &Ready, now: Result<AdapterKind, CantDraw>) {
    *ready.lock().unwrap_or_else(|e| e.into_inner()) = Some(now);
}

/// Why a drawer gave no picture.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NotDrawn {
    /// It can't be drawn here: no device can be had, or the model or the picture is too large
    /// for the device there is.
    Cant(CantDraw),
    /// The model isn't on this drawer's device: the device was lost and made again, it was made
    /// after the model was uploaded, or the scene was uploaded to another drawer. Read the model
    /// again and upload it to this drawer.
    UploadAgain,
    /// A newer picture of the same scene was asked for before this one was begun.
    Replaced,
    /// The drawer was closed, or its thread has ended.
    Closed,
}

impl From<CantDraw> for NotDrawn {
    fn from(why: CantDraw) -> NotDrawn {
        NotDrawn::Cant(why)
    }
}

impl fmt::Display for NotDrawn {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            NotDrawn::Cant(why) => why.fmt(f),
            NotDrawn::UploadAgain => f.write_str("The model is being read again to be drawn."),
            NotDrawn::Replaced => f.write_str("A newer picture of the model was asked for."),
            NotDrawn::Closed => f.write_str("Models are no longer being drawn."),
        }
    }
}

impl std::error::Error for NotDrawn {}

/// Draws models off-screen, on a thread of its own with a device of its own. One is enough for
/// an app; copies of it share the one thread.
///
/// Call `close` before the program ends. Dropping the last copy ends the thread too, but
/// without waiting for it, and a program that ends while a graphics driver is letting go of a
/// device can crash in the driver.
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
    /// How many devices the thread has made.
    #[cfg(feature = "test-hooks")]
    made: Arc<AtomicU32>,
    /// How many times the thread has tried to make one.
    #[cfg(feature = "test-hooks")]
    tried: Arc<AtomicU32>,
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

/// Where a picture asked for comes. Dropping it before the picture is begun means it isn't
/// drawn. A drawer that ends without answering is said as `Closed`.
pub struct Answer {
    receiver: Receiver<Result<Picture, NotDrawn>>,
    _wanted: Arc<()>,
}

impl Answer {
    /// Waits for the answer: never call this on a window's thread.
    pub fn recv(&self) -> Result<Picture, NotDrawn> {
        self.receiver.recv().unwrap_or(Err(NotDrawn::Closed))
    }

    /// Waits up to `within` for the answer: `None` if it hasn't come.
    pub fn recv_timeout(&self, within: Duration) -> Option<Result<Picture, NotDrawn>> {
        match self.receiver.recv_timeout(within) {
            Ok(answer) => Some(answer),
            Err(mpsc::RecvTimeoutError::Timeout) => None,
            Err(mpsc::RecvTimeoutError::Disconnected) => Some(Err(NotDrawn::Closed)),
        }
    }

    /// The answer if it has come, without waiting.
    pub fn try_recv(&self) -> Option<Result<Picture, NotDrawn>> {
        match self.receiver.try_recv() {
            Ok(answer) => Some(answer),
            Err(mpsc::TryRecvError::Empty) => None,
            Err(mpsc::TryRecvError::Disconnected) => Some(Err(NotDrawn::Closed)),
        }
    }
}

struct Asked {
    scene: u64,
    view: View,
    width: u32,
    height: u32,
    answer: Sender<Result<Picture, NotDrawn>>,
    /// Held by the `Answer` too: alone, the answer is no longer wanted.
    wanted: Arc<()>,
}

impl Asked {
    fn wanted(&self) -> bool {
        Arc::strong_count(&self.wanted) > 1
    }
}

enum Job {
    Upload(u64, Box<Model>),
    Release(u64),
    Draw(Asked),
    #[cfg(feature = "test-hooks")]
    Lose,
    #[cfg(feature = "test-hooks")]
    FailMaking(u32),
    #[cfg(feature = "test-hooks")]
    Refuse,
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
        let made = Arc::new(AtomicU32::new(0));
        let tried = Arc::new(AtomicU32::new(0));
        let (ending, ended) = mpsc::channel();
        let thread = Thread { ready: ready.clone(), made: made.clone(), tried: tried.clone(), _ending: ending };
        let started = std::thread::Builder::new().name("model drawer".into()).spawn(move || run(options, receiver, thread));
        if started.is_err() {
            say(&ready, Err(CantDraw::NoAdapter));
        }
        Drawer {
            shared: Arc::new(Shared {
                id: next_id(),
                jobs,
                ready,
                ended: Mutex::new(ended),
                #[cfg(feature = "test-hooks")]
                made,
                #[cfg(feature = "test-hooks")]
                tried,
            }),
        }
    }

    /// `None` while the device is first being made; then the kind of adapter drawn with, or
    /// `NoAdapter` while none can be had, and once the drawer is closed or its thread has ended.
    pub fn ready(&self) -> Option<Result<AdapterKind, CantDraw>> {
        *self.shared.ready.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Uploads `model` to the drawer's device, on the drawer's thread. The model is let go of
    /// once it's on the card, or at once if there's no device to put it on.
    pub fn upload(&self, model: Model) -> Scene {
        let id = next_id();
        let facts = model.facts;
        let _ = self.shared.jobs.send(Job::Upload(id, Box::new(model)));
        Scene { id, drawer: self.shared.id, facts, jobs: self.shared.jobs.clone() }
    }

    /// Draws `scene` seen from `view` into a picture `width` by `height`, with depth and a
    /// simple light, and reads it back. The answer comes on what this returns; this never waits.
    /// Only the newest picture asked for of a scene is drawn: one asked for earlier and not yet
    /// begun is answered `Replaced`.
    pub fn draw(&self, scene: &Scene, view: View, width: u32, height: u32) -> Answer {
        let (answer, receiver) = mpsc::channel();
        let wanted = Arc::new(());
        let asking = Answer { receiver, _wanted: wanted.clone() };
        if scene.drawer != self.shared.id {
            let _ = answer.send(Err(NotDrawn::UploadAgain));
        } else if let Err(mpsc::SendError(Job::Draw(asked))) =
            self.shared.jobs.send(Job::Draw(Asked { scene: scene.id, view, width, height, answer, wanted }))
        {
            let _ = asked.answer.send(Err(NotDrawn::Closed));
        }
        asking
    }

    /// Ends the drawer's thread, for every copy of this drawer, and waits up to `within` for it to
    /// let go of its device: true if it did. Call this before the program ends (see `Drawer`);
    /// it waits, so never call it on a window's thread while the window is in use.
    pub fn close(self, within: Duration) -> bool {
        let _ = self.shared.jobs.send(Job::Stop);
        let ended = self.shared.ended.lock().unwrap_or_else(|e| e.into_inner());
        matches!(ended.recv_timeout(within), Err(mpsc::RecvTimeoutError::Disconnected))
    }

    /// For tests: loses the device, as a card being reset would. The next draw makes it again.
    #[cfg(feature = "test-hooks")]
    pub fn lose_device(&self) {
        let _ = self.shared.jobs.send(Job::Lose);
    }

    /// For tests: the next `times` tries to make a device fail.
    #[cfg(feature = "test-hooks")]
    pub fn fail_making(&self, times: u32) {
        let _ = self.shared.jobs.send(Job::FailMaking(times));
    }

    /// For tests: how many devices the drawer has made.
    #[cfg(feature = "test-hooks")]
    pub fn devices_made(&self) -> u32 {
        self.shared.made.load(Ordering::Acquire)
    }

    /// For tests: how many times the drawer has tried to make a device.
    #[cfg(feature = "test-hooks")]
    pub fn devices_tried(&self) -> u32 {
        self.shared.tried.load(Ordering::Acquire)
    }

    /// For tests: the next picture drawn asks the device for something it refuses, as a card
    /// short of memory would.
    #[cfg(feature = "test-hooks")]
    pub fn refuse_next_draw(&self) {
        let _ = self.shared.jobs.send(Job::Refuse);
    }
}

/// What the thread shares with the drawer. Dropped as the thread ends, after its device has
/// been let go of: it then says nothing can be drawn, and that the thread has ended.
struct Thread {
    ready: Ready,
    made: Arc<AtomicU32>,
    tried: Arc<AtomicU32>,
    _ending: Sender<()>,
}

impl Drop for Thread {
    fn drop(&mut self) {
        say(&self.ready, Err(CantDraw::NoAdapter));
    }
}

fn run(options: DrawerOptions, jobs: Receiver<Job>, thread: Thread) {
    #[cfg(feature = "test-hooks")]
    let (no_adapter, delay, most_buffer) = (options.no_adapter, options.device_delay, options.most_buffer);
    #[cfg(not(feature = "test-hooks"))]
    let (no_adapter, delay, most_buffer) = (false, Duration::ZERO, None);
    std::thread::sleep(delay);
    let mut drawing = Drawing {
        only: options.only,
        card: None,
        scenes: HashMap::new(),
        thread: &thread,
        no_adapter,
        most_buffer,
        failing: 0,
        refuse: false,
        first_retry: options.retry_after,
        retry_after: options.retry_after,
        next_try: Instant::now(),
    };
    drawing.make();
    let mut pending = VecDeque::new();
    loop {
        if pending.is_empty() {
            match jobs.recv() {
                Ok(job) => pending.push_back(job),
                Err(_) => break,
            }
        }
        while let Ok(job) = jobs.try_recv() {
            pending.push_back(job);
        }
        let Some(job) = pending.pop_front() else { continue };
        match job {
            Job::Upload(id, model) => drawing.upload(id, *model),
            Job::Release(id) => {
                drawing.scenes.remove(&id);
            }
            Job::Draw(asked) => {
                if !asked.wanted() {
                    continue;
                }
                let newer = pending.iter().any(|job| matches!(job, Job::Draw(later) if later.scene == asked.scene && later.wanted()));
                let answer = if newer { Err(NotDrawn::Replaced) } else { drawing.draw(asked.scene, asked.view, asked.width, asked.height) };
                let _ = asked.answer.send(answer);
            }
            #[cfg(feature = "test-hooks")]
            Job::Lose => {
                if let Some(card) = &drawing.card {
                    card.gpu.lose();
                }
            }
            #[cfg(feature = "test-hooks")]
            Job::FailMaking(times) => drawing.failing = times,
            #[cfg(feature = "test-hooks")]
            Job::Refuse => drawing.refuse = true,
            Job::Stop => break,
        }
    }
    // Every answer still waiting is told the drawer is closed, those not yet taken from the
    // channel too.
    pending.extend(jobs.try_iter());
    for job in pending {
        if let Job::Draw(asked) = job {
            let _ = asked.answer.send(Err(NotDrawn::Closed));
        }
    }
}

/// What the drawer's thread holds.
struct Drawing<'a> {
    only: Option<AdapterKind>,
    /// The device and what's made on it, or `None` while none can be had.
    card: Option<Card>,
    scenes: HashMap<u64, OnDevice>,
    thread: &'a Thread,
    /// For tests: making a device always fails, or fails this many more times.
    no_adapter: bool,
    most_buffer: Option<u64>,
    failing: u32,
    /// For tests: the next picture drawn asks the device for something it refuses.
    refuse: bool,
    /// When to try again to make a device, and how long to wait after that if it fails again.
    first_retry: Duration,
    retry_after: Duration,
    next_try: Instant,
}

/// Where a scene's model is.
enum OnDevice {
    On(OnCard),
    /// On no device: it must be uploaded again.
    Gone,
    /// It can't be drawn on this device, and isn't tried again.
    Cant(CantDraw),
}

struct Card {
    gpu: Gpu,
    kit: Kit,
}

/// Why a picture wasn't drawn: the device was lost, or what was asked can't be drawn on it.
enum Failure {
    Lost,
    Cant(CantDraw),
}

impl Drawing<'_> {
    /// Tries to make a device, saying whether one can draw.
    fn make(&mut self) {
        self.card = None;
        self.thread.tried.fetch_add(1, Ordering::AcqRel);
        let fail = self.no_adapter || self.failing > 0;
        self.failing = self.failing.saturating_sub(1);
        if !fail {
            let only = self.only;
            // An adapter whose device can't make the pipelines is passed over for the next.
            let kit = |gpu: &Gpu| catch_unwind(AssertUnwindSafe(|| scoped(gpu, "making the pipelines", || Ok(Kit::new(gpu))))).ok()?.ok();
            if let Ok(Ok((mut gpu, kit))) = catch_unwind(AssertUnwindSafe(|| device::make(only, kit))) {
                if let Some(most) = self.most_buffer {
                    gpu.most_buffer = gpu.most_buffer.min(most);
                }
                self.thread.made.fetch_add(1, Ordering::AcqRel);
                self.card = Some(Card { gpu, kit });
            }
        }
        match &self.card {
            Some(card) => {
                self.retry_after = self.first_retry;
                say(&self.thread.ready, Ok(card.gpu.kind));
            }
            None => {
                self.next_try = Instant::now() + self.retry_after;
                self.retry_after = (self.retry_after * 2).min(MOST_RETRY);
                say(&self.thread.ready, Err(CantDraw::NoAdapter));
            }
        }
    }

    /// Makes the device again if it was lost, or if none could be made and it's time to try
    /// again. A model on a device lost is gone with it.
    fn mend(&mut self) {
        let lost = self.card.as_ref().is_some_and(|card| card.gpu.lost());
        if lost {
            self.card = None;
            for on in self.scenes.values_mut() {
                if matches!(on, OnDevice::On(_)) {
                    *on = OnDevice::Gone;
                }
            }
            self.make();
        } else if self.card.is_none() && Instant::now() >= self.next_try {
            self.make();
        }
    }

    fn upload(&mut self, id: u64, model: Model) {
        self.mend();
        let on = match &self.card {
            Some(card) => match catch_unwind(AssertUnwindSafe(|| scoped(&card.gpu, "uploading a model", || upload(card, &model)))) {
                Ok(Ok(on_card)) => OnDevice::On(on_card),
                Ok(Err(Failure::Cant(why))) => OnDevice::Cant(why),
                Ok(Err(Failure::Lost)) | Err(_) => {
                    card.gpu.set_lost();
                    OnDevice::Gone
                }
            },
            None => OnDevice::Gone,
        };
        self.scenes.insert(id, on);
    }

    fn draw(&mut self, scene: u64, view: View, width: u32, height: u32) -> Result<Picture, NotDrawn> {
        self.mend();
        let Some(card) = &mut self.card else { return Err(CantDraw::NoAdapter.into()) };
        let on_card = match self.scenes.get(&scene) {
            Some(OnDevice::On(on_card)) => on_card,
            Some(OnDevice::Cant(why)) => return Err((*why).into()),
            Some(OnDevice::Gone) | None => return Err(NotDrawn::UploadAgain),
        };
        let most = card.gpu.limits.max_texture_dimension_2d;
        let row = (u64::from(width) * 4).next_multiple_of(u64::from(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT));
        let pixels = u64::from(width) * u64::from(height);
        if width > most || height > most || pixels > MOST_SAMPLES || row * u64::from(height) > card.gpu.limits.max_buffer_size {
            return Err(CantDraw::TooLarge.into());
        }
        // Edges are smoothed with several samples a pixel where that isn't too many.
        let samples = if pixels * u64::from(card.gpu.samples) <= MOST_SAMPLES { card.gpu.samples } else { 1 };
        if width == 0 || height == 0 {
            return Ok(Picture { width, height, bgra: Vec::new(), drawing: Duration::ZERO, reading_back: Duration::ZERO });
        }
        let Card { gpu, kit } = card;
        let refuse = std::mem::take(&mut self.refuse);
        let drawn = catch_unwind(AssertUnwindSafe(|| {
            scoped(gpu, "drawing a picture", || {
                if refuse {
                    // Writing past the end of a buffer, which the device refuses.
                    gpu.queue.write_buffer(&kit.frame, FRAME_BYTES, &[0; 4]);
                }
                render(gpu, kit, on_card, view, width, height, samples)
            })
        }));
        match drawn {
            Ok(Ok(picture)) => Ok(picture),
            Ok(Err(Failure::Cant(why))) => {
                // What it was drawn into may be what was refused.
                kit.targets.clear();
                Err(why.into())
            }
            Ok(Err(Failure::Lost)) | Err(_) => {
                gpu.set_lost();
                // Made again now, so that the model can be uploaded again at once.
                self.mend();
                Err(NotDrawn::UploadAgain)
            }
        }
    }
}

/// Does `work`, an error the device reports about it said as the card refusing it, and the
/// device found lost as that. `what` names the work in the line written about an error.
fn scoped<T>(gpu: &Gpu, what: &str, work: impl FnOnce() -> Result<T, Failure>) -> Result<T, Failure> {
    let validation = gpu.device.push_error_scope(wgpu::ErrorFilter::Validation);
    let memory = gpu.device.push_error_scope(wgpu::ErrorFilter::OutOfMemory);
    let internal = gpu.device.push_error_scope(wgpu::ErrorFilter::Internal);
    let result = work();
    let errors = [internal.pop(), memory.pop(), validation.pop()].map(pollster::block_on);
    if gpu.lost() {
        return Err(Failure::Lost);
    }
    if let Some(error) = errors.iter().flatten().next() {
        eprintln!("The graphics card refused {what}: {error}");
        return Err(Failure::Cant(CantDraw::CardRefused));
    }
    result
}

/// The pipelines and pictures a device draws with.
struct Kit {
    /// For as many samples a pixel as the device draws with, and for one where that's more.
    pipelines: Vec<(u32, Pipelines)>,
    texture_layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    frame: wgpu::Buffer,
    frame_group: wgpu::BindGroup,
    /// Coloured from for a mesh with no texture.
    white: wgpu::BindGroup,
    /// For the last two sizes drawn, the newest first: a picture in the editor and one in the
    /// show needn't make each other's again.
    targets: Vec<Targets>,
}

struct Pipelines {
    meshes: wgpu::RenderPipeline,
    meshes_both_sides: wgpu::RenderPipeline,
    points: wgpu::RenderPipeline,
}

/// What a picture of one size is drawn into and read back through.
struct Targets {
    width: u32,
    height: u32,
    samples: u32,
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
        let pipeline = |label, layout, shader, stride, attributes: &[wgpu::VertexAttribute], topology, cull_mode, samples| {
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
                multisample: wgpu::MultisampleState { count: samples, ..Default::default() },
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
        let set = |samples| Pipelines {
            meshes: pipeline("meshes", &mesh_layout, &mesh_shader, MESH_STRIDE, &mesh_attributes, triangles, Some(wgpu::Face::Back), samples),
            meshes_both_sides: pipeline("meshes seen from behind too", &mesh_layout, &mesh_shader, MESH_STRIDE, &mesh_attributes, triangles, None, samples),
            points: pipeline("points", &points_layout, &points_shader, POINT_STRIDE, &point_attributes, wgpu::PrimitiveTopology::PointList, None, samples),
        };
        let mut pipelines = vec![(gpu.samples, set(gpu.samples))];
        if gpu.samples > 1 {
            pipelines.push((1, set(1)));
        }
        Kit {
            pipelines,
            texture_layout,
            sampler,
            frame,
            frame_group,
            white,
            targets: Vec::new(),
        }
    }

    fn targets(&mut self, gpu: &Gpu, width: u32, height: u32, samples: u32) -> &Targets {
        if let Some(at) = self.targets.iter().position(|t| (t.width, t.height, t.samples) == (width, height, samples)) {
            let targets = self.targets.remove(at);
            self.targets.insert(0, targets);
        } else {
            self.targets.truncate(1);
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
            let many = (samples > 1).then(|| texture("drawn", COLOUR, samples, drawn).create_view(&Default::default()));
            let colour = texture("picture", COLOUR, 1, drawn | wgpu::TextureUsages::COPY_SRC);
            let colour_view = colour.create_view(&Default::default());
            let depth = texture("depth", DEPTH, samples, drawn).create_view(&Default::default());
            // Within the device's largest texture, so the multiply can't overflow.
            let row = (width * 4).next_multiple_of(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT);
            let read_back = gpu.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("read back"),
                size: u64::from(row) * u64::from(height),
                usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            self.targets.insert(0, Targets { width, height, samples, many, colour, colour_view, depth, read_back, row });
        }
        &self.targets[0]
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
    /// A cloud's points, in as many buffers as the device needs, each with how many it holds.
    points: Vec<(wgpu::Buffer, u32)>,
    texture: Option<wgpu::BindGroup>,
    /// The middle of the model and the radius of a sphere round it, so that nothing of it is cut
    /// off by being too near or too far.
    middle: [f32; 3],
    radius: f32,
}

/// A mesh, or as much of one as fits a buffer.
struct MeshOnCard {
    vertices: wgpu::Buffer,
    /// Its triangles, in as many buffers as the device needs, each with how many indices it holds.
    indices: Vec<(wgpu::Buffer, u32)>,
    textured: bool,
    double_sided: bool,
}

/// A buffer of `count` items of `stride` bytes, each written by `fill`, put straight into the
/// device's memory a piece at a time rather than made whole first.
fn filled(gpu: &Gpu, label: &str, count: usize, stride: usize, usage: wgpu::BufferUsages, fill: impl Fn(usize, &mut [u8])) -> Result<wgpu::Buffer, Failure> {
    let size = count as u64 * stride as u64;
    if size > gpu.most_buffer || size == 0 {
        return Err(Failure::Cant(CantDraw::CardRefused));
    }
    let buffer = gpu.device.create_buffer(&wgpu::BufferDescriptor { label: Some(label), size, usage, mapped_at_creation: true });
    {
        let mut mapped = buffer.slice(..).get_mapped_range_mut().map_err(|_| Failure::Cant(CantDraw::CardRefused))?;
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

/// The triangles of `indices`, into `vertices` positions, in parts that each use at most `most`
/// positions and `most` indices: each part's positions, as places in the whole, and its
/// triangles, as places in those. A triangle naming a position past the end is left out.
fn split(indices: &[u32], vertices: usize, most: usize) -> Vec<(Vec<u32>, Vec<u32>)> {
    let most = most.max(3);
    let unused = u32::MAX;
    let mut local = vec![unused; vertices];
    let mut parts = Vec::new();
    let mut part: (Vec<u32>, Vec<u32>) = (Vec::new(), Vec::new());
    for triangle in indices.as_chunks::<3>().0 {
        if triangle.iter().any(|&v| v as usize >= vertices) {
            continue;
        }
        let new = triangle.iter().filter(|&&v| local[v as usize] == unused).count();
        if part.0.len() + new > most || part.1.len() + 3 > most {
            for &v in &part.0 {
                local[v as usize] = unused;
            }
            parts.push(std::mem::take(&mut part));
        }
        for &v in triangle {
            if local[v as usize] == unused {
                local[v as usize] = part.0.len() as u32;
                part.0.push(v);
            }
            part.1.push(local[v as usize]);
        }
    }
    if !part.1.is_empty() {
        parts.push(part);
    }
    parts
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
    let most_vertices = (gpu.most_buffer / MESH_STRIDE) as usize;
    let most_indices = ((gpu.most_buffer / 4) as usize / 3 * 3).max(3);
    let index_buffer = |indices: &[u32]| {
        let buffer = filled(gpu, "mesh indices", indices.len(), 4, wgpu::BufferUsages::INDEX, |i, bytes| {
            bytes.copy_from_slice(&indices[i].to_ne_bytes());
        })?;
        Ok::<_, Failure>((buffer, indices.len() as u32))
    };
    let mut meshes = Vec::new();
    for mesh in &model.meshes {
        if mesh.indices.len() < 3 || mesh.positions.is_empty() {
            continue;
        }
        let corner = |i: usize, corner: &mut [u8]| {
            put(&mut corner[0..12], &mesh.positions[i]);
            put(&mut corner[12..24], mesh.normals.get(i).unwrap_or(&[0.0, 0.0, 1.0]));
            put(&mut corner[24..32], mesh.texture_coords.get(i).unwrap_or(&[0.0, 0.0]));
            put(&mut corner[32..48], mesh.colours.get(i).unwrap_or(&[1.0; 4]));
        };
        let on = |vertices, indices| MeshOnCard { vertices, indices, textured: mesh.textured, double_sided: mesh.double_sided };
        if mesh.positions.len() <= most_vertices {
            let vertices = filled(gpu, "mesh", mesh.positions.len(), MESH_STRIDE as usize, wgpu::BufferUsages::VERTEX, corner)?;
            let indices = mesh.indices.chunks(most_indices).map(index_buffer).collect::<Result<_, _>>()?;
            meshes.push(on(vertices, indices));
        } else {
            for (whole, local) in split(&mesh.indices, mesh.positions.len(), most_vertices.min(most_indices)) {
                let vertices = filled(gpu, "mesh", whole.len(), MESH_STRIDE as usize, wgpu::BufferUsages::VERTEX, |i, to| corner(whole[i] as usize, to))?;
                meshes.push(on(vertices, vec![index_buffer(&local)?]));
            }
        }
    }
    let mut points = Vec::new();
    if let Some(cloud) = &model.points {
        let each = (gpu.most_buffer / POINT_STRIDE).max(1) as usize;
        for first in (0..cloud.positions.len()).step_by(each) {
            let count = each.min(cloud.positions.len() - first);
            let buffer = filled(gpu, "points", count, POINT_STRIDE as usize, wgpu::BufferUsages::VERTEX, |i, point| {
                put(&mut point[0..12], &cloud.positions[first + i]);
                point[12..16].copy_from_slice(cloud.colours.get(first + i).unwrap_or(&[255; 4]));
            })?;
            points.push((buffer, count as u32));
        }
    }
    let texture = match &model.texture {
        Some(texture) if meshes.iter().any(|mesh| mesh.textured) => {
            let most = gpu.limits.max_texture_dimension_2d;
            if texture.width > most || texture.height > most {
                return Err(Failure::Cant(CantDraw::CardRefused));
            }
            if texture.width == 0 || texture.height == 0 || texture.rgba.len() as u64 != u64::from(texture.width) * u64::from(texture.height) * 4 {
                return Err(Failure::Cant(CantDraw::Damaged));
            }
            Some(texture_group(gpu, &card.kit.texture_layout, &card.kit.sampler, texture.width, texture.height, &texture.rgba))
        }
        _ => None,
    };
    let (middle, radius) = sphere(model.facts.bounds);
    Ok(OnCard { meshes, points, texture, middle, radius })
}

type Matrix = [f32; 16];

fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

/// The view and projection, by columns, and where the light comes from in the model's space.
fn frame(view: View, middle: [f32; 3], radius: f32, width: u32, height: u32) -> Option<(Matrix, [f32; 3])> {
    let view = View { pitch: view.pitch.clamp(-MOST_PITCH, MOST_PITCH), ..view.cleaned() };
    let (target, distance) = (view.target, view.distance);
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
    // Always far enough apart to tell depths in between, whatever the sphere's size.
    let far = (reach + radius * 1.05).max(distance * 1e-3).clamp(1e-6, 1e30);
    let near = (reach - radius * 1.05).clamp(far * 1e-4, far * 0.5);
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
    // A view too far out to work out leaves nothing to draw.
    (both.iter().chain(&light).all(|v| v.is_finite())).then_some((both, light))
}

/// Waits for the card to finish what was submitted: a card that doesn't within `MOST_WAIT` is
/// taken as lost.
fn wait(gpu: &Gpu, submitted: wgpu::SubmissionIndex) -> Result<(), Failure> {
    if gpu.device.poll(wgpu::PollType::Wait { submission_index: Some(submitted), timeout: Some(MOST_WAIT) }).is_err() {
        gpu.set_lost();
    }
    if gpu.lost() { Err(Failure::Lost) } else { Ok(()) }
}

fn render(gpu: &Gpu, kit: &mut Kit, on_card: &OnCard, view: View, width: u32, height: u32, samples: u32) -> Result<Picture, Failure> {
    let started = Instant::now();
    let (matrix, light) = frame(view, on_card.middle, on_card.radius, width, height).ok_or(Failure::Cant(CantDraw::TooLarge))?;
    let mut bytes = [0u8; FRAME_BYTES as usize];
    put(&mut bytes[0..64], &matrix);
    put(&mut bytes[64..80], &[light[0], light[1], light[2], 0.0]);
    gpu.queue.write_buffer(&kit.frame, 0, &bytes);
    kit.targets(gpu, width, height, samples);
    let targets = &kit.targets[0];
    let pipelines = &kit.pipelines.iter().find(|(count, _)| *count == samples).ok_or(Failure::Cant(CantDraw::CardRefused))?.1;

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
            pass.set_pipeline(if mesh.double_sided { &pipelines.meshes_both_sides } else { &pipelines.meshes });
            let texture = on_card.texture.as_ref().filter(|_| mesh.textured).unwrap_or(&kit.white);
            pass.set_bind_group(1, texture, &[]);
            pass.set_vertex_buffer(0, mesh.vertices.slice(..));
            for (indices, count) in &mesh.indices {
                pass.set_index_buffer(indices.slice(..), wgpu::IndexFormat::Uint32);
                pass.draw_indexed(0..*count, 0, 0..1);
            }
        }
        if !on_card.points.is_empty() {
            pass.set_pipeline(&pipelines.points);
        }
        for (points, count) in &on_card.points {
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
        return Err(Failure::Cant(CantDraw::CardRefused));
    }
    let mut bgra = vec![0u8; width as usize * height as usize * 4];
    {
        let view = targets.read_back.slice(..).get_mapped_range().map_err(|_| Failure::Cant(CantDraw::CardRefused))?;
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
