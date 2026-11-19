use std::alloc::{GlobalAlloc, Layout, System};
use std::io::{Cursor, Read, Seek, SeekFrom};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use proptest::prelude::*;
use serde_json::{Value, json};
use slides_model::*;

/// Counts the largest single allocation any test here makes, and the most in use at once, so a
/// test can tell whether a size claimed by a file was ever allocated.
struct Counting;

static LARGEST: AtomicUsize = AtomicUsize::new(0);
static IN_USE: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);

fn allocated(size: usize) {
    LARGEST.fetch_max(size, Ordering::Relaxed);
    let now = IN_USE.fetch_add(size, Ordering::Relaxed) + size;
    PEAK.fetch_max(now, Ordering::Relaxed);
}

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        allocated(layout.size());
        unsafe { System.alloc(layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        allocated(layout.size());
        unsafe { System.alloc_zeroed(layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        IN_USE.fetch_sub(layout.size(), Ordering::Relaxed);
        allocated(new_size);
        unsafe { System.realloc(ptr, layout, new_size) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        IN_USE.fetch_sub(layout.size(), Ordering::Relaxed);
        unsafe { System.dealloc(ptr, layout) }
    }
}

#[global_allocator]
static COUNTING: Counting = Counting;

/// The most allocated at once, and the most in use at once. The other tests here run beside
/// each case and count too, so the second is the whole test program's peak.
const MOST_ALLOCATED: usize = 300 * 1024 * 1024;
const MOST_TIME: Duration = Duration::from_secs(2);

fn check_memory() {
    assert!(LARGEST.load(Ordering::Relaxed) <= MOST_ALLOCATED, "allocated {} at once", LARGEST.load(Ordering::Relaxed));
    assert!(PEAK.load(Ordering::Relaxed) <= MOST_ALLOCATED, "{} in use at once", PEAK.load(Ordering::Relaxed));
}

/// How much was read from a reader, and whether anything was asked for past its end.
#[derive(Default)]
struct Count {
    bytes: AtomicU64,
    reads: AtomicU64,
    past_end: AtomicU64,
    /// The furthest byte any read reached.
    furthest: AtomicU64,
}

/// A reader of bytes in memory that counts what's read of it.
struct Counted {
    inner: Cursor<Vec<u8>>,
    count: Arc<Count>,
}

impl Read for Counted {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if self.inner.position() > self.inner.get_ref().len() as u64 {
            self.count.past_end.fetch_add(1, Ordering::Relaxed);
        }
        let read = self.inner.read(buf)?;
        self.count.furthest.fetch_max(self.inner.position(), Ordering::Relaxed);
        self.count.bytes.fetch_add(read as u64, Ordering::Relaxed);
        self.count.reads.fetch_add(1, Ordering::Relaxed);
        Ok(read)
    }
}

impl Seek for Counted {
    fn seek(&mut self, pos: SeekFrom) -> std::io::Result<u64> {
        self.inner.seek(pos)
    }
}

fn counted(bytes: Vec<u8>) -> (Box<dyn ReadSeek>, Arc<Count>) {
    let count = Arc::new(Count::default());
    (Box::new(Counted { inner: Cursor::new(bytes), count: count.clone() }), count)
}

fn reader(bytes: Vec<u8>) -> Box<dyn ReadSeek> {
    Box::new(Cursor::new(bytes))
}

/// Says what `bytes` is and reads it whole, each within two seconds and allocating no more than
/// 300 MB. What `read` said, after checking `facts` agreed where it could.
fn facts_and_read(bytes: Vec<u8>) -> Result<Model, CantDraw> {
    PEAK.store(IN_USE.load(Ordering::Relaxed), Ordering::Relaxed);
    let started = Instant::now();
    let facts = facts(reader(bytes.clone()));
    assert!(started.elapsed() < MOST_TIME, "facts took {:?}", started.elapsed());
    let started = Instant::now();
    let model = read(reader(bytes));
    assert!(started.elapsed() < MOST_TIME, "reading took {:?}", started.elapsed());
    check_memory();
    match (&facts, &model) {
        (Err(why), _) => assert_eq!(model.as_ref().err(), Some(why), "read and facts disagree"),
        (Ok(facts), Ok(model)) => {
            assert_eq!(facts.kind, model.facts.kind);
            assert_eq!(facts.triangles, model.facts.triangles);
            assert_eq!(facts.points, model.facts.points);
        }
        (Ok(_), Err(_)) => {}
    }
    model
}

fn close(a: [f32; 3], b: [f32; 3]) -> bool {
    a.iter().zip(b).all(|(a, b)| (a - b).abs() < 1e-4)
}

fn grid() -> &'static [u8] {
    static GRID: OnceLock<Vec<u8>> = OnceLock::new();
    GRID.get_or_init(|| make_test_glb(MadeGlb::Grid { cells: 8, texture: 64, jpeg: false }))
}

fn jpeg_grid() -> &'static [u8] {
    static GRID: OnceLock<Vec<u8>> = OnceLock::new();
    GRID.get_or_init(|| make_test_glb(MadeGlb::Grid { cells: 2, texture: 16, jpeg: true }))
}

fn boxes() -> &'static [u8] {
    static BOXES: OnceLock<Vec<u8>> = OnceLock::new();
    BOXES.get_or_init(|| make_test_glb(MadeGlb::Boxes))
}

fn cloud() -> &'static [u8] {
    static CLOUD: OnceLock<Vec<u8>> = OnceLock::new();
    CLOUD.get_or_init(|| make_test_ply(200, true))
}

fn text_cloud() -> &'static [u8] {
    static CLOUD: OnceLock<Vec<u8>> = OnceLock::new();
    CLOUD.get_or_init(|| make_test_ply(200, false))
}

/// A made model with its description changed by `change`.
fn edited(glb: &[u8], change: impl FnOnce(&mut Value, &mut Vec<u8>)) -> Vec<u8> {
    let (text, mut bin) = glb_parts(glb).unwrap();
    let mut json: Value = serde_json::from_str(&text).unwrap();
    change(&mut json, &mut bin);
    glb_from(&json.to_string(), &bin)
}

fn texture_colour(texture: &Texture, x: u32, y: u32) -> [u8; 4] {
    let at = ((y * texture.width + x) * 4) as usize;
    texture.rgba[at..at + 4].try_into().unwrap()
}

/// The made grid with `picture` as its texture.
fn with_texture(picture: &[u8]) -> Vec<u8> {
    edited(grid(), |json, bin| {
        while bin.len() % 4 != 0 {
            bin.push(0);
        }
        json["bufferViews"].as_array_mut().unwrap().push(json!({ "buffer": 0, "byteOffset": bin.len(), "byteLength": picture.len() }));
        bin.extend_from_slice(picture);
        json["images"][0]["bufferView"] = json!(json["bufferViews"].as_array().unwrap().len() - 1);
        json["buffers"][0]["byteLength"] = json!(bin.len());
    })
}

/// The made grid with `picture` as its texture, followed by `pad` bytes of nothing kept in the
/// same part of the file: the start of the file, as far as the picture's end, and its length.
fn padded_texture(picture: &[u8], pad: u64) -> (Vec<u8>, u64) {
    padded_texture_with(picture, pad, 0)
}

/// The same, the padding of `fill` bytes, as `Virtual::filled` gives them. The few bytes that
/// pad the picture to four are made `fill` too.
fn padded_texture_with(picture: &[u8], pad: u64, fill: u8) -> (Vec<u8>, u64) {
    assert_eq!(pad % 4, 0);
    let glb = with_texture(picture);
    let (text, bin) = glb_parts(&glb).unwrap();
    let mut json: Value = serde_json::from_str(&text).unwrap();
    let view = json["images"][0]["bufferView"].as_u64().unwrap() as usize;
    let view = &mut json["bufferViews"][view];
    // The picture ends the made data, but for padding to four.
    let offset = view["byteOffset"].as_u64().unwrap() as usize;
    assert!(bin.len() - (offset + picture.len()) < 4);
    view["byteLength"] = json!((bin.len() - offset) as u64 + pad);
    json["buffers"][0]["byteLength"] = json!(bin.len() as u64 + pad);
    let mut start = glb_from(&json.to_string(), &bin);
    // The block of data, and the whole file, are as long as the nothing after it makes them.
    let total = u32::from_le_bytes(start[8..12].try_into().unwrap()) + pad as u32;
    start[8..12].copy_from_slice(&total.to_le_bytes());
    let described = u32::from_le_bytes(start[12..16].try_into().unwrap()) as usize;
    let bin_at = 20 + described;
    let bin_len = u32::from_le_bytes(start[bin_at..bin_at + 4].try_into().unwrap()) + pad as u32;
    start[bin_at..bin_at + 4].copy_from_slice(&bin_len.to_le_bytes());
    start[bin_at + 8 + offset + picture.len()..].fill(fill);
    let len = u64::from(total);
    (start, len)
}

/// A progressive JPEG of `width` by `height`, of one colour, grey or with three numbers to a
/// pixel at full size each (4:4:4): its first scan the average of every block, then one scan
/// of the rest for each number, every block of which is nothing.
fn progressive_jpeg(width: u16, height: u16, components: u8) -> Vec<u8> {
    let mut out = vec![0xFF, 0xD8];
    let part = |out: &mut Vec<u8>, marker: u8, body: &[u8]| {
        out.extend_from_slice(&[0xFF, marker]);
        out.extend_from_slice(&(body.len() as u16 + 2).to_be_bytes());
        out.extend_from_slice(body);
    };
    part(&mut out, 0xDB, &[[0u8].as_slice(), &[1; 64]].concat());
    let mut frame = vec![8];
    frame.extend_from_slice(&height.to_be_bytes());
    frame.extend_from_slice(&width.to_be_bytes());
    frame.push(components);
    for id in 1..=components {
        frame.extend_from_slice(&[id, 0x11, 0]);
    }
    part(&mut out, 0xC2, &frame);
    // Two codes in each table: the shortest, `0`, for no difference and for the end of a block.
    let counts = [[1u8, 1].as_slice(), &[0; 14]].concat();
    part(&mut out, 0xC4, &[[0x00].as_slice(), &counts, &[0, 1], &[0x10], &counts, &[0x00, 0x01]].concat());
    let blocks = (u64::from(width).div_ceil(8) * u64::from(height).div_ceil(8)) as usize;
    let coded = |bits: usize| {
        let mut bytes = vec![0u8; bits.div_ceil(8)];
        if !bits.is_multiple_of(8) {
            *bytes.last_mut().unwrap() = 0xFF >> (bits % 8);
        }
        bytes
    };
    let mut scan = vec![components];
    for id in 1..=components {
        scan.extend_from_slice(&[id, 0x00]);
    }
    scan.extend_from_slice(&[0, 0, 0]);
    part(&mut out, 0xDA, &scan);
    out.extend_from_slice(&coded(blocks * components as usize));
    for id in 1..=components {
        part(&mut out, 0xDA, &[1, id, 0x00, 1, 63, 0]);
        out.extend_from_slice(&coded(blocks));
    }
    out.extend_from_slice(&[0xFF, 0xD9]);
    out
}

/// A PNG of 4 by 4, 16 bits a number, RGBA, all orange.
fn png_of_16_bits() -> Vec<u8> {
    let picture = image::ImageBuffer::<image::Rgba<u16>, _>::from_pixel(4, 4, image::Rgba([65535, 32896, 0, 65535]));
    let mut bytes = Cursor::new(Vec::new());
    picture.write_to(&mut bytes, image::ImageFormat::Png).unwrap();
    bytes.into_inner()
}

/// `png` with the size in its header changed, and the header's check made again to match.
fn resized_png(mut png: Vec<u8>, width: u32, height: u32) -> Vec<u8> {
    png[16..20].copy_from_slice(&width.to_be_bytes());
    png[20..24].copy_from_slice(&height.to_be_bytes());
    let check = crc32(&png[12..29]);
    png[29..33].copy_from_slice(&check.to_be_bytes());
    png
}

fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for byte in bytes {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            crc = if crc & 1 == 1 { (crc >> 1) ^ 0xEDB8_8320 } else { crc >> 1 };
        }
    }
    !crc
}

#[test]
fn a_made_model_says_what_it_is() {
    let said = facts(reader(grid().to_vec())).unwrap();
    assert_eq!(said.kind, ModelKind::Mesh);
    assert_eq!((said.triangles, said.points), (128, 0));
    assert!(close(said.bounds.0, [-1.0, -1.0, 0.0]) && close(said.bounds.1, [1.0, 1.0, 0.0]), "{:?}", said.bounds);

    let model = read(reader(grid().to_vec())).unwrap();
    assert_eq!(model.facts, said);
    assert!(model.points.is_none());
    assert_eq!(model.meshes.len(), 1);
    let mesh = &model.meshes[0];
    assert_eq!(mesh.positions.len(), 81);
    assert_eq!(mesh.indices.len(), 128 * 3);
    assert_eq!(mesh.texture_coords.len(), 81);
    assert_eq!(mesh.colours.len(), 81);
    // The grid has no normals of its own: they're made, facing the front.
    assert_eq!(mesh.normals.len(), 81);
    assert!(mesh.normals.iter().all(|n| close(*n, [0.0, 0.0, 1.0])), "{:?}", &mesh.normals[..3]);
    assert!(mesh.textured);
    let texture = model.texture.as_ref().unwrap();
    assert_eq!((texture.width, texture.height, texture.rgba.len()), (64, 64, 64 * 64 * 4));
    assert_eq!(texture_colour(texture, 4, 4), GRID_COLOURS[0]);
    assert_eq!(texture_colour(texture, 60, 4), GRID_COLOURS[1]);
    assert_eq!(texture_colour(texture, 4, 60), GRID_COLOURS[2]);
    assert_eq!(texture_colour(texture, 60, 60), GRID_COLOURS[3]);
    // The top left corner of the grid is the top left of its picture.
    let corner = mesh.positions.iter().position(|p| close(*p, [-1.0, 1.0, 0.0])).unwrap();
    assert_eq!(mesh.texture_coords[corner], [0.0, 0.0]);

    // The same with a JPEG, whose colours come back near.
    let jpeg = read(reader(make_test_glb(MadeGlb::Grid { cells: 2, texture: 32, jpeg: true }))).unwrap();
    let texture = jpeg.texture.unwrap();
    assert_eq!((texture.width, texture.height), (32, 32));
    let near = |a: [u8; 4], b: [u8; 4]| a.iter().zip(b).all(|(a, b)| a.abs_diff(b) <= 24);
    assert!(near(texture_colour(&texture, 4, 4), GRID_COLOURS[0]), "{:?}", texture_colour(&texture, 4, 4));
    assert!(near(texture_colour(&texture, 28, 28), GRID_COLOURS[3]));

    // A box of three materials, standing on the ground.
    let said = facts(reader(boxes().to_vec())).unwrap();
    assert_eq!((said.kind, said.triangles, said.points), (ModelKind::Mesh, 12, 0));
    assert!(close(said.bounds.0, [-0.5, 0.0, -0.5]) && close(said.bounds.1, [0.5, 1.0, 0.5]), "{:?}", said.bounds);
    let model = read(reader(boxes().to_vec())).unwrap();
    assert!(model.texture.is_none());
    assert_eq!(model.meshes.len(), 3);
    for (mesh, colour) in model.meshes.iter().zip(BOX_COLOURS) {
        assert_eq!((mesh.positions.len(), mesh.indices.len()), (8, 12));
        assert!(!mesh.textured && mesh.texture_coords.is_empty());
        assert!(mesh.colours.iter().all(|c| *c == colour), "{:?}", mesh.colours[0]);
        // Each normal, given or made, points out of the box.
        for (p, n) in mesh.positions.iter().zip(&mesh.normals) {
            let out = [p[0], p[1] - 0.5, p[2]];
            let dot: f32 = out.iter().zip(n).map(|(a, b)| a * b).sum();
            let length: f32 = n.iter().map(|v| v * v).sum::<f32>().sqrt();
            assert!(dot > 0.4 && (length - 1.0).abs() < 1e-4, "{p:?} {n:?}");
        }
        // Each triangle turns the same way as its normals.
        for t in mesh.indices.chunks(3) {
            let [a, b, c] = [0, 1, 2].map(|i| mesh.positions[t[i] as usize]);
            let (u, v) = ([b[0] - a[0], b[1] - a[1], b[2] - a[2]], [c[0] - a[0], c[1] - a[1], c[2] - a[2]]);
            let cross = [u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0]];
            let n = mesh.normals[t[0] as usize];
            assert!(cross.iter().zip(n).map(|(a, b)| a * b).sum::<f32>() > 0.0);
        }
    }

    // Saying what a model is reads its description, not its contents.
    let large = make_test_glb(MadeGlb::Grid { cells: 64, texture: 16, jpeg: false });
    let (reader, count) = counted(large.clone());
    assert_eq!(facts(reader).unwrap().triangles, 64 * 64 * 2);
    assert!(count.bytes.load(Ordering::Relaxed) < large.len() as u64 / 4, "{} of {}", count.bytes.load(Ordering::Relaxed), large.len());
}

#[test]
fn a_made_cloud_says_what_it_is() {
    for bytes in [cloud(), text_cloud()] {
        let said = facts(reader(bytes.to_vec())).unwrap();
        assert_eq!((said.kind, said.triangles, said.points), (ModelKind::Points, 0, 200));
        let model = read(reader(bytes.to_vec())).unwrap();
        assert!(model.meshes.is_empty() && model.texture.is_none());
        let points = model.points.as_ref().unwrap();
        assert_eq!((points.positions.len(), points.colours.len()), (200, 200));
        assert!(points.positions.iter().all(|p| p.iter().all(|v| v.abs() <= 1.0 + 1e-4)));
        let (low, high) = model.facts.bounds;
        assert!(low.iter().all(|v| *v < -0.8) && high.iter().all(|v| *v > 0.8), "{:?}", model.facts.bounds);
        assert_eq!((model.facts.kind, model.facts.points), (ModelKind::Points, 200));
    }
    let binary = read(reader(cloud().to_vec())).unwrap().points.unwrap();
    let text = read(reader(text_cloud().to_vec())).unwrap().points.unwrap();
    assert!(binary.positions.iter().zip(&text.positions).all(|(a, b)| close(*a, *b)));
    assert_eq!(binary.colours, text.colours);
    assert!(binary.colours.iter().any(|c| c[0] > 200) && binary.colours.iter().any(|c| c[0] < 50));
    assert!(binary.colours.iter().all(|c| c[3] == 255));

    // A cloud with no colours is white.
    let plain = b"ply\nformat ascii 1.0\nelement vertex 2\nproperty float x\nproperty float y\nproperty float z\nend_header\n0 0 0\n1 2 3\n";
    let points = read(reader(plain.to_vec())).unwrap().points.unwrap();
    assert_eq!(points.positions, vec![[0.0, 0.0, 0.0], [1.0, 2.0, 3.0]]);
    assert_eq!(points.colours, vec![[255; 4]; 2]);
}

#[derive(Debug, Clone)]
enum Damage {
    Random(Vec<u8>),
    /// Random bytes as a glTF's description, in a header and chunk of the right lengths.
    RandomDescription(Vec<u8>),
    /// A made glTF's description whole, with random bytes for its data, of the same length.
    RandomData(Vec<u8>),
    /// One number in a made glTF's description changed.
    Number(usize, u64),
    Byte(usize, u8),
    Zeroed(usize, usize),
    Cut(usize),
}

fn damage() -> impl Strategy<Value = Damage> {
    prop_oneof![
        proptest::collection::vec(any::<u8>(), 0..2048).prop_map(Damage::Random),
        proptest::collection::vec(any::<u8>(), 0..2048).prop_map(Damage::RandomDescription),
        proptest::collection::vec(any::<u8>(), 1..4096).prop_map(Damage::RandomData),
        (any::<usize>(), prop_oneof![0u64..16, any::<u32>().prop_map(u64::from), any::<u64>()]).prop_map(|(at, value)| Damage::Number(at, value)),
        (any::<usize>(), any::<u8>()).prop_map(|(at, value)| Damage::Byte(at, value)),
        (any::<usize>(), 1usize..256).prop_map(|(at, len)| Damage::Zeroed(at, len)),
        any::<usize>().prop_map(Damage::Cut),
    ]
}

/// One of the made models, damaged. Random bytes come after the start of a model of its kind,
/// or alone.
fn damaged(damage: &Damage, which: usize) -> Vec<u8> {
    let made = [grid(), boxes(), cloud(), text_cloud(), jpeg_grid()];
    let mut bytes = made[which % made.len()].to_vec();
    match damage {
        Damage::Random(random) => {
            let start: &[u8] = match which % 3 {
                0 => b"",
                1 => &bytes[..12],
                _ => b"ply\nformat binary_little_endian 1.0\n",
            };
            return [start, random].concat();
        }
        Damage::RandomDescription(random) => {
            let mut out = b"glTF".to_vec();
            out.extend_from_slice(&2u32.to_le_bytes());
            out.extend_from_slice(&(20 + random.len() as u32).to_le_bytes());
            out.extend_from_slice(&(random.len() as u32).to_le_bytes());
            out.extend_from_slice(b"JSON");
            out.extend_from_slice(random);
            return out;
        }
        Damage::RandomData(random) => {
            let glb = [grid(), boxes(), jpeg_grid()][which % 3];
            let (text, bin) = glb_parts(glb).unwrap();
            let bin: Vec<u8> = random.iter().copied().cycle().take(bin.len()).collect();
            return glb_from(&text, &bin);
        }
        Damage::Number(at, value) => {
            let glb = [grid(), boxes()][which % 2];
            let (text, bin) = glb_parts(glb).unwrap();
            let numbers: Vec<(usize, usize)> = text
                .char_indices()
                .filter(|(i, c)| c.is_ascii_digit() && !text[..*i].ends_with(|c: char| c.is_ascii_digit() || c == '.'))
                .map(|(i, _)| (i, text[i..].find(|c: char| !c.is_ascii_digit()).map_or(text.len(), |n| i + n)))
                .collect();
            let (start, end) = numbers[at % numbers.len()];
            return glb_from(&format!("{}{value}{}", &text[..start], &text[end..]), &bin);
        }
        Damage::Byte(at, value) => {
            let at = at % bytes.len();
            bytes[at] = *value;
        }
        Damage::Zeroed(at, len) => {
            let at = at % bytes.len();
            let end = (at + len).min(bytes.len());
            bytes[at..end].fill(0);
        }
        Damage::Cut(at) => bytes.truncate(at % bytes.len()),
    }
    bytes
}

/// A modest number of cases, so this runs in seconds with the rest; `PROPTEST_CASES` asks for
/// more.
fn cases() -> u32 {
    std::env::var("PROPTEST_CASES").ok().and_then(|cases| cases.parse().ok()).unwrap_or(96)
}

proptest! {
    #![proptest_config(ProptestConfig { cases: cases(), ..ProptestConfig::default() })]

    #[test]
    fn any_bytes_read_or_say_why(damage in damage(), which in 0usize..12) {
        let _ = facts_and_read(damaged(&damage, which));
    }
}

#[test]
fn any_bytes_read_or_say_why_named_cases() {
    assert_eq!(facts_and_read(Vec::new()).err(), Some(CantDraw::NotAModel));
    assert_eq!(facts_and_read(b"A note, named as a model by mistake.\n".repeat(40)).err(), Some(CantDraw::NotAModel));
    assert_eq!(facts_and_read(b"glTF".to_vec()).err(), Some(CantDraw::Damaged));
    assert!(facts_and_read(grid().to_vec()).is_ok());
    assert!(facts_and_read(boxes().to_vec()).is_ok());

    // A texture whose header says 60,000 by 60,000.
    let huge_texture = edited(grid(), |json, bin| {
        let view = json["images"][0]["bufferView"].as_u64().unwrap() as usize;
        let at = json["bufferViews"][view]["byteOffset"].as_u64().unwrap_or(0) as usize;
        assert_eq!(&bin[at + 12..at + 16], b"IHDR");
        bin[at + 16..at + 20].copy_from_slice(&60_000u32.to_be_bytes());
        bin[at + 20..at + 24].copy_from_slice(&60_000u32.to_be_bytes());
    });
    assert_eq!(facts(reader(huge_texture.clone())).err(), Some(CantDraw::TooLarge));
    assert_eq!(facts_and_read(huge_texture).err(), Some(CantDraw::TooLarge));
    // One a pixel wider than the most, and ones under it on each side but too many in all.
    let sized = |width: u32, height: u32| {
        edited(grid(), |json, bin| {
            let view = json["images"][0]["bufferView"].as_u64().unwrap() as usize;
            let at = json["bufferViews"][view]["byteOffset"].as_u64().unwrap_or(0) as usize;
            bin[at + 16..at + 20].copy_from_slice(&u32::to_be_bytes(width));
            bin[at + 20..at + 24].copy_from_slice(&u32::to_be_bytes(height));
        })
    };
    for (width, height) in [(MOST_TEXTURE_SIDE + 1, 16), (16, MOST_TEXTURE_SIDE + 1), (8192, 8192), (8000, 8200)] {
        assert!(u64::from(width) * u64::from(height) > MOST_TEXTURE_PIXELS || width.max(height) > MOST_TEXTURE_SIDE);
        assert_eq!(facts_and_read(sized(width, height)).err(), Some(CantDraw::TooLarge), "{width} by {height}");
    }
    // Within the limits, the header is believed, and the picture found damaged when decoded.
    let within = sized(4096, 4096);
    assert!(facts(reader(within.clone())).is_ok());
    assert_eq!(facts_and_read(within).err(), Some(CantDraw::Damaged));

    // A PNG of 16 bits a number and four numbers a pixel, whose well made header says 8,000 by
    // 8,000, in a file of a few kilobytes: it's never held at 16 bits, and what's taken to
    // decode it stays under the most.
    let deep_png = with_texture(&resized_png(png_of_16_bits(), 8000, 8000));
    let started = Instant::now();
    assert!(facts(reader(deep_png.clone())).is_ok());
    assert_eq!(facts_and_read(deep_png).err(), Some(CantDraw::Damaged));
    assert!(started.elapsed() < MOST_TIME);
    // A small one of 16 bits reads, made 8.
    let small = read(reader(with_texture(&png_of_16_bits()))).unwrap().texture.unwrap();
    assert_eq!((small.width, small.height), (4, 4));
    assert_eq!(texture_colour(&small, 0, 0), [255, 128, 0, 255]);
    // A grey PNG is spread to RGBA.
    let mut grey = Cursor::new(Vec::new());
    image::GrayImage::from_pixel(3, 2, image::Luma([90])).write_to(&mut grey, image::ImageFormat::Png).unwrap();
    let grey = read(reader(with_texture(grey.get_ref()))).unwrap().texture.unwrap();
    assert_eq!(grey.rgba, [90, 90, 90, 255].repeat(6));
    // A JPEG with markers that stand alone among its parts, where they don't belong: the
    // decoder would read a length after them, so they're damage, as every walk here takes them.
    let jpeg = make_test_glb(MadeGlb::Grid { cells: 2, texture: 32, jpeg: true });
    let (text, bin) = glb_parts(&jpeg).unwrap();
    let json: Value = serde_json::from_str(&text).unwrap();
    let view = &json["bufferViews"][json["images"][0]["bufferView"].as_u64().unwrap() as usize];
    let at = view["byteOffset"].as_u64().unwrap() as usize;
    let picture = bin[at..at + view["byteLength"].as_u64().unwrap() as usize].to_vec();
    for alone in [[0xFF, 0x01], [0xFF, 0xD0], [0xFF, 0xD7], [0xFF, 0xD8]] {
        let marked = with_texture(&[&picture[..2], &alone, &picture[2..]].concat());
        assert_eq!(facts(reader(marked.clone())).err(), Some(CantDraw::Damaged), "{alone:x?}");
        assert_eq!(facts_and_read(marked).err(), Some(CantDraw::Damaged));
    }
    // Fill bytes before a marker are passed over.
    let filled = with_texture(&[&picture[..2], &[0xFF; 5], &picture[2..]].concat());
    assert_eq!(facts_and_read(filled).unwrap().texture.map(|t| (t.width, t.height)), Some((32, 32)));
    // However long the fill runs, even to the end of a piece read, it is passed over.
    for run in [65_533, 65_534, 65_535, 65_536, 131_071] {
        let filled = with_texture(&[&picture[..2], &vec![0xFF; run], &picture[2..]].concat());
        assert_eq!(facts_and_read(filled).unwrap().texture.map(|t| (t.width, t.height)), Some((32, 32)), "{run}");
    }
    // A JPEG cut short, in its last scan or just before its end, is damaged.
    let whole = progressive_jpeg(64, 64, 3);
    for cut in [whole.len() - 2, whole.len() - 3, whole.len() / 2] {
        assert_eq!(facts_and_read(with_texture(&whole[..cut])).err(), Some(CantDraw::Damaged), "cut at {cut}");
    }
    assert!(facts_and_read(with_texture(&whole)).is_ok());
    // A grey frame said first, a marker standing alone that the decoder would read a length
    // after, landing it past the grey frame on a colour one twice the size: budgeted from one
    // and decoded as the other, unless both agree.
    let grey_frame = [0xFF, 0xC0, 0, 11, 8, 0x1B, 0x58, 0x1B, 0x58, 1, 1, 0x11, 0];
    let mut desync = vec![0xFF, 0xD8, 0xFF, 0xD0];
    desync.extend_from_slice(&grey_frame);
    desync.extend_from_slice(&[0xFF, 0xE0]);
    desync.extend_from_slice(&65_476u16.to_be_bytes());
    desync.resize(desync.len() + 65_474, 0);
    desync.extend_from_slice(&progressive_jpeg(7000, 7000, 3)[2..]);
    let desync = with_texture(&desync);
    assert!(facts_and_read(desync).is_err());
    // The same without the marker: a grey frame first, then a whole colour JPEG's parts.
    let mut two_frames = vec![0xFF, 0xD8];
    two_frames.extend_from_slice(&grey_frame);
    two_frames.extend_from_slice(&progressive_jpeg(7000, 7000, 3)[2..]);
    assert!(facts_and_read(with_texture(&two_frames)).is_err());
    // A JPEG of 100 MB of fill bytes after its frame is walked through in pieces, quickly.
    let mut start = progressive_jpeg(16, 16, 3);
    let scan = start.windows(2).position(|w| w == [0xFF, 0xDA]).unwrap();
    start.truncate(scan);
    let (start, len) = padded_texture_with(&start, 100_000_000, 0xFF);
    let filled = || Box::new(Virtual::new(&start, len).filled(0xFF));
    let started = Instant::now();
    assert!(facts(filled()).is_ok());
    assert_eq!(read(filled()).err(), Some(CantDraw::Damaged));
    assert!(started.elapsed() < MOST_TIME, "{:?}", started.elapsed());

    // A progressive JPEG of 4:4:4 colour, the kind that takes the most to decode, just within
    // what may be taken, and one a little larger, refused before it's decoded. Each is a few
    // hundred kilobytes, as they're one colour.
    let started = Instant::now();
    let near = with_texture(&progressive_jpeg(5400, 5400, 3));
    let texture = facts_and_read(near).unwrap().texture.unwrap();
    assert_eq!((texture.width, texture.height), (5400, 5400));
    assert!(texture_colour(&texture, 2700, 2700).iter().take(3).all(|v| v.abs_diff(128) <= 2), "{:?}", texture_colour(&texture, 2700, 2700));
    drop(texture);
    eprintln!("A progressive JPEG of 5,400 a side took {:?}.", started.elapsed());
    let over = with_texture(&progressive_jpeg(5600, 5600, 3));
    assert_eq!(facts(reader(over.clone())).err(), Some(CantDraw::TooLarge));
    assert_eq!(facts_and_read(over).err(), Some(CantDraw::TooLarge));
    // Grey takes less, so may be larger.
    let grey = facts_and_read(with_texture(&progressive_jpeg(7000, 7000, 1))).unwrap().texture.unwrap();
    assert_eq!(texture_colour(&grey, 10, 10)[3], 255);
    drop(grey);
    // A small JPEG followed by 240 MB kept beside it: only the picture is read.
    let small = progressive_jpeg(16, 16, 3);
    let (start, len) = padded_texture(&small, 240_000_000);
    let padded = || Box::new(Virtual::new(&start, len));
    PEAK.store(IN_USE.load(Ordering::Relaxed), Ordering::Relaxed);
    assert!(facts(padded()).is_ok());
    let texture = read(padded()).unwrap().texture.unwrap();
    assert_eq!((texture.width, texture.height), (16, 16));
    check_memory();
    assert!(PEAK.load(Ordering::Relaxed) - IN_USE.load(Ordering::Relaxed) < 16 * 1024 * 1024);

    // A billion vertices said in a header of a kilobyte.
    let mut billion = b"ply\nformat binary_little_endian 1.0\nelement vertex 1000000000\nproperty float x\nproperty float y\nproperty float z\nend_header\n".to_vec();
    billion.resize(1024, 0);
    assert!(matches!(facts_and_read(billion.clone()), Err(CantDraw::Damaged | CantDraw::TooLarge)));
    let text_billion = String::from_utf8(billion).unwrap().replace("binary_little_endian", "ascii").into_bytes();
    assert!(matches!(facts_and_read(text_billion), Err(CantDraw::Damaged | CantDraw::TooLarge)));
    // A cloud with more points than are drawn, in a file long enough for them, is too large.
    let many = format!("ply\nformat ascii 1.0\nelement vertex {}\nproperty uchar x\nproperty uchar y\nproperty uchar z\nend_header\n", MOST_POINTS + 1);
    let mut many = many.into_bytes();
    many.resize(many.len() + 10, b' ');
    assert!(matches!(facts(reader(many)), Err(CantDraw::Damaged | CantDraw::TooLarge)));

    // Indices past the vertices.
    let past = edited(boxes(), |json, bin| {
        let accessor = json["meshes"][0]["primitives"][0]["indices"].as_u64().unwrap() as usize;
        let view = json["accessors"][accessor]["bufferView"].as_u64().unwrap() as usize;
        let at = json["bufferViews"][view]["byteOffset"].as_u64().unwrap_or(0) as usize;
        bin[at..at + 2].copy_from_slice(&8u16.to_le_bytes());
    });
    assert_eq!(facts_and_read(past).err(), Some(CantDraw::Damaged));

    // NaN and infinite positions.
    for bad in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
        let nan = edited(boxes(), |json, bin| {
            let accessor = json["meshes"][0]["primitives"][1]["attributes"]["POSITION"].as_u64().unwrap() as usize;
            let view = json["accessors"][accessor]["bufferView"].as_u64().unwrap() as usize;
            let at = json["bufferViews"][view]["byteOffset"].as_u64().unwrap_or(0) as usize;
            bin[at + 16..at + 20].copy_from_slice(&bad.to_le_bytes());
        });
        assert_eq!(facts_and_read(nan).err(), Some(CantDraw::Damaged));
    }

    // Positions that are numbers, placed so far away they aren't.
    let far = edited(boxes(), |json, _| json["nodes"][0] = json!({ "mesh": 0, "scale": [3e38, 3e38, 3e38], "translation": [3e38, 0, 0] }));
    assert_eq!(facts_and_read(far).err(), Some(CantDraw::Damaged));

    // A required extension that isn't known; one that is used but not required is drawn.
    let unknown = edited(boxes(), |json, _| {
        json["extensionsUsed"] = json!(["KHR_draco_mesh_compression"]);
        json["extensionsRequired"] = json!(["KHR_draco_mesh_compression"]);
    });
    assert_eq!(facts_and_read(unknown).err(), Some(CantDraw::KindNotDrawn));
    let used = edited(boxes(), |json, _| json["extensionsUsed"] = json!(["EXT_something_new"]));
    assert!(facts_and_read(used).is_ok());

    // Accessors and views that point past what holds them.
    let past_view = edited(boxes(), |json, _| json["accessors"][0]["byteOffset"] = json!(4));
    assert_eq!(facts_and_read(past_view).err(), Some(CantDraw::Damaged));
    let long_count = edited(boxes(), |json, _| json["accessors"][0]["count"] = json!(4_000_000_000u64));
    assert_eq!(facts_and_read(long_count).err(), Some(CantDraw::Damaged));
    let narrow_stride = edited(boxes(), |json, _| {
        let view = json["accessors"][0]["bufferView"].as_u64().unwrap() as usize;
        json["bufferViews"][view]["byteStride"] = json!(4);
    });
    assert_eq!(facts_and_read(narrow_stride).err(), Some(CantDraw::Damaged));
    let past_buffer = edited(boxes(), |json, _| json["bufferViews"][0]["byteOffset"] = json!(1_000_000));
    assert_eq!(facts_and_read(past_buffer).err(), Some(CantDraw::Damaged));
    let long_buffer = edited(boxes(), |json, _| json["buffers"][0]["byteLength"] = json!(1u64 << 40));
    assert_eq!(facts_and_read(long_buffer).err(), Some(CantDraw::Damaged));
    let no_view = edited(boxes(), |json, _| json["accessors"][0]["bufferView"] = json!(99));
    assert_eq!(facts_and_read(no_view).err(), Some(CantDraw::Damaged));
    let wrong_type = edited(boxes(), |json, _| json["accessors"][0]["componentType"] = json!(9999));
    assert_eq!(facts_and_read(wrong_type).err(), Some(CantDraw::Damaged));

    // Nodes that hold each other.
    let cycle = edited(boxes(), |json, _| {
        json["nodes"] = json!([{ "mesh": 0, "children": [1] }, { "children": [0] }]);
    });
    assert_eq!(facts_and_read(cycle).err(), Some(CantDraw::Damaged));
    // Nodes that each hold the next ten times over: a few nodes, but far too many boxes.
    let fan = edited(boxes(), |json, _| {
        let nodes: Vec<Value> = (0..12).map(|i| json!({ "children": vec![i + 1; 10] })).chain([json!({ "mesh": 0 })]).collect();
        json["nodes"] = Value::Array(nodes);
    });
    assert_eq!(facts_and_read(fan).err(), Some(CantDraw::TooLarge));
    // A node holding itself half a million times, and one holding another as often: a file of a
    // few megabytes whose nodes, followed, would fill memory.
    let held = |by_itself: bool| {
        edited(boxes(), |json, _| {
            let child = if by_itself { 0 } else { 1 };
            json["nodes"] = json!([{ "children": vec![child; 500_000] }, { "mesh": 0 }]);
        })
    };
    assert_eq!(facts_and_read(held(true)).err(), Some(CantDraw::Damaged));
    assert_eq!(facts_and_read(held(false)).err(), Some(CantDraw::TooLarge));
    // A node held by one it holds, further down.
    let deep_cycle = edited(boxes(), |json, _| {
        json["nodes"] = json!([{ "children": [1] }, { "children": [2], "mesh": 0 }, { "children": [1] }]);
    });
    assert_eq!(facts_and_read(deep_cycle).err(), Some(CantDraw::Damaged));
    // Nodes held one inside the next, deeper than is drawn.
    let deep = edited(boxes(), |json, _| {
        let nodes: Vec<Value> = (0..100).map(|i| json!({ "children": [i + 1] })).chain([json!({ "mesh": 0 })]).collect();
        json["nodes"] = Value::Array(nodes);
    });
    assert_eq!(facts_and_read(deep).err(), Some(CantDraw::Damaged));
    // The same node placed many times over by different paths is drawn each time.
    let twice = edited(boxes(), |json, _| json["nodes"] = json!([{ "children": [1, 1] }, { "mesh": 0 }]));
    assert_eq!(facts_and_read(twice).unwrap().meshes.len(), 6);

    // A description of a great many small things.
    let crowded = edited(boxes(), |json, _| json["samplers"] = Value::Array(vec![json!({}); 200_000]));
    assert_eq!(facts_and_read(crowded).err(), Some(CantDraw::TooLarge));
    // A model with nothing in it that can be drawn.
    let lines = edited(boxes(), |json, _| {
        for primitive in json["meshes"][0]["primitives"].as_array_mut().unwrap() {
            primitive["mode"] = json!(1);
        }
    });
    assert_eq!(facts_and_read(lines).err(), Some(CantDraw::KindNotDrawn));

    // Clouds with properties of odd types.
    let header = |properties: &str| format!("ply\nformat ascii 1.0\nelement vertex 1\n{properties}end_header\n1 2 3 4\n").into_bytes();
    assert_eq!(facts_and_read(header("property float x\nproperty float y\nproperty float z\nproperty float128 w\n")).err(), Some(CantDraw::Damaged));
    assert_eq!(facts_and_read(header("property float x\nproperty float y\nproperty list uchar int z\n")).err(), Some(CantDraw::KindNotDrawn));
    assert_eq!(facts_and_read(header("property float x\nproperty float y\nproperty float w\n")).err(), Some(CantDraw::Damaged));
    let numbers = read(reader(header("property int x\nproperty short y\nproperty double z\nproperty uchar red\n"))).unwrap();
    assert_eq!(numbers.points.unwrap().positions, vec![[1.0, 2.0, 3.0]]);
    assert_eq!(facts_and_read(b"ply\nformat ascii 1.0\nelement vertex 1\nproperty float x\nproperty float y\nproperty float z\nend_header\n1 nan 3\n".to_vec()).err(), Some(CantDraw::Damaged));
    assert_eq!(facts_and_read(b"ply\nformat ascii 1.0\nelement vertex 1\nproperty float x\n".to_vec()).err(), Some(CantDraw::Damaged));
    assert_eq!(facts_and_read(b"ply\nformat ascii 1.0\nelement face 1\nproperty list uchar int vertex_indices\nelement vertex 1\nproperty float x\nproperty float y\nproperty float z\nend_header\n3 0 0 0\n1 2 3\n".to_vec()).err(), Some(CantDraw::KindNotDrawn));
    let endless_header = [b"ply\nformat ascii 1.0\n".as_slice(), &b"comment more\n".repeat(10_000)].concat();
    assert!(facts_and_read(endless_header).is_err());

    // Too large a file, without reading it.
    let large = || Box::new(Virtual::new(b"ply\nformat binary_little_endian 1.0\n", MOST_BYTES + 1));
    assert_eq!(facts(large()).err(), Some(CantDraw::TooLarge));
    assert_eq!(read(large()).err(), Some(CantDraw::TooLarge));

    // A cloud of a point more than the most, in a file long enough for them all.
    let header = format!(
        "ply\nformat binary_little_endian 1.0\nelement vertex {}\nproperty uchar x\nproperty uchar y\nproperty uchar z\nend_header\n",
        MOST_POINTS + 1
    );
    let cloud = || Box::new(Virtual::new(header.as_bytes(), header.len() as u64 + 3 * (MOST_POINTS + 1)));
    assert_eq!(facts(cloud()).err(), Some(CantDraw::TooLarge));
    assert_eq!(read(cloud()).err(), Some(CantDraw::TooLarge));
    // And one of the most, which is said as it is.
    let header = header.replace(&(MOST_POINTS + 1).to_string(), &MOST_POINTS.to_string());
    let most = Virtual::new(header.as_bytes(), header.len() as u64 + 3 * MOST_POINTS);
    assert_eq!(facts(Box::new(most)).unwrap().points, MOST_POINTS);
}

/// A reader of a file of `len` bytes that starts with `start` and is nothing but zeros after it,
/// or another byte, without holding it.
struct Virtual {
    start: Vec<u8>,
    len: u64,
    at: u64,
    fill: u8,
}

impl Virtual {
    fn new(start: &[u8], len: u64) -> Virtual {
        Virtual { start: start.to_vec(), len, at: 0, fill: 0 }
    }

    fn filled(self, fill: u8) -> Virtual {
        Virtual { fill, ..self }
    }
}

impl Read for Virtual {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let n = (self.len.saturating_sub(self.at)).min(buf.len() as u64) as usize;
        let from_start = (self.start.len() as u64).saturating_sub(self.at).min(n as u64) as usize;
        if from_start > 0 {
            let at = self.at as usize;
            buf[..from_start].copy_from_slice(&self.start[at..at + from_start]);
        }
        buf[from_start..n].fill(self.fill);
        self.at += n as u64;
        Ok(n)
    }
}

impl Seek for Virtual {
    fn seek(&mut self, pos: SeekFrom) -> std::io::Result<u64> {
        self.at = match pos {
            SeekFrom::Start(at) => at,
            SeekFrom::End(back) => self.len.saturating_add_signed(back),
            SeekFrom::Current(by) => self.at.saturating_add_signed(by),
        };
        Ok(self.at)
    }
}

#[test]
fn nothing_outside_the_file_is_read() {
    // Real files where every name a model might give would find them: beside the working
    // folder, in it, and named in full. Whether a file was read can't be told from its time of
    // reading here, as other programs on a desktop read new files too; that nothing is opened
    // is shown by the source holding no way to open one, below, and what's in them is checked.
    let folder = std::env::temp_dir().join(format!("slides-model-outside-{}", std::process::id()));
    let inner = folder.join("inner");
    std::fs::create_dir_all(&inner).unwrap();
    let mut planted = Vec::new();
    for place in [&folder, &inner] {
        for (name, contents) in [("x.bin", vec![7u8; 4096]), ("x.png", png_of_16_bits())] {
            let path = place.join(name);
            std::fs::write(&path, &contents).unwrap();
            planted.push((path, contents));
        }
    }
    let beside = folder.join("x.bin").to_string_lossy().into_owned();
    let picture = folder.join("x.png").to_string_lossy().into_owned();
    /// Puts the working folder back however the test ends.
    struct Back(std::path::PathBuf);
    impl Drop for Back {
        fn drop(&mut self) {
            let _ = std::env::set_current_dir(&self.0);
        }
    }
    let back = Back(std::env::current_dir().unwrap());
    std::env::set_current_dir(&inner).unwrap();

    let buffer = |uri: &str| {
        let uri = uri.to_owned();
        edited(boxes(), move |json, _| json["buffers"][0]["uri"] = json!(uri))
    };
    let extra_buffer = |uri: &str| {
        let uri = uri.to_owned();
        edited(boxes(), move |json, _| json["buffers"].as_array_mut().unwrap().push(json!({ "uri": uri, "byteLength": 16 })))
    };
    let image = |uri: &str| {
        let uri = uri.to_owned();
        edited(grid(), move |json, _| {
            json["images"][0] = json!({ "uri": uri });
        })
    };
    let data = "data:application/octet-stream;base64,AAAAAAAAAAAAAAAAAAAAAA==";
    let cases = [
        buffer(&beside),
        buffer("x.bin"),
        buffer("../x.bin"),
        buffer(data),
        buffer("https://example.com/x.bin"),
        buffer(&format!("file://{beside}")),
        extra_buffer("x.bin"),
        extra_buffer(data),
        image(&picture),
        image("x.png"),
        image("../x.png"),
        image("https://example.com/x.png"),
        image("data:image/png;base64,iVBORw0KGgo="),
    ];
    for bytes in cases {
        let len = bytes.len() as u64;
        for how in 0..2 {
            let (reader, count) = counted(bytes.clone());
            let said = if how == 0 { facts(reader).err() } else { read(reader).err() };
            assert_eq!(said, Some(CantDraw::NeedsOtherFiles));
            assert!(count.bytes.load(Ordering::Relaxed) <= len);
            assert!(count.furthest.load(Ordering::Relaxed) <= len);
            assert_eq!(count.past_end.load(Ordering::Relaxed), 0);
        }
    }
    drop(back);
    for (path, contents) in &planted {
        assert_eq!(&std::fs::read(path).unwrap(), contents);
    }

    // Nor is a file read past its end when whole and made well.
    for bytes in [grid(), boxes(), cloud(), text_cloud()] {
        let (reader, count) = counted(bytes.to_vec());
        read(reader).unwrap();
        assert_eq!(count.past_end.load(Ordering::Relaxed), 0);
        assert!(count.reads.load(Ordering::Relaxed) > 0);
    }

    // The reading has no way of its own to open a file or reach the network: it's only given
    // the one reader.
    // Its source names none of the ways a program reaches files, the network or other
    // programs, in any form: `std::fs::read`, `use std::{fs, io}`, `fs::File`, the picture
    // library's `image::open` and the rest.
    let source = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let forbidden = [
        "fs", "File", "OpenOptions", "DirBuilder", "read_dir", "net", "TcpStream", "UdpSocket", "process", "Command", "env",
        "set_current_dir", "include_bytes", "include_str", "libc", "mmap", "open", "ImageReader", "load_from_memory",
    ];
    let mut looked = 0;
    let mut folders = vec![source];
    while let Some(folder) = folders.pop() {
        for entry in std::fs::read_dir(folder).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                folders.push(path);
                continue;
            }
            if path.extension().is_none_or(|e| e != "rs") {
                continue;
            }
            let text = std::fs::read_to_string(&path).unwrap();
            for word in text.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_')) {
                assert!(!forbidden.contains(&word), "{} names {word}", path.display());
            }
            looked += 1;
        }
    }
    assert!(looked >= 5);
    std::fs::remove_dir_all(&folder).unwrap();
}

#[test]
fn each_refusal_reads_in_plain_words() {
    let all = [
        CantDraw::NotAModel,
        CantDraw::Damaged,
        CantDraw::NeedsOtherFiles,
        CantDraw::KindNotDrawn,
        CantDraw::TooLarge,
        CantDraw::CouldNotRead,
        CantDraw::NoAdapter,
        CantDraw::CardRefused,
    ];
    let said: Vec<String> = all.iter().map(|why| why.to_string()).collect();
    for words in &said {
        assert!(words.starts_with(|c: char| c.is_uppercase()) && words.ends_with('.'), "{words}");
        assert!(words.len() < 200, "{words}");
        for odd in ["<", ">", "_", "::", "/", "\\", "Err", "{", "glTF", "PLY"] {
            assert!(!words.contains(odd), "{words} has {odd}");
        }
        assert!(words.contains("model") || words.contains("3D"), "{words}");
    }
    for (i, a) in said.iter().enumerate() {
        assert!(said[i + 1..].iter().all(|b| b != a), "{a} is said twice");
    }
    // A file named by its kind has plain words beside it.
    assert!(CantDraw::NeedsOtherFiles.to_string().contains("one file"));
    assert_eq!(
        CantDraw::NoAdapter.to_string(),
        "This computer can't draw 3D models here. The slide shows a still when it's presented somewhere that can."
    );
}
