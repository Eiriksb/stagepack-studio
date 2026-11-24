//! Running an add-on, held to its limits. An add-on is a WebAssembly module with no imports
//! at all: it is given a bundle of files and gives one back, and nothing else. It runs on
//! the components' engine and clock, with limits of its own, and everything it hands back
//! is checked before the app uses it.

use std::collections::BTreeSet;
use std::time::Duration;

use addon_abi::{decode, encode, safe_name, Bundle, Report, ABI_VERSION, ARGS, ERROR, MAX_FILES, OP, REPORT};
use serde::Deserialize;
use wasmtime::{ExternType, FuncType, Instance, Memory, Module, ResourceLimiter, Store, Trap, TypedFunc, ValType};

use crate::sandbox::{engine, unpack, MAX_TABLE_ELEMENTS, TICK};
use crate::shortened;

/// The longest an add-on may run, for loading or for one operation.
pub const ADDON_TIME_LIMIT: Duration = Duration::from_secs(60);
/// The most memory an add-on may have.
pub const ADDON_MEMORY_LIMIT_BYTES: usize = 1024 * 1024 * 1024;
/// The most bytes a bundle may come to, given to an add-on or given back.
pub const MAX_BUNDLE_BYTES: usize = 512 * 1024 * 1024;
/// The most files a bundle may hold, its own names among them.
pub const MAX_BUNDLE_FILES: usize = MAX_FILES;
/// The largest add-on that is loaded.
pub const MAX_ADDON_BYTES: usize = 64 * 1024 * 1024;
/// The most bytes `info` may give back.
pub const MAX_INFO_BYTES: usize = 64 * 1024;
/// The most characters of an add-on's own sentence on why it failed.
pub const MAX_ERROR_CHARS: usize = 500;
/// The most bytes of an add-on's name, its version and each operation's name.
pub const MAX_INFO_STRING_BYTES: usize = 64;
/// The most operations an add-on can name.
pub const MAX_OPERATIONS: usize = 16;

const EXPORTS: [(&str, &[ValType], &[ValType]); 4] = [
    ("addon_abi", &[], &[ValType::I32]),
    ("alloc", &[ValType::I32], &[ValType::I32]),
    ("info", &[], &[ValType::I64]),
    ("run", &[ValType::I32, ValType::I32], &[ValType::I64]),
];

/// Why an add-on couldn't be loaded or couldn't do what it was asked. Each says so in one sentence.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum AddonError {
    /// It isn't an add-on this app can run, or it gave back something that can't be read.
    #[error("The add-on can't be used: {0}.")]
    Malformed(String),
    /// Over one of the limits, which the sentence names.
    #[error("{0}")]
    TooLarge(&'static str),
    #[error("The add-on took longer than a minute, so it was stopped.")]
    TookTooLong,
    #[error("The add-on needed more than 1 GiB of memory, so it was stopped.")]
    OutOfMemory,
    /// The add-on's own sentence, from its `.error`.
    #[error("The add-on couldn't finish: {0}")]
    Failed(String),
    #[error("The add-on stopped working.")]
    Trapped,
}

/// What an add-on says it is, from its `info`.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AddonInfo {
    pub name: String,
    pub version: String,
    pub operations: Vec<String>,
}

/// What one run of an add-on is held to. Only tests run under limits other than the usual.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AddonLimits {
    pub time: Duration,
    pub memory_bytes: usize,
    pub input_bytes: usize,
    pub output_bytes: usize,
    pub files: usize,
}

impl Default for AddonLimits {
    fn default() -> AddonLimits {
        AddonLimits {
            time: ADDON_TIME_LIMIT,
            memory_bytes: ADDON_MEMORY_LIMIT_BYTES,
            input_bytes: MAX_BUNDLE_BYTES,
            output_bytes: MAX_BUNDLE_BYTES,
            files: MAX_BUNDLE_FILES,
        }
    }
}

const INPUT_TOO_LARGE: &str = "The files for the add-on come to more than 512 MiB.";
const OUTPUT_TOO_LARGE: &str = "The add-on gave back more than 512 MiB.";
const TOO_MANY_FILES: &str = "An add-on can be given or give back at most 10 000 files.";
const ADDON_TOO_LARGE: &str = "The add-on is larger than 64 MiB.";

/// A loaded add-on. Each run starts a fresh instance, so nothing survives between them.
pub struct Addon {
    module: Module,
    info: AddonInfo,
}

impl std::fmt::Debug for Addon {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.debug_struct("Addon").field("info", &self.info).finish_non_exhaustive()
    }
}

impl Addon {
    /// Compiles and checks the add-on, and asks it once what it is.
    pub fn load(wasm: &[u8]) -> Result<Addon, AddonError> {
        Addon::load_limited(wasm, &AddonLimits::default())
    }

    /// [`Addon::load`] under limits of the test's own.
    #[cfg(feature = "test-hooks")]
    pub fn load_with_limits(wasm: &[u8], limits: &AddonLimits) -> Result<Addon, AddonError> {
        Addon::load_limited(wasm, limits)
    }

    fn load_limited(wasm: &[u8], limits: &AddonLimits) -> Result<Addon, AddonError> {
        if wasm.len() > MAX_ADDON_BYTES {
            return Err(AddonError::TooLarge(ADDON_TOO_LARGE));
        }
        let module = Module::new(engine(), wasm).map_err(|error| malformed(format!("it isn't WebAssembly this app can run ({})", detail(&error.to_string()))))?;
        check_shape(&module)?;

        // These are the add-on's code too, so they run under the same limits as an operation.
        let mut running = Running::start(&module, limits)?;
        let version = running.export::<(), u32>("addon_abi")?;
        let version = running.call(&version, ())?;
        if version != ABI_VERSION {
            return Err(malformed(format!("it is built for add-on interface {version}, and this app runs interface {ABI_VERSION}")));
        }
        let info = running.export::<(), u64>("info")?;
        let packed = running.call(&info, ())?;
        let (at, len) = unpack(packed);
        if len > MAX_INFO_BYTES {
            return Err(malformed(format!("its info is more than {} KiB", MAX_INFO_BYTES / 1024)));
        }
        let info = running.read(at, len)?;
        let info: AddonInfo = serde_json::from_slice(&info).map_err(|error| malformed(format!("its info can't be read ({})", detail(&error.to_string()))))?;
        check_info(&info)?;
        Ok(Addon { module, info })
    }

    pub fn info(&self) -> &AddonInfo {
        &self.info
    }

    /// Does `op` with `args_json` on `files`, and gives back the files and the report. Every
    /// name in `files` must pass [`safe_name`].
    pub fn run(&self, op: &str, args_json: &str, files: Bundle) -> Result<(Bundle, Report), AddonError> {
        self.run_limited(op, args_json, files, &AddonLimits::default())
    }

    /// [`Addon::run`] under limits of the test's own.
    #[cfg(feature = "test-hooks")]
    pub fn run_with_limits(&self, op: &str, args_json: &str, files: Bundle, limits: &AddonLimits) -> Result<(Bundle, Report), AddonError> {
        self.run_limited(op, args_json, files, limits)
    }

    fn run_limited(&self, op: &str, args_json: &str, files: Bundle, limits: &AddonLimits) -> Result<(Bundle, Report), AddonError> {
        // Everything is checked before anything is copied or run.
        if files.len() + 2 > limits.files {
            return Err(AddonError::TooLarge(TOO_MANY_FILES));
        }
        if let Some((name, _)) = files.iter().find(|(name, _)| !safe_name(name)) {
            return Err(malformed(format!("it can't be given a file named {name:?}")));
        }
        let size = 8 + [(OP, op.len()), (ARGS, args_json.len())].iter().map(|(name, len)| 8 + name.len() + len).sum::<usize>()
            + files.iter().map(|(name, data)| 8 + name.len() + data.len()).sum::<usize>();
        if size > limits.input_bytes || u32::try_from(size).is_err() {
            return Err(AddonError::TooLarge(INPUT_TOO_LARGE));
        }
        let mut input = Vec::with_capacity(files.len() + 2);
        input.push((OP.to_string(), op.as_bytes().to_vec()));
        input.push((ARGS.to_string(), args_json.as_bytes().to_vec()));
        input.extend(files);
        let input = encode(&input);
        let len = input.len() as u32;

        let mut running = Running::start(&self.module, limits)?;
        let alloc = running.export::<u32, u32>("alloc")?;
        let run = running.export::<(u32, u32), u64>("run")?;
        let at = running.call(&alloc, len)?;
        running
            .memory
            .write(&mut running.store, at as usize, &input)
            .map_err(|_| malformed("it gave room for its input outside its memory".into()))?;
        drop(input);
        let packed = running.call(&run, (at, len))?;
        let (at, len) = unpack(packed);
        if len > limits.output_bytes {
            return Err(AddonError::TooLarge(OUTPUT_TOO_LARGE));
        }
        let output = running.read(at, len)?;
        drop(running);
        answer(&output, limits)
    }
}

fn malformed(message: String) -> AddonError {
    AddonError::Malformed(message)
}

/// Another part's message, as part of a sentence: on one line, short, starting in lower case
/// and without its full stop.
fn detail(message: &str) -> String {
    let line = one_line(message.as_bytes());
    let line = shortened(line.trim_end_matches('.'), 200);
    // "The bundle …" reads on as "the bundle …", but "WebAssembly …" stays as it is.
    let mut chars = line.chars();
    match (chars.next(), chars.next()) {
        (Some(first), Some(second)) if second.is_lowercase() => first.to_lowercase().chain(line[first.len_utf8()..].chars()).collect(),
        _ => line,
    }
}

/// The files and the report in what an add-on gave back, or its own sentence on why it failed.
fn answer(output: &[u8], limits: &AddonLimits) -> Result<(Bundle, Report), AddonError> {
    let mut files = decode(output).map_err(|message| malformed(format!("what it gave back can't be read: {}", detail(&message))))?;
    if files.len() > limits.files {
        return Err(AddonError::TooLarge(TOO_MANY_FILES));
    }
    if let Some((_, sentence)) = files.iter().find(|(name, _)| name == ERROR) {
        return Err(AddonError::Failed(one_line(sentence)));
    }
    if let Some((name, _)) = files.iter().find(|(name, _)| name == OP || name == ARGS) {
        return Err(malformed(format!("it gave back {name}, which only the app gives")));
    }
    let at = files.iter().position(|(name, _)| name == REPORT).ok_or_else(|| malformed("it gave back no report".into()))?;
    let (_, report) = files.remove(at);
    let report = serde_json::from_slice(&report).map_err(|error| malformed(format!("its report can't be read ({})", detail(&error.to_string()))))?;
    Ok((files, report))
}

/// An add-on's sentence, on one line of at most `MAX_ERROR_CHARS` characters.
fn one_line(sentence: &[u8]) -> String {
    let text = String::from_utf8_lossy(sentence);
    let words: Vec<&str> = text.split(|c: char| c.is_control() || c.is_whitespace()).filter(|word| !word.is_empty()).collect();
    let line = words.join(" ");
    if line.is_empty() {
        return "it gave no reason.".into();
    }
    shortened(&line, MAX_ERROR_CHARS)
}

/// Checks what the module asks for and offers before anything of it runs.
fn check_shape(module: &Module) -> Result<(), AddonError> {
    if let Some(import) = module.imports().next() {
        return Err(malformed(format!("it asks for {}.{}, and an add-on can have nothing from the app", import.module(), import.name())));
    }
    for (name, params, results) in EXPORTS {
        let Some(ExternType::Func(function)) = module.get_export(name) else {
            return Err(malformed(format!("it has no `{name}` function")));
        };
        let expected = FuncType::new(engine(), params.iter().cloned(), results.iter().cloned());
        if !FuncType::eq(&function, &expected) {
            return Err(malformed(format!("its `{name}` function isn't of the kind add-ons have")));
        }
    }
    match module.get_export("memory") {
        Some(ExternType::Memory(memory)) if !memory.is_64() && !memory.is_shared() => Ok(()),
        Some(ExternType::Memory(_)) => Err(malformed("its memory isn't of the kind add-ons have".into())),
        _ => Err(malformed("it has no `memory`".into())),
    }
}

/// Holds what an add-on says it is to what the app can show.
fn check_info(info: &AddonInfo) -> Result<(), AddonError> {
    let a_name = |name: &str| {
        (1..=MAX_INFO_STRING_BYTES).contains(&name.len()) && name.bytes().all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
    };
    if !a_name(&info.name) {
        return Err(malformed(format!("its name, {:?}, isn't lower-case letters, digits and dashes", shortened(&info.name, 80))));
    }
    let version_is_plain = info.version.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '+'));
    if !(1..=MAX_INFO_STRING_BYTES).contains(&info.version.len()) || !version_is_plain {
        return Err(malformed(format!("its version, {:?}, isn't a version", shortened(&info.version, 80))));
    }
    if info.operations.len() > MAX_OPERATIONS {
        return Err(malformed(format!("it names more than {MAX_OPERATIONS} operations")));
    }
    let mut seen = BTreeSet::new();
    for operation in &info.operations {
        if !a_name(operation) || !seen.insert(operation) {
            return Err(malformed(format!("its operation {:?} isn't a name it can have", shortened(operation, 80))));
        }
    }
    Ok(())
}

/// Holds an add-on to its memory, and remembers when it asked for more than it may have.
struct Limiter {
    memory_bytes: usize,
    refused_growth: bool,
}

impl ResourceLimiter for Limiter {
    fn memory_growing(&mut self, _current: usize, desired: usize, _maximum: Option<usize>) -> wasmtime::Result<bool> {
        let allowed = desired <= self.memory_bytes;
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

/// One instance of an add-on, in a store of its own.
struct Running {
    store: Store<Limiter>,
    instance: Instance,
    memory: Memory,
}

impl Running {
    fn start(module: &Module, limits: &AddonLimits) -> Result<Running, AddonError> {
        let mut store = Store::new(engine(), Limiter { memory_bytes: limits.memory_bytes, refused_growth: false });
        store.limiter(|limiter| limiter);
        // Set before instantiating, as a start function is the add-on's code too.
        store.set_epoch_deadline((limits.time.as_millis() / TICK.as_millis()).max(1) as u64);
        store.epoch_deadline_trap();
        // `check_shape` let through no imports, so there is nothing to give it.
        let instance = match Instance::new(&mut store, module, &[]) {
            Ok(instance) => instance,
            Err(error) if error.downcast_ref::<Trap>().is_some() || store.data().refused_growth => return Err(fault(&error, &store)),
            Err(error) => return Err(malformed(format!("it couldn't be started ({})", detail(&error.to_string())))),
        };
        let memory = instance.get_memory(&mut store, "memory").ok_or_else(|| malformed("it has no `memory`".into()))?;
        Ok(Running { store, instance, memory })
    }

    fn export<Params: wasmtime::WasmParams, Results: wasmtime::WasmResults>(
        &mut self,
        name: &str,
    ) -> Result<TypedFunc<Params, Results>, AddonError> {
        self.instance
            .get_typed_func(&mut self.store, name)
            .map_err(|error| malformed(format!("its `{name}` isn't what add-ons export ({})", detail(&error.to_string()))))
    }

    fn call<Params: wasmtime::WasmParams, Results: wasmtime::WasmResults>(
        &mut self,
        function: &TypedFunc<Params, Results>,
        params: Params,
    ) -> Result<Results, AddonError> {
        function.call(&mut self.store, params).map_err(|error| fault(&error, &self.store))
    }

    /// `len` bytes at `at` in the add-on's memory, which the caller has held to a limit.
    fn read(&self, at: usize, len: usize) -> Result<Vec<u8>, AddonError> {
        let bytes = at
            .checked_add(len)
            .and_then(|end| self.memory.data(&self.store).get(at..end))
            .ok_or_else(|| malformed("it points outside its memory".into()))?;
        Ok(bytes.to_vec())
    }
}

/// What stopped an add-on, told apart by the clock and by the limiter.
fn fault(error: &wasmtime::Error, store: &Store<Limiter>) -> AddonError {
    if error.downcast_ref::<Trap>() == Some(&Trap::Interrupt) {
        AddonError::TookTooLong
    } else if store.data().refused_growth {
        AddonError::OutOfMemory
    } else {
        AddonError::Trapped
    }
}
