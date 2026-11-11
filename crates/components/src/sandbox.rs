//! Running a talk's compiled components, held to their limits. A talk is untrusted, so
//! everything a module hands back is checked before the app uses it.

use std::rc::Rc;
use std::sync::OnceLock;
use std::time::Duration;

use toolkit::{asset_path_allowed, ComponentSchema, Element, PropKind, RenderInput, RenderOutput, ABI_VERSION};
use wasmtime::{
    Caller, Config, Engine, ExternType, Instance, Linker, Memory, Module, ResourceLimiter, Store, Trap, TypedFunc,
};

pub const MEMORY_LIMIT_BYTES: usize = 64 * 1024 * 1024;
pub const RENDER_TIME_LIMIT: Duration = Duration::from_millis(250);
pub const MAX_OUTPUT_BYTES: usize = 4 * 1024 * 1024;
pub const MAX_MODULE_BYTES: usize = 4 * 1024 * 1024;
pub const MAX_ELEMENTS: usize = 20_000;
/// Below the 64 the design names, because the JSON reader stops at about 42 nested stacks.
pub const MAX_ELEMENT_DEPTH: usize = 40;
/// The most text one answer can hold, in its text, messages, icon names, picture paths
/// and path data together. Laying out text costs far more than reading it.
pub const MAX_TEXT_BYTES: usize = 256 * 1024;
/// The most bytes in each of a schema's names and choices.
pub const MAX_SCHEMA_STRING_BYTES: usize = 64;
/// The most props a schema can list.
pub const MAX_PROPS: usize = 64;

/// How often the clock that stops a component ticks.
const TICK: Duration = Duration::from_millis(10);
const MAX_ASSET_READS: u32 = 64;
/// More than any component needs: each element takes a pointer's worth of memory.
const MAX_TABLE_ELEMENTS: usize = 100_000;
const READ_ASSET: (&str, &str) = ("toolkit", "read_asset");
const EXPORTS: [&str; 4] = ["abi_version", "alloc", "schema", "render"];

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum ComponentError {
    #[error("too large to run")]
    TooLarge,
    #[error("can't be loaded: {0}")]
    Malformed(String),
    #[error("built by a newer version of this app (interface {0})")]
    NewerVersion(u32),
    #[error("took too long")]
    TookTooLong,
    #[error("used too much memory")]
    OutOfMemory,
    #[error("stopped working")]
    Crashed,
    #[error("gave an answer that can't be read: {0}")]
    BadOutput(String),
}

impl ComponentError {
    /// The words for the labelled box that takes the component's place.
    pub fn short(&self) -> &'static str {
        match self {
            ComponentError::TooLarge => "too large to run",
            ComponentError::Malformed(_) => "can't be loaded",
            ComponentError::NewerVersion(_) => "built by a newer version of this app",
            ComponentError::TookTooLong => "took too long",
            ComponentError::OutOfMemory => "used too much memory",
            ComponentError::Crashed => "stopped working",
            ComponentError::BadOutput(_) => "gave an answer that can't be read",
        }
    }
}

/// The bytes of a file in the talk's `assets` folder, by its talk-relative path.
pub type Assets = Rc<dyn Fn(&str) -> Option<Vec<u8>>>;

/// A loaded component. Each render starts a fresh instance, so nothing survives between them.
pub struct WasmComponent {
    module: Module,
    schema: ComponentSchema,
}

impl WasmComponent {
    pub fn load(bytes: &[u8]) -> Result<WasmComponent, ComponentError> {
        if bytes.len() > MAX_MODULE_BYTES {
            return Err(ComponentError::TooLarge);
        }
        let module = Module::new(engine(), bytes).map_err(|error| ComponentError::Malformed(error.to_string()))?;
        check_shape(&module)?;

        // These are the talk's code too, so they run under the same limits as a render.
        let mut running = Running::start(&module, Rc::new(|_: &str| None))?;
        running.export::<u32, u32>("alloc")?;
        running.export::<(u32, u32), u64>("render")?;
        let version = running.export::<(), u32>("abi_version")?;
        let version = running.call(&version, ())?;
        if version > ABI_VERSION {
            return Err(ComponentError::NewerVersion(version));
        }
        if version == 0 {
            return Err(ComponentError::Malformed("it names no interface version".into()));
        }
        let schema = running.export::<(), u64>("schema")?;
        let packed = running.call(&schema, ())?;
        let schema = serde_json::from_slice(&running.answer(packed)?).map_err(|error| ComponentError::BadOutput(error.to_string()))?;
        check_schema(&schema)?;
        Ok(WasmComponent { module, schema })
    }

    pub fn schema(&self) -> &ComponentSchema {
        &self.schema
    }

    pub fn render(&self, input: &RenderInput, assets: Assets) -> Result<RenderOutput, ComponentError> {
        let input = serde_json::to_vec(input).expect("render input is always JSON");
        let len = u32::try_from(input.len()).map_err(|_| ComponentError::TooLarge)?;
        let mut running = Running::start(&self.module, assets)?;
        let alloc = running.export::<u32, u32>("alloc")?;
        let render = running.export::<(u32, u32), u64>("render")?;
        let at = running.call(&alloc, len)?;
        running
            .memory
            .write(&mut running.store, at as usize, &input)
            .map_err(|_| ComponentError::BadOutput("it gave room for its input outside its memory".into()))?;
        let packed = running.call(&render, (at, len))?;
        let answer = running.answer(packed)?;
        let mut output: RenderOutput = serde_json::from_slice(&answer).map_err(|error| ComponentError::BadOutput(error.to_string()))?;

        Measure::default().element(&output.element, 1)?;
        keep_pictures_in_assets(&mut output.element);
        Ok(output)
    }
}

/// One engine for the process, with one clock that every running component is timed by.
fn engine() -> &'static Engine {
    static ENGINE: OnceLock<Engine> = OnceLock::new();
    ENGINE.get_or_init(|| {
        let mut config = Config::new();
        config.epoch_interruption(true).max_wasm_stack(256 * 1024);
        let engine = Engine::new(&config).expect("this configuration is always valid");
        let clock = engine.clone();
        std::thread::Builder::new()
            .name("component clock".into())
            .spawn(move || {
                loop {
                    std::thread::sleep(TICK);
                    clock.increment_epoch();
                }
            })
            .expect("the component clock couldn't be started");
        engine
    })
}

/// Checks what the module asks for and offers before anything of it runs.
fn check_shape(module: &Module) -> Result<(), ComponentError> {
    let malformed = |message: String| Err(ComponentError::Malformed(message));
    for import in module.imports() {
        if (import.module(), import.name()) != READ_ASSET || !matches!(import.ty(), ExternType::Func(_)) {
            return malformed(format!("it asks for {}.{}, which components can't have", import.module(), import.name()));
        }
    }
    for name in EXPORTS {
        if !matches!(module.get_export(name), Some(ExternType::Func(_))) {
            return malformed(format!("it has no `{name}` function"));
        }
    }
    match module.get_export("memory") {
        Some(ExternType::Memory(memory)) if !memory.is_64() && !memory.is_shared() => Ok(()),
        Some(ExternType::Memory(_)) => malformed("its memory isn't of the kind components have".into()),
        _ => malformed("it has no `memory`".into()),
    }
}

/// Holds a component to its memory, and remembers when it asked for more than it may have.
struct Limits {
    refused_growth: bool,
}

impl ResourceLimiter for Limits {
    fn memory_growing(&mut self, _current: usize, desired: usize, _maximum: Option<usize>) -> wasmtime::Result<bool> {
        let allowed = desired <= MEMORY_LIMIT_BYTES;
        self.refused_growth |= !allowed;
        Ok(allowed)
    }

    fn table_growing(&mut self, _current: usize, desired: usize, _maximum: Option<usize>) -> wasmtime::Result<bool> {
        let allowed = desired <= MAX_TABLE_ELEMENTS;
        self.refused_growth |= !allowed;
        Ok(allowed)
    }

    fn instances(&self) -> usize {
        1
    }

    fn tables(&self) -> usize {
        1
    }

    fn memories(&self) -> usize {
        1
    }
}

struct State {
    limits: Limits,
    assets: Assets,
    asset_reads: u32,
}

/// One instance of a component, in a store of its own.
struct Running {
    store: Store<State>,
    instance: Instance,
    memory: Memory,
}

impl Running {
    fn start(module: &Module, assets: Assets) -> Result<Running, ComponentError> {
        let mut store = Store::new(engine(), State { limits: Limits { refused_growth: false }, assets, asset_reads: 0 });
        store.limiter(|state| &mut state.limits);
        // Set before instantiating, as a start function is the component's code too.
        store.set_epoch_deadline((RENDER_TIME_LIMIT.as_millis() / TICK.as_millis()) as u64);
        store.epoch_deadline_trap();

        let mut linker = Linker::new(engine());
        linker
            .func_wrap(READ_ASSET.0, READ_ASSET.1, read_asset)
            .expect("read_asset is defined once");
        let instance = match linker.instantiate(&mut store, module) {
            Ok(instance) => instance,
            Err(error) if error.downcast_ref::<Trap>().is_some() || store.data().limits.refused_growth => {
                return Err(fault(&error, &store));
            }
            Err(error) => return Err(ComponentError::Malformed(error.to_string())),
        };
        let memory = instance.get_memory(&mut store, "memory").ok_or_else(|| ComponentError::Malformed("it has no `memory`".into()))?;
        Ok(Running { store, instance, memory })
    }

    fn export<Params: wasmtime::WasmParams, Results: wasmtime::WasmResults>(
        &mut self,
        name: &str,
    ) -> Result<TypedFunc<Params, Results>, ComponentError> {
        self.instance
            .get_typed_func(&mut self.store, name)
            .map_err(|error| ComponentError::Malformed(format!("`{name}` isn't what components export: {error}")))
    }

    fn call<Params: wasmtime::WasmParams, Results: wasmtime::WasmResults>(
        &mut self,
        function: &TypedFunc<Params, Results>,
        params: Params,
    ) -> Result<Results, ComponentError> {
        function.call(&mut self.store, params).map_err(|error| fault(&error, &self.store))
    }

    /// The bytes at a packed pointer and length, read through the module's memory.
    fn answer(&self, packed: u64) -> Result<Vec<u8>, ComponentError> {
        let (at, len) = unpack(packed);
        if len > MAX_OUTPUT_BYTES {
            return Err(ComponentError::BadOutput(format!("it is {len} bytes, more than the {MAX_OUTPUT_BYTES} allowed")));
        }
        let mut bytes = vec![0; len];
        self.memory
            .read(&self.store, at, &mut bytes)
            .map_err(|_| ComponentError::BadOutput("it points outside the component's memory".into()))?;
        Ok(bytes)
    }
}

fn unpack(packed: u64) -> (usize, usize) {
    ((packed >> 32) as usize, (packed & 0xffff_ffff) as usize)
}

/// What stopped a component, told apart by the clock and by the limiter.
fn fault(error: &wasmtime::Error, store: &Store<State>) -> ComponentError {
    if error.downcast_ref::<Trap>() == Some(&Trap::Interrupt) {
        ComponentError::TookTooLong
    } else if store.data().limits.refused_growth {
        ComponentError::OutOfMemory
    } else {
        ComponentError::Crashed
    }
}

/// `toolkit.read_asset(ptr, len)`: the file at the path, placed through the module's own
/// `alloc`, or 0 if it can't be had. Only a trap in `alloc` stops the component.
fn read_asset(mut caller: Caller<'_, State>, ptr: u32, len: u32) -> wasmtime::Result<u64> {
    let state = caller.data_mut();
    if state.asset_reads >= MAX_ASSET_READS {
        return Ok(0);
    }
    state.asset_reads += 1;
    let len = len as usize;
    if len > MAX_OUTPUT_BYTES {
        return Ok(0);
    }
    let Some(memory) = caller.get_export("memory").and_then(|export| export.into_memory()) else {
        return Ok(0);
    };
    let mut path = vec![0; len];
    if memory.read(&caller, ptr as usize, &mut path).is_err() {
        return Ok(0);
    }
    let Ok(path) = String::from_utf8(path) else {
        return Ok(0);
    };
    if !asset_path_allowed(&path) {
        return Ok(0);
    }
    let assets = caller.data().assets.clone();
    let Some(bytes) = assets(&path) else {
        return Ok(0);
    };
    if bytes.len() > MEMORY_LIMIT_BYTES {
        return Ok(0);
    }
    let Some(alloc) = caller.get_export("alloc").and_then(|export| export.into_func()) else {
        return Ok(0);
    };
    let Ok(alloc) = alloc.typed::<u32, u32>(&caller) else {
        return Ok(0);
    };
    let at = alloc.call(&mut caller, bytes.len() as u32)?;
    if memory.write(&mut caller, at as usize, &bytes).is_err() {
        return Ok(0);
    }
    Ok((u64::from(at) << 32) | bytes.len() as u64)
}

/// Holds a schema's strings and props to what a form can show.
fn check_schema(schema: &ComponentSchema) -> Result<(), ComponentError> {
    let malformed = |message: String| Err(ComponentError::Malformed(message));
    if schema.props.len() > MAX_PROPS {
        return malformed(format!("its schema has more than {MAX_PROPS} props"));
    }
    let choices = schema.props.iter().flat_map(|prop| match &prop.kind {
        PropKind::File(choices) | PropKind::Choice(choices) => choices.as_slice(),
        _ => &[],
    });
    let names = std::iter::once(&schema.name).chain(schema.props.iter().map(|prop| &prop.name)).chain(choices);
    for name in names {
        if name.len() > MAX_SCHEMA_STRING_BYTES {
            return malformed(format!("its schema has a name or choice over {MAX_SCHEMA_STRING_BYTES} bytes"));
        }
    }
    Ok(())
}

/// What an answer has added up to so far, held to the limits on it.
#[derive(Default)]
struct Measure {
    elements: usize,
    text_bytes: usize,
    children: bool,
}

impl Measure {
    /// Counts the elements, how deeply they nest, their text and where the tag's content
    /// goes, and refuses a tree over any limit.
    fn element(&mut self, element: &Element, depth: usize) -> Result<(), ComponentError> {
        self.elements += 1;
        if self.elements > MAX_ELEMENTS {
            return Err(ComponentError::BadOutput(format!("it has more than {MAX_ELEMENTS} elements")));
        }
        if depth > MAX_ELEMENT_DEPTH {
            return Err(ComponentError::BadOutput(format!("it nests more than {MAX_ELEMENT_DEPTH} levels deep")));
        }
        match element {
            Element::Text { text, .. } => self.text(text),
            Element::Problem { message } => self.text(message),
            Element::Icon { name, .. } => self.text(name),
            Element::Picture { src, .. } => self.text(src),
            Element::Path { data, .. } => self.text(data),
            Element::Children if self.children => Err(ComponentError::BadOutput("it places the tag's content more than once".into())),
            Element::Children => {
                self.children = true;
                Ok(())
            }
            Element::Stack { children, .. } => children.iter().try_for_each(|child| self.element(child, depth + 1)),
            Element::Shape { child: Some(child), .. } => self.element(child, depth + 1),
            Element::Canvas { items, .. } => items.iter().try_for_each(|item| self.element(&item.element, depth + 1)),
            _ => Ok(()),
        }
    }

    fn text(&mut self, text: &str) -> Result<(), ComponentError> {
        self.text_bytes += text.len();
        if self.text_bytes > MAX_TEXT_BYTES {
            return Err(ComponentError::BadOutput(format!("it has more than {} KiB of text", MAX_TEXT_BYTES / 1024)));
        }
        Ok(())
    }
}

fn keep_pictures_in_assets(element: &mut Element) {
    match element {
        Element::Picture { src, .. } if !asset_path_allowed(src) => {
            *element = toolkit::problem("A picture must be in the talk's assets folder");
        }
        Element::Stack { children, .. } => children.iter_mut().for_each(keep_pictures_in_assets),
        Element::Shape { child: Some(child), .. } => keep_pictures_in_assets(child),
        Element::Canvas { items, .. } => items.iter_mut().for_each(|item| keep_pictures_in_assets(&mut item.element)),
        _ => {}
    }
}
