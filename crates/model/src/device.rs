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
    /// How many samples a pixel is drawn with: 4 where the adapter can, smoothing edges.
    pub samples: u32,
    /// Set when the device is lost or says something went wrong: it's made again before the
    /// next draw.
    broken: Arc<AtomicBool>,
}

impl Gpu {
    pub fn broken(&self) -> bool {
        self.broken.load(Ordering::Acquire)
    }

    /// Takes the device as lost: it's made again before the next draw.
    pub fn set_broken(&self) {
        self.broken.store(true, Ordering::Release);
    }

    /// Loses the device, as a card being reset or its driver updated would.
    pub fn lose(&self) {
        self.set_broken();
        self.device.destroy();
    }
}

/// The graphics libraries made for the card, tried first.
const NATIVE: wgpu::Backends = wgpu::Backends::VULKAN.union(wgpu::Backends::DX12).union(wgpu::Backends::METAL);

/// GL, tried only where no library made for the card finds an adapter: opening it at all can
/// cost a driver's own threads and a wait as the program ends.
const FALLBACK: wgpu::Backends = wgpu::Backends::GL;

fn instance(backends: wgpu::Backends) -> wgpu::Instance {
    let mut description = wgpu::InstanceDescriptor::new_without_display_handle();
    description.backends = backends;
    // No checking layers, even in a build for tests: they're slow where they're found.
    description.flags = wgpu::InstanceFlags::empty();
    wgpu::Instance::new(description)
}

/// The adapters `instance` finds, best first.
fn adapters(instance: &wgpu::Instance, backends: wgpu::Backends) -> Vec<wgpu::Adapter> {
    let mut adapters = pollster::block_on(instance.enumerate_adapters(backends));
    adapters.sort_by_key(|adapter| AdapterKind::of(adapter.get_info().device_type).rank());
    adapters
}

/// The kinds of adapter this computer has, best first. This waits for the graphics library, a
/// second or more: never call it on a window's thread.
pub fn adapter_kinds() -> Vec<AdapterKind> {
    let mut kinds = Vec::new();
    for backends in [NATIVE, FALLBACK] {
        for adapter in adapters(&instance(backends), backends) {
            let kind = AdapterKind::of(adapter.get_info().device_type);
            if !kinds.contains(&kind) {
                kinds.push(kind);
            }
        }
        if !kinds.is_empty() {
            break;
        }
    }
    kinds
}

/// A device on the best adapter there is, or on the best of kind `only`.
pub(crate) fn make(only: Option<AdapterKind>) -> Result<Gpu, CantDraw> {
    for backends in [NATIVE, FALLBACK] {
        if let Some(gpu) = make_with(backends, only) {
            return Ok(gpu);
        }
    }
    Err(CantDraw::NoAdapter)
}

fn make_with(backends: wgpu::Backends, only: Option<AdapterKind>) -> Option<Gpu> {
    let instance = instance(backends);
    for adapter in adapters(&instance, backends) {
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
        let broken = Arc::new(AtomicBool::new(false));
        let lost = broken.clone();
        device.set_device_lost_callback(move |_, _| lost.store(true, Ordering::Release));
        let failed = broken.clone();
        device.on_uncaptured_error(Arc::new(move |_| failed.store(true, Ordering::Release)));
        let four = |format| adapter.get_texture_format_features(format).flags.sample_count_supported(4);
        let samples = if four(COLOUR) && four(DEPTH) { 4 } else { 1 };
        return Some(Gpu { device, queue, kind, limits, samples, broken });
    }
    None
}
