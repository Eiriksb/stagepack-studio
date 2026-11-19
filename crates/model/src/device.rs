//! Making a graphics device of the drawer's own: any adapter there is, a discrete card first,
//! then an integrated one, then a software one. Everything here waits, so it's only ever done
//! on the drawer's thread.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::CantDraw;

/// What kind of graphics adapter a drawer draws with.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum AdapterKind {
    /// A card of its own, with its own memory.
    Discrete,
    /// Part of the computer's processor, sharing its memory.
    Integrated,
    /// Drawing done by the processor itself.
    Software,
    /// One that doesn't say, or a virtual one.
    Other,
}

impl AdapterKind {
    fn of(kind: wgpu::DeviceType) -> AdapterKind {
        match kind {
            wgpu::DeviceType::DiscreteGpu => AdapterKind::Discrete,
            wgpu::DeviceType::IntegratedGpu => AdapterKind::Integrated,
            wgpu::DeviceType::Cpu => AdapterKind::Software,
            wgpu::DeviceType::Other | wgpu::DeviceType::VirtualGpu => AdapterKind::Other,
        }
    }

    /// Which is tried first: the lowest.
    fn rank(self) -> u8 {
        match self {
            AdapterKind::Discrete => 0,
            AdapterKind::Integrated => 1,
            AdapterKind::Other => 2,
            AdapterKind::Software => 3,
        }
    }

    /// Whether a picture read back from it is slow enough to copy on several threads: the
    /// spike found an integrated card's read-back the slowest part of drawing.
    pub(crate) fn copies_on_threads(self) -> bool {
        matches!(self, AdapterKind::Integrated | AdapterKind::Software)
    }
}

/// The picture drawn into, sRGB so the card blends and averages in linear colour.
pub(crate) const COLOUR: wgpu::TextureFormat = wgpu::TextureFormat::Bgra8UnormSrgb;
pub(crate) const DEPTH: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;

/// A device made, and what it can do.
pub(crate) struct Gpu {
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    pub kind: AdapterKind,
    pub limits: wgpu::Limits,
    /// The largest buffer a model is held in.
    pub most_buffer: u64,
    /// How many samples a pixel is drawn with: 4 where the adapter can, smoothing edges.
    pub samples: u32,
    /// Set when the device is lost: it's made again before the next draw. An error the device
    /// reports about what was asked of it doesn't set it.
    lost: Arc<AtomicBool>,
}

impl Gpu {
    pub fn lost(&self) -> bool {
        self.lost.load(Ordering::Acquire)
    }

    /// Takes the device as lost: it's made again before the next draw.
    pub fn set_lost(&self) {
        self.lost.store(true, Ordering::Release);
    }

    /// Loses the device, as a card being reset or its driver updated would.
    #[cfg(feature = "test-hooks")]
    pub fn lose(&self) {
        self.set_lost();
        self.device.destroy();
    }
}

/// The graphics libraries made for the card.
const NATIVE: wgpu::Backends = wgpu::Backends::VULKAN.union(wgpu::Backends::DX12).union(wgpu::Backends::METAL);

/// GL, looked at only where the libraries made for the card find no card: opening it at all
/// can cost a driver's own threads and a wait as the program ends.
const FALLBACK: wgpu::Backends = wgpu::Backends::GL;

fn instance(backends: wgpu::Backends) -> wgpu::Instance {
    let mut description = wgpu::InstanceDescriptor::new_without_display_handle();
    description.backends = backends;
    // No checking layers, even in a build for tests: they're slow where they're found.
    description.flags = wgpu::InstanceFlags::empty();
    wgpu::Instance::new(description)
}

/// Whether a picture can be drawn on `adapter` and copied back from it.
fn can_draw(adapter: &wgpu::Adapter) -> bool {
    let colour = adapter.get_texture_format_features(COLOUR).allowed_usages;
    let depth = adapter.get_texture_format_features(DEPTH).allowed_usages;
    colour.contains(wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC) && depth.contains(wgpu::TextureUsages::RENDER_ATTACHMENT)
}

/// The adapters that can draw, best first: by kind, a discrete card, then an integrated one,
/// then a software one, and a library made for the card before GL. GL is looked at only when the
/// libraries made for the card find nothing but software, so a card only GL reaches is still
/// used. Each adapter comes with its instance, which must outlive it.
fn adapters() -> Vec<(wgpu::Instance, wgpu::Adapter)> {
    let mut found = Vec::new();
    for backends in [NATIVE, FALLBACK] {
        let instance = instance(backends);
        for adapter in pollster::block_on(instance.enumerate_adapters(backends)) {
            if can_draw(&adapter) {
                found.push((instance.clone(), adapter));
            }
        }
        let hardware = found.iter().any(|(_, adapter)| AdapterKind::of(adapter.get_info().device_type) != AdapterKind::Software);
        if hardware {
            break;
        }
    }
    found.sort_by_key(|(_, adapter)| {
        let info = adapter.get_info();
        (AdapterKind::of(info.device_type).rank(), info.backend == wgpu::Backend::Gl)
    });
    found
}

/// The kinds of adapter this computer has that can draw, best first. This waits for the
/// graphics library, a second or more: never call it on a window's thread.
pub fn adapter_kinds() -> Vec<AdapterKind> {
    let mut kinds = Vec::new();
    for (_, adapter) in adapters() {
        let kind = AdapterKind::of(adapter.get_info().device_type);
        if !kinds.contains(&kind) {
            kinds.push(kind);
        }
    }
    kinds
}

/// A device on the best adapter there is, or on the best of kind `only`.
pub(crate) fn make(only: Option<AdapterKind>) -> Result<Gpu, CantDraw> {
    for (_instance, adapter) in adapters() {
        let kind = AdapterKind::of(adapter.get_info().device_type);
        if only.is_some_and(|only| only != kind) {
            continue;
        }
        let limits = adapter.limits();
        let description = wgpu::DeviceDescriptor {
            label: Some("slides-model drawer"),
            required_limits: limits.clone(),
            memory_hints: wgpu::MemoryHints::MemoryUsage,
            ..Default::default()
        };
        let Ok((device, queue)) = pollster::block_on(adapter.request_device(&description)) else { continue };
        let lost = Arc::new(AtomicBool::new(false));
        let theirs = lost.clone();
        device.set_device_lost_callback(move |_, _| theirs.store(true, Ordering::Release));
        // Errors are caught where the work is asked for; one that gets past is let go, rather
        // than ending the program as the library would.
        device.on_uncaptured_error(Arc::new(|_| {}));
        let four = |format| adapter.get_texture_format_features(format).flags.sample_count_supported(4);
        let samples = if four(COLOUR) && four(DEPTH) { 4 } else { 1 };
        let most_buffer = limits.max_buffer_size;
        return Ok(Gpu { device, queue, kind, limits, samples, lost, most_buffer });
    }
    Err(CantDraw::NoAdapter)
}
