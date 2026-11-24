//! Writing a talk out for other programs, and handing a file over to Stagepack.
//!
//! An export builds a deck of its own from [`ExportInput`], so it can run on a background
//! thread and share nothing with the window but the talk's files. Its 3D models are drawn as
//! stills there, by the app's drawer.

use std::io::Write as _;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;

use slides_engine::{Deck, Fonts, TalkFiles, write_atomically};
use slides_format::Block;
use slides_pdf::write_pdf;
use slides_pptx::{PptxOutput, write_pptx};

/// The longest a suggested file name's stem gets, in characters. Four bytes each at worst, and
/// file systems stop at 255 bytes with the extension still to come.
const LONGEST_STEM: usize = 60;

/// Everything an export needs, taken from the window's document.
pub struct ExportInput {
    pub source: String,
    pub files: Arc<dyn TalkFiles + Send + Sync>,
    pub talk_fonts: Vec<Arc<Vec<u8>>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExportKind {
    PowerPoint,
    Pdf,
}

impl ExportKind {
    /// The extension of the file this kind of export writes.
    pub fn extension(self) -> &'static str {
        match self {
            ExportKind::PowerPoint => "pptx",
            ExportKind::Pdf => "pdf",
        }
    }
}

/// Writes the talk to `out`, replacing whatever is there only once it has all been made, and
/// returns what the user should hear about the slides. This blocks until it's done.
pub fn run_export(input: ExportInput, kind: ExportKind, out: &Path) -> Result<Vec<String>, String> {
    run_export_reading_with(input, kind, out, |make| make())
}

/// [`run_export`], reading the talk inside `reading`, as the talk's readers keep a save from
/// replacing its files while they're read: the output is written after.
pub fn run_export_reading_with(
    input: ExportInput,
    kind: ExportKind,
    out: &Path,
    reading: impl FnOnce(&mut dyn FnMut() -> Result<(Vec<u8>, Vec<String>), String>) -> Result<(Vec<u8>, Vec<String>), String>,
) -> Result<Vec<String>, String> {
    let mut input = Some(input);
    // Nothing in a talk should get this far, but the window is waiting to hear how it went.
    let mut make_it = || match input.take() {
        Some(input) => catch_unwind(AssertUnwindSafe(|| make(input, kind)))
            .unwrap_or_else(|_| Err("the exporter stopped on something in this talk".to_string())),
        None => Err("the exporter stopped on something in this talk".to_string()),
    };
    let (bytes, notices) = reading(&mut make_it).map_err(|problem| format!("Could not export {}: {problem}", out.display()))?;
    write_atomically(out, |file| file.write_all(&bytes))
        .map_err(|error| format!("Could not write {}: {error}", out.display()))?;
    Ok(notices)
}

/// The fonts a talk is drawn with: the system's, and then its own.
pub(crate) fn fonts_with(talk_fonts: Vec<Arc<Vec<u8>>>) -> Fonts {
    let mut fonts = Fonts::with_system_fallback();
    for font in talk_fonts {
        fonts.load_talk_font(font);
    }
    fonts
}

/// The exported file's bytes, and what the user should hear about the slides.
fn make(input: ExportInput, kind: ExportKind) -> Result<(Vec<u8>, Vec<String>), String> {
    let ExportInput {
        source,
        files,
        talk_fonts,
    } = input;
    let mut deck = Deck::open(Box::new(files.clone()), fonts_with(talk_fonts), source);
    // Drawn here and now, on this thread, as each slide is laid out.
    deck.draw_models_with(Arc::new(crate::model_view::draw_still));
    match kind {
        ExportKind::PowerPoint => {
            let PptxOutput { bytes, mut notices } = write_pptx(&mut deck, &files)?;
            let every: Vec<usize> = (0..deck.slide_count()).collect();
            notices.extend(videos_as_pictures(videos_on(&mut deck, &every)));
            notices.extend(models_as_pictures(models_on(&mut deck, &every)));
            let (recordings, live) = terminals_on(&mut deck, &every);
            notices.extend(terminals_as_screens(recordings));
            notices.extend(live_terminals_as_stills(live));
            Ok((bytes, notices))
        }
        ExportKind::Pdf => {
            let bytes = write_pdf(&mut deck)?;
            let shown = deck.shown_slides().to_vec();
            let videos = videos_on(&mut deck, &shown);
            let models = models_on(&mut deck, &shown);
            let (terminals, live) = terminals_on(&mut deck, &shown);
            let mut notices: Vec<String> = shown
                .into_iter()
                .flat_map(|index| {
                    deck.notices_whole(index)
                        .into_iter()
                        .map(move |notice| format!("Slide {}: {notice}", index + 1))
                })
                .collect();
            notices.extend(videos_as_pictures(videos));
            notices.extend(models_as_pictures(models));
            notices.extend(terminals_as_screens(terminals));
            notices.extend(live_terminals_as_stills(live));
            Ok((bytes, notices))
        }
    }
}

/// How many videos slides `slides` of `deck`'s talk hold, placed or in the text, that were drawn
/// as a picture, their first or the still they name: one that can't be played is its plain panel,
/// and says so in its own notice. Asked once the slides are laid out.
fn videos_on(deck: &mut Deck, slides: &[usize]) -> usize {
    fn videos(blocks: &[Block], found: &mut Vec<(String, Option<String>)>) {
        for block in blocks {
            if let Block::Tag(tag) = block {
                if tag.name == "Video" {
                    found.push((tag.attr("src").unwrap_or_default().to_string(), tag.attr("poster").map(str::to_string)));
                }
                videos(&tag.children, found);
            }
        }
    }
    let mut found = Vec::new();
    for slide in slides.iter().filter_map(|index| deck.talk().slides.get(*index)) {
        videos(&slide.blocks, &mut found);
    }
    found.into_iter().filter(|(src, poster)| deck.video_shows_a_picture(src, poster.as_deref())).count()
}

/// What an export says of the `videos` it holds, which PowerPoint and PDF get as their stills:
/// nothing, if there are none.
fn videos_as_pictures(videos: usize) -> Option<String> {
    match videos {
        0 => None,
        1 => Some("1 video was exported as a picture.".to_string()),
        videos => Some(format!("{videos} videos were exported as pictures.")),
    }
}

/// How many 3D models placed on slides `slides` of `deck`'s talk were drawn as a picture, their
/// still: one that can't be drawn is its plain panel, and says so in its own notice, and one in
/// the text is a panel saying it's drawn once it's placed. Asked once the slides are laid out.
fn models_on(deck: &mut Deck, slides: &[usize]) -> usize {
    let mut drawn = 0;
    for &index in slides {
        for model in deck.models(index) {
            let Some(source) = model.source else { continue };
            if deck.model_problem(index, &source).is_none() {
                drawn += 1;
            }
        }
    }
    drawn
}

/// What an export says of the `models` it holds, which PowerPoint and PDF get as their stills:
/// nothing, if there are none.
fn models_as_pictures(models: usize) -> Option<String> {
    match models {
        0 => None,
        1 => Some("1 model was exported as a picture.".to_string()),
        models => Some(format!("{models} models were exported as pictures.")),
    }
}

/// How many terminal recordings placed on slides `slides` of `deck`'s talk were drawn as their
/// last screen, and how many live terminals, those that name a command to run, were drawn as
/// their recording or a still: one that can't be shown is its plain panel, and says so in its own
/// notice. Asked once the slides are laid out. Nothing is run: an export has no way to.
fn terminals_on(deck: &mut Deck, slides: &[usize]) -> (usize, usize) {
    let (mut recordings, mut live) = (0, 0);
    for &index in slides {
        for terminal in deck.terminals(index) {
            let Some(source) = terminal.source else { continue };
            if deck.terminal_problem(index, &source).is_some() {
                continue;
            }
            if terminal.on.run.is_some() {
                live += 1;
            } else {
                recordings += 1;
            }
        }
    }
    (recordings, live)
}

/// What an export says of the `terminals` it holds, which PowerPoint gets as pictures of their
/// last screen and PDF as its text: nothing, if there are none.
fn terminals_as_screens(terminals: usize) -> Option<String> {
    match terminals {
        0 => None,
        1 => Some("1 terminal recording was exported as its last screen.".to_string()),
        terminals => Some(format!("{terminals} terminal recordings were exported as their last screen.")),
    }
}

/// What an export says of the `live` terminals it holds, each shown as its recording or a still:
/// nothing, if there are none.
fn live_terminals_as_stills(live: usize) -> Option<String> {
    match live {
        0 => None,
        1 => Some("1 live terminal is shown as its recording or a still.".to_string()),
        live => Some(format!("{live} live terminals are shown as their recording or a still.")),
    }
}

/// A file name, without an extension, to suggest for a talk called `name`. The name comes from
/// inside the talk, so nothing in it can be allowed to look like a path.
pub fn file_stem_for(name: &str) -> String {
    let replaced: String = name
        .chars()
        .take(LONGEST_STEM)
        .map(|c| {
            if c.is_control() || r#"/\:*?"<>|"#.contains(c) {
                '-'
            } else {
                c
            }
        })
        .collect();
    // A leading dot hides a file, and Windows drops trailing dots and spaces.
    match replaced.trim_matches([' ', '.']) {
        "" => "talk".to_string(),
        stem => stem.to_string(),
    }
}

/// Where Stagepack is installed, if it is. `env` reads an environment variable and `exists`
/// says whether a file is there.
pub fn stagepack_executable(
    env: impl Fn(&str) -> Option<String>,
    exists: impl Fn(&Path) -> bool,
) -> Option<PathBuf> {
    // A relative setting would run whatever happens to sit at that path under the current folder.
    let folder = |name: &str| {
        env(name)
            .map(PathBuf::from)
            .filter(|path| path.is_absolute())
    };
    let places = if cfg!(windows) {
        vec![folder("LOCALAPPDATA").map(|dir| dir.join("Stagepack").join("stagepack.exe"))]
    } else {
        vec![
            folder("XDG_DATA_HOME").map(|dir| dir.join("stagepack/stagepack")),
            folder("HOME").map(|dir| dir.join(".local/share/stagepack/stagepack")),
        ]
    };
    places.into_iter().flatten().find(|path| exists(path))
}

/// Runs `stagepack open <file>` and leaves it running.
pub fn open_in_stagepack(executable: &Path, file: &Path) -> std::io::Result<()> {
    let mut stagepack = Command::new(executable)
        .arg("open")
        .arg(file)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    // Waited for, or it would stay in the process table until this app exits.
    std::thread::spawn(move || stagepack.wait());
    Ok(())
}

/// The file name to suggest for a Slidev export of the talk called `name`.
pub fn slidev_file_name(name: &str) -> String {
    format!("{} (Slidev).{}", file_stem_for(name), SLIDEV_EXTENSION)
}

/// The extension of a Slidev talk made for Stagepack.
pub const SLIDEV_EXTENSION: &str = "sptx";

/// The most the files given to the Slidev add-on for an export may come to. The add-on holds
/// what it's given up to three times over in its 1 GiB of memory, beside its 64 MiB stack: the
/// bundle as it was given, the files it reads out of it, and the bundle it gives back. A
/// third of what's left is a little over 320 MiB, so that much always fits, where the 512 MiB
/// any bundle may come to wouldn't.
pub const SLIDEV_EXPORT_BUDGET: u64 = 320 * 1024 * 1024;

const _: () = assert!(
    SLIDEV_EXPORT_BUDGET * 3
        <= (slides_components::addon::ADDON_MEMORY_LIMIT_BYTES - slides_components::addon::ADDON_STACK_BYTES) as u64
        && SLIDEV_EXPORT_BUDGET <= slides_components::addon::MAX_BUNDLE_BYTES as u64
);

/// What a video too large to go to the add-on says, by its file `name`.
fn video_left_out(name: &str) -> String {
    format!("{name} is too large to include; copy it into public/ yourself.")
}

/// The endings of the videos a talk can hold, which are the first left out of an export to Slidev
/// that would be too large.
const VIDEO_KINDS: [&str; 3] = ["mp4", "m4v", "mov"];

fn is_video(name: &str) -> bool {
    name.rsplit_once('.').is_some_and(|(_, ending)| VIDEO_KINDS.iter().any(|kind| ending.eq_ignore_ascii_case(kind)))
}

/// A Slidev export, made and waiting to be written: its Markdown, Stagepack's manifest, the files
/// under `public/`, and what the user should hear.
pub struct SlidevExport {
    slides: Vec<u8>,
    manifest: Vec<u8>,
    public: Vec<(String, Vec<u8>)>,
    notices: Vec<String>,
}

/// What making a Slidev export gives.
pub type MadeSlidev = Result<SlidevExport, String>;

/// Writes the talk to `out` as a Slidev talk made for Stagepack, through the Slidev add-on
/// `addon`: its `slides.md`, the files it shows under `public/`, and Stagepack's manifest, with
/// `id` and created and updated `now`. The parts Slidev can't draw are drawn here as pictures.
/// It returns what the user should hear: what was left out, and the add-on's own notices.
pub fn export_slidev(
    input: ExportInput,
    addon: &slides_components::addon::Addon,
    out: &Path,
    now: chrono::DateTime<chrono::Utc>,
    id: &str,
) -> Result<Vec<String>, String> {
    export_slidev_reading_with(input, addon, out, now, id, |make| make())
}

/// [`export_slidev`], reading the talk inside `reading`, as [`run_export_reading_with`] does.
pub fn export_slidev_reading_with(
    input: ExportInput,
    addon: &slides_components::addon::Addon,
    out: &Path,
    now: chrono::DateTime<chrono::Utc>,
    id: &str,
    reading: impl FnOnce(&mut dyn FnMut() -> MadeSlidev) -> MadeSlidev,
) -> Result<Vec<String>, String> {
    let mut input = Some(input);
    let mut make_it = || match input.take() {
        Some(input) => catch_unwind(AssertUnwindSafe(|| make_slidev(input, addon, out, now, id)))
            .unwrap_or_else(|_| Err("the exporter stopped on something in this talk".to_string())),
        None => Err("the exporter stopped on something in this talk".to_string()),
    };
    let made = reading(&mut make_it).map_err(|problem| format!("Could not export {}: {problem}", out.display()))?;
    // Written straight into the file, so the files aren't held twice over as a zip in memory too.
    write_atomically(out, |file| write_slidev_zip(file, &made).map_err(std::io::Error::other))
        .map_err(|error| format!("Could not write {}: {error}", out.display()))?;
    Ok(made.notices)
}

/// How many bytes a file named `name` of `len` bytes takes in a bundle.
fn in_bundle(name: &str, len: u64) -> u64 {
    8 + name.len() as u64 + len
}

/// The talk's files a Slidev export may show, as the talk names them: every file a picture, a
/// video or another tag names, and the talk's fonts.
fn named_files(deck: &Deck) -> Vec<String> {
    fn walk(blocks: &[Block], found: &mut std::collections::BTreeSet<String>) {
        for block in blocks {
            match block {
                Block::Picture { src, .. } => {
                    found.insert(src.trim().to_string());
                }
                Block::Tag(tag) => {
                    for attr in &tag.attrs {
                        if attr.name == "src" || attr.name == "poster" {
                            found.insert(attr.value.trim().to_string());
                        }
                    }
                    walk(&tag.children, found);
                }
                Block::List { items, .. } => items.iter().for_each(|item| walk(item, found)),
                _ => {}
            }
        }
    }
    let mut found = std::collections::BTreeSet::new();
    for slide in &deck.talk().slides {
        walk(&slide.blocks, &mut found);
    }
    found.into_iter().filter(|name| !name.is_empty() && name != crate::addons::SLIDES_FILE).collect()
}

/// A file of the talk to give the add-on: its name, its length, and its bytes, if they had to be
/// read to know its length.
struct Wanted {
    name: String,
    len: u64,
    read: Option<Vec<u8>>,
    left_out: bool,
}

/// The export, made but not yet written.
fn make_slidev(
    input: ExportInput,
    addon: &slides_components::addon::Addon,
    out: &Path,
    now: chrono::DateTime<chrono::Utc>,
    id: &str,
) -> MadeSlidev {
    let ExportInput { source, files, talk_fonts } = input;
    let mut deck = Deck::open(Box::new(files.clone()), fonts_with(talk_fonts), source.clone());
    deck.draw_models_with(Arc::new(crate::model_view::draw_still));
    let baked = crate::addons::bake::bake(&mut deck);
    let title = deck.title().filter(|title| !title.trim().is_empty()).unwrap_or_else(|| {
        let stem = out.file_stem().map_or_else(String::new, |stem| stem.to_string_lossy().into_owned());
        stem.strip_suffix(" (Slidev)").unwrap_or(&stem).to_string()
    });
    let args = serde_json::json!({ "title": title }).to_string();

    let mut names = crate::addons::Names::default();
    let mut wanted: Vec<Wanted> = Vec::new();
    let fonts = files.names_in("assets/fonts");
    for name in named_files(&deck).into_iter().chain(fonts) {
        let Some(name) = slides_engine::normalise(&name).filter(|name| name != crate::addons::SLIDES_FILE) else { continue };
        if wanted.iter().any(|known| known.name == name) {
            continue;
        }
        // Measured without reading it where the files can; otherwise read once, and kept. One the
        // talk doesn't hold is the add-on's to say.
        let (len, read) = match files.len(&name) {
            Some(len) => (len, None),
            None => match read_whole(files.as_ref(), &name) {
                Some(bytes) => (bytes.len() as u64, Some(bytes)),
                None => continue,
            },
        };
        if names.take(&name) {
            wanted.push(Wanted { name, len, read, left_out: false });
        }
    }
    let mut notices: Vec<String> = names.notice().into_iter().map(|notice| notice.text).collect();

    // Over the budget, the largest videos are left out first, linked by name.
    let fixed = 8
        + in_bundle(".op", 6)
        + in_bundle(".args.json", args.len() as u64)
        + in_bundle(crate::addons::SLIDES_FILE, source.len() as u64)
        + baked.iter().map(|(name, data)| in_bundle(name, data.len() as u64)).sum::<u64>();
    let mut total = fixed + wanted.iter().map(|file| in_bundle(&file.name, file.len)).sum::<u64>();
    while total > SLIDEV_EXPORT_BUDGET {
        let Some(file) = wanted.iter_mut().filter(|file| is_video(&file.name) && !file.left_out).max_by_key(|file| file.len) else {
            let budget = SLIDEV_EXPORT_BUDGET >> 20;
            let some_left_out = wanted.iter().any(|file| file.left_out);
            return Err(if some_left_out {
                format!("the talk's files come to more than {budget} MiB, the most the Slidev add-on can be given, even without its videos")
            } else {
                format!("the talk's files come to more than {budget} MiB, the most the Slidev add-on can be given")
            });
        };
        total -= file.len;
        file.left_out = true;
        file.read = None;
        notices.push(video_left_out(&file.name));
    }
    let any_left_out = wanted.iter().any(|file| file.left_out);

    let mut bundle: addon_abi::Bundle = vec![(crate::addons::SLIDES_FILE.to_string(), source.into_bytes())];
    for file in wanted {
        // A video left out is given empty, so that the talk still names it.
        let data = match (file.left_out, file.read) {
            (true, _) => Vec::new(),
            (false, Some(bytes)) => bytes,
            (false, None) => read_whole(files.as_ref(), &file.name).ok_or_else(|| format!("{} couldn't be read", file.name))?,
        };
        bundle.push((file.name, data));
    }
    bundle.extend(baked);
    let (given, report) = addon.run("export", &args, bundle).map_err(|error| error.to_string())?;

    let mut slides = None;
    let mut manifest_title = None;
    let mut public = Vec::new();
    for (name, data) in given {
        if name == addon_abi::MANIFEST {
            let fields: serde_json::Value = serde_json::from_slice(&data).map_err(|_| "the Slidev add-on gave back a manifest that can't be read".to_string())?;
            manifest_title = fields.get("title").and_then(|title| title.as_str()).map(str::to_string);
        } else if name == crate::addons::SLIDES_FILE {
            slides = Some(data);
        } else if name.starts_with("public/") && addon_abi::safe_name(&name) {
            // The videos given empty, by whatever name the add-on gave them: the user copies
            // those in. A talk's own video is never empty.
            if any_left_out && data.is_empty() && is_video(&name) {
                continue;
            }
            public.push((name, data));
        }
    }
    let slides = slides.ok_or("the Slidev add-on gave back no slides")?;
    let manifest = serde_json::json!({
        "format": 1,
        "id": id,
        "title": manifest_title.unwrap_or(title),
        "created": now.to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
        "updated": now.to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
        "slidev": crate::addons::SLIDEV_VERSION,
        "entry": crate::addons::SLIDES_FILE,
        "meta": {},
    });
    let manifest = serde_json::to_vec_pretty(&manifest).expect("a manifest is always JSON");
    notices.extend(report.notices.into_iter().map(|notice| match notice.slide {
        Some(slide) => format!("Slide {slide}: {}", notice.text),
        None => notice.text,
    }));
    Ok(SlidevExport { slides, manifest, public, notices })
}

/// The whole of the talk's file `name`.
fn read_whole(files: &(dyn TalkFiles + Send + Sync), name: &str) -> Option<Vec<u8>> {
    if let Some(bytes) = files.read(name) {
        return Some(bytes);
    }
    let mut bytes = Vec::new();
    std::io::Read::read_to_end(&mut files.open(name)?, &mut bytes).ok()?;
    Some(bytes)
}

/// Writes `made` into `file` as the zip of a Slidev talk made for Stagepack: `slides.md` first,
/// then the manifest, then the files under `public/`, which are pictures, video and fonts, and
/// stored as they are.
fn write_slidev_zip(file: &mut std::fs::File, made: &SlidevExport) -> zip::result::ZipResult<()> {
    use zip::write::SimpleFileOptions;
    use zip::{CompressionMethod, ZipWriter};

    let mut writer = ZipWriter::new(file);
    let text = SimpleFileOptions::DEFAULT.compression_method(CompressionMethod::Deflated);
    let stored = SimpleFileOptions::DEFAULT.compression_method(CompressionMethod::Stored);
    writer.start_file(crate::addons::SLIDES_FILE, text)?;
    writer.write_all(&made.slides)?;
    writer.start_file(slides_engine::STAGEPACK_MANIFEST, text)?;
    writer.write_all(&made.manifest)?;
    for (name, data) in &made.public {
        writer.start_file(name.as_str(), stored.large_file(data.len() as u64 >= u64::from(u32::MAX)))?;
        writer.write_all(data)?;
    }
    writer.finish()?;
    Ok(())
}
