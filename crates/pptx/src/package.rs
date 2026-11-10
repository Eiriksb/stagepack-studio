//! OPC package writer. The only way to add a part is through `add_xml`/`add_media`, which also
//! registers its content type, and `[Content_Types].xml` and every `_rels/*.rels` are generated
//! from what was actually added, so a phantom Override (pptxgenjs#1449) or a relationship to a
//! part that doesn't exist can't be written. Same calls in the same order give the same bytes.
//!
//! The JavaScript throws on a bad call. Here the first bad call is remembered and `to_bytes`
//! returns it, so building a package never panics and the caller handles one error at the end.

use std::collections::HashSet;
use std::io::{Cursor, Write};

use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, DateTime, ZipWriter};

use crate::xml::{NS_CT, NS_RELS, XML_DECL, esc};

/// Relationship types.
pub struct REL;

macro_rules! office_rel {
    ($name:literal) => {
        concat!("http://schemas.openxmlformats.org/officeDocument/2006/relationships/", $name)
    };
}

impl REL {
    pub const OFFICE_DOCUMENT: &'static str = office_rel!("officeDocument");
    pub const CORE_PROPS: &'static str =
        "http://schemas.openxmlformats.org/package/2006/relationships/metadata/core-properties";
    pub const EXT_PROPS: &'static str = office_rel!("extended-properties");
    pub const SLIDE_MASTER: &'static str = office_rel!("slideMaster");
    pub const SLIDE_LAYOUT: &'static str = office_rel!("slideLayout");
    pub const SLIDE: &'static str = office_rel!("slide");
    pub const THEME: &'static str = office_rel!("theme");
    pub const NOTES_MASTER: &'static str = office_rel!("notesMaster");
    pub const NOTES_SLIDE: &'static str = office_rel!("notesSlide");
    pub const PRES_PROPS: &'static str = office_rel!("presProps");
    pub const VIEW_PROPS: &'static str = office_rel!("viewProps");
    pub const TABLE_STYLES: &'static str = office_rel!("tableStyles");
    pub const IMAGE: &'static str = office_rel!("image");
    pub const FONT: &'static str = office_rel!("font");
    pub const HYPERLINK: &'static str = office_rel!("hyperlink");
}

macro_rules! pml {
    ($name:literal) => {
        concat!("application/vnd.openxmlformats-officedocument.presentationml.", $name)
    };
}

/// Content types.
pub struct CT;

impl CT {
    pub const PRESENTATION: &'static str = pml!("presentation.main+xml");
    pub const SLIDE_MASTER: &'static str = pml!("slideMaster+xml");
    pub const SLIDE_LAYOUT: &'static str = pml!("slideLayout+xml");
    pub const SLIDE: &'static str = pml!("slide+xml");
    pub const NOTES_MASTER: &'static str = pml!("notesMaster+xml");
    pub const NOTES_SLIDE: &'static str = pml!("notesSlide+xml");
    pub const PRES_PROPS: &'static str = pml!("presProps+xml");
    pub const VIEW_PROPS: &'static str = pml!("viewProps+xml");
    pub const TABLE_STYLES: &'static str = pml!("tableStyles+xml");
    pub const THEME: &'static str = "application/vnd.openxmlformats-officedocument.theme+xml";
    pub const CORE_PROPS: &'static str = "application/vnd.openxmlformats-package.core-properties+xml";
    pub const EXT_PROPS: &'static str = "application/vnd.openxmlformats-officedocument.extended-properties+xml";
}

const MEDIA_TYPES: [(&str, &str); 6] = [
    ("png", "image/png"),
    ("jpg", "image/jpeg"),
    ("jpeg", "image/jpeg"),
    ("gif", "image/gif"),
    ("svg", "image/svg+xml"),
    ("fntdata", "application/x-fontdata"),
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PackageError {
    InvalidPartName(String),
    DuplicatePart(String),
    UnsupportedMedia(String),
    MissingRelationshipSource(String),
    MissingRelationshipTarget { id: String, source: String, target: String },
    Zip(String),
}

impl std::fmt::Display for PackageError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PackageError::InvalidPartName(n) => write!(f, "invalid part name {n:?}"),
            PackageError::DuplicatePart(n) => write!(f, "duplicate part {n}"),
            PackageError::UnsupportedMedia(n) => write!(f, "unsupported media type for {n}"),
            PackageError::MissingRelationshipSource(n) => {
                write!(f, "relationships source part {n} was never added")
            }
            PackageError::MissingRelationshipTarget { id, source, target } => {
                write!(f, "relationship {id} from {source:?} points at missing part {target}")
            }
            PackageError::Zip(e) => write!(f, "writing the zip failed: {e}"),
        }
    }
}

impl std::error::Error for PackageError {}

struct Part {
    name: String,
    data: Vec<u8>,
    compress: bool,
}

struct Relationship {
    id: String,
    kind: String,
    target: String,
    external: bool,
}

/// `/_rels/.rels` for the package root, otherwise `<dir>/_rels/<file>.rels`.
fn rels_part_for(source: &str) -> String {
    if source.is_empty() {
        return "/_rels/.rels".to_string();
    }
    let i = source.rfind('/').unwrap_or(0);
    format!("{}/_rels/{}.rels", &source[..i], &source[i + 1..])
}

fn relative_target(from: &str, to: &str) -> String {
    let from_dir: Vec<&str> = from.split('/').skip(1).collect();
    let from_dir = &from_dir[..from_dir.len().saturating_sub(1)];
    let to: Vec<&str> = to.split('/').skip(1).collect();
    let mut i = 0;
    while i < from_dir.len() && i + 1 < to.len() && from_dir[i] == to[i] {
        i += 1;
    }
    format!("{}{}", "../".repeat(from_dir.len() - i), to[i..].join("/"))
}

/// JavaScript's `\s`, which `char::is_whitespace` misses the byte order mark in.
fn is_js_space(c: char) -> bool {
    c.is_whitespace() || c == '\u{FEFF}'
}

/// A part name is `/`, one or more non-space characters, and a last character that isn't `/`.
fn valid_part_name(name: &str) -> bool {
    let chars: Vec<char> = name.chars().collect();
    chars.len() >= 3
        && chars[0] == '/'
        && chars[chars.len() - 1] != '/'
        && chars[1..chars.len() - 1].iter().all(|c| !is_js_space(*c))
}

pub struct Package {
    parts: Vec<Part>,
    names: HashSet<String>,
    overrides: Vec<(String, String)>,
    defaults: Vec<(String, String)>,
    /// Source part (`""` is the package root) and its relationships, in first-use order.
    rels: Vec<(String, Vec<Relationship>)>,
    error: Option<PackageError>,
}

impl Default for Package {
    fn default() -> Package {
        Package {
            parts: Vec::new(),
            names: HashSet::new(),
            overrides: Vec::new(),
            defaults: vec![
                ("rels".into(), "application/vnd.openxmlformats-package.relationships+xml".into()),
                ("xml".into(), "application/xml".into()),
            ],
            rels: Vec::new(),
            error: None,
        }
    }
}

impl Package {
    fn fail(&mut self, error: PackageError) {
        self.error.get_or_insert(error);
    }

    /// Takes a new part's name, or records why it can't have it.
    fn claim(&mut self, name: &str) -> bool {
        if !valid_part_name(name) {
            self.fail(PackageError::InvalidPartName(name.to_string()));
            return false;
        }
        if !self.names.insert(name.to_string()) {
            self.fail(PackageError::DuplicatePart(name.to_string()));
            return false;
        }
        true
    }

    pub fn add_xml(&mut self, part_name: &str, content_type: &str, xml: &str) {
        if !self.claim(part_name) {
            return;
        }
        self.parts.push(Part { name: part_name.to_string(), data: xml.as_bytes().to_vec(), compress: true });
        self.overrides.push((part_name.to_string(), content_type.to_string()));
    }

    pub fn add_media(&mut self, part_name: &str, data: Vec<u8>) {
        let ext = part_name.rsplit('.').next().unwrap_or("").to_lowercase();
        let Some((_, media_type)) = MEDIA_TYPES.iter().find(|(e, _)| *e == ext) else {
            // The name is checked first, like the original, so its error is the one reported.
            if self.claim(part_name) {
                self.fail(PackageError::UnsupportedMedia(part_name.to_string()));
            }
            return;
        };
        if !self.claim(part_name) {
            return;
        }
        // Already-compressed binaries are stored, not deflated again.
        self.parts.push(Part { name: part_name.to_string(), data, compress: false });
        if !self.defaults.iter().any(|(e, _)| *e == ext) {
            self.defaults.push((ext, media_type.to_string()));
        }
    }

    /// The bytes of a part added so far.
    pub fn data(&self, part_name: &str) -> Option<&[u8]> {
        self.parts.iter().find(|p| p.name == part_name).map(|p| p.data.as_slice())
    }

    /// The text of the XML parts added so far (a part that isn't valid UTF-8 is left out).
    pub fn xml_parts(&self) -> impl Iterator<Item = &str> {
        self.parts.iter().filter(|p| p.compress).filter_map(|p| std::str::from_utf8(&p.data).ok())
    }

    pub fn has(&self, part_name: &str) -> bool {
        self.names.contains(part_name)
    }

    /// The rId of a relationship from `from_part` to `target`, reusing an identical one.
    pub fn rel(&mut self, from_part: &str, target: &str, kind: &str, external: bool) -> String {
        let index = match self.rels.iter().position(|(source, _)| source == from_part) {
            Some(i) => i,
            None => {
                self.rels.push((from_part.to_string(), Vec::new()));
                self.rels.len() - 1
            }
        };
        let list = &mut self.rels[index].1;
        if let Some(found) = list.iter().find(|r| r.kind == kind && r.target == target && r.external == external) {
            return found.id.clone();
        }
        let id = format!("rId{}", list.len() + 1);
        list.push(Relationship { id: id.clone(), kind: kind.to_string(), target: target.to_string(), external });
        id
    }

    fn content_types_xml(&self) -> String {
        let defaults: String = self
            .defaults
            .iter()
            .map(|(ext, ty)| format!("<Default Extension=\"{}\" ContentType=\"{}\"/>", esc(ext), esc(ty)))
            .collect();
        let overrides: String = self
            .overrides
            .iter()
            .map(|(name, ty)| format!("<Override PartName=\"{}\" ContentType=\"{}\"/>", esc(name), esc(ty)))
            .collect();
        format!("{XML_DECL}<Types xmlns=\"{NS_CT}\">{defaults}{overrides}</Types>")
    }

    fn rels_xml(source: &str, list: &[Relationship]) -> String {
        let items: String = list
            .iter()
            .map(|r| {
                let target = if r.external { r.target.clone() } else { relative_target(source, &r.target) };
                let mode = if r.external { " TargetMode=\"External\"" } else { "" };
                format!(
                    "<Relationship Id=\"{}\" Type=\"{}\" Target=\"{}\"{mode}/>",
                    r.id,
                    esc(&r.kind),
                    esc(&target)
                )
            })
            .collect();
        format!("{XML_DECL}<Relationships xmlns=\"{NS_RELS}\">{items}</Relationships>")
    }

    /// The whole package as a zip. Entries carry the DOS epoch as their time, the earliest a zip
    /// entry can have, so the output is reproducible. `[Content_Types].xml` comes first.
    pub fn to_bytes(&self) -> Result<Vec<u8>, PackageError> {
        if let Some(error) = &self.error {
            return Err(error.clone());
        }
        for (source, list) in &self.rels {
            // A source that was never added would otherwise be dropped silently below (the part
            // loop only visits parts that exist) instead of getting the .rels file it needs.
            if !source.is_empty() && !self.has(source) {
                return Err(PackageError::MissingRelationshipSource(source.clone()));
            }
            for r in list {
                if !r.external && !self.has(&r.target) {
                    return Err(PackageError::MissingRelationshipTarget {
                        id: r.id.clone(),
                        source: source.clone(),
                        target: r.target.clone(),
                    });
                }
            }
        }
        let zip_error = |e: &dyn std::fmt::Display| PackageError::Zip(e.to_string());
        let rels_of = |source: &str| self.rels.iter().find(|(s, _)| s == source).map(|(_, l)| l);

        let epoch = DateTime::from_date_and_time(1980, 1, 1, 0, 0, 0).map_err(|e| zip_error(&e))?;
        let options = |compress: bool| {
            let method = if compress { CompressionMethod::Deflated } else { CompressionMethod::Stored };
            let o = SimpleFileOptions::default().compression_method(method).last_modified_time(epoch);
            if compress { o.compression_level(Some(6)) } else { o }
        };
        let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
        let mut add = |name: &str, data: &[u8], compress: bool| -> Result<(), PackageError> {
            zip.start_file(&name[1..], options(compress)).map_err(|e| zip_error(&e))?;
            zip.write_all(data).map_err(|e| zip_error(&e))
        };

        add("/[Content_Types].xml", self.content_types_xml().as_bytes(), true)?;
        if let Some(list) = rels_of("") {
            add(&rels_part_for(""), Self::rels_xml("", list).as_bytes(), true)?;
        }
        for part in &self.parts {
            add(&part.name, &part.data, part.compress)?;
            if let Some(list) = rels_of(&part.name).filter(|l| !l.is_empty()) {
                add(&rels_part_for(&part.name), Self::rels_xml(&part.name, list).as_bytes(), true)?;
            }
        }
        let cursor = zip.finish().map_err(|e| zip_error(&e))?;
        Ok(cursor.into_inner())
    }
}
