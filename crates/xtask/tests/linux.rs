//! The Linux package's scripts, run against a scratch home. Nothing here touches the real home:
//! every run sets HOME and every XDG_* folder inside a temporary folder.
//!
//! The staged app (about 560 MB, ~8 minutes to build from scratch) is made once and reused: set
//! `STAGED_APP` to a folder made by `package stage` to use that one; with it unset the tests use
//! `target/staged-app` and stage it there (with `target/bundle`) when it isn't there yet.
//! `scripts/cargo-safe.sh test -p xtask --test linux`; the tarball test is `-- --ignored`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::OnceLock;
use xtask::stage::repo_root;

const TYPE: &str = "application/vnd.stagepack.talk";

fn staged() -> &'static Path {
    static STAGED: OnceLock<PathBuf> = OnceLock::new();
    STAGED.get_or_init(|| {
        let dir = match std::env::var_os("STAGED_APP") {
            Some(dir) => PathBuf::from(dir),
            None => repo_root().join("target/staged-app"),
        };
        if !dir.join("stagepack-studio").is_file() {
            let bundle = repo_root().join("target/bundle");
            xtask::stage::stage(&dir, Some(&bundle)).expect("staging the app");
        }
        dir
    })
}

/// The package folder (the staged app plus the install files), made once; hard-linked, never written to.
fn package() -> &'static Path {
    static PACKAGE: OnceLock<PathBuf> = OnceLock::new();
    PACKAGE.get_or_init(|| {
        let dir = repo_root().join("target/linux-test/stagepack-studio-pkg");
        xtask::linux::assemble(staged(), &dir).expect("assembling the package folder");
        dir
    })
}

/// A scratch home with every XDG folder inside it.
struct Scratch {
    _dir: tempfile::TempDir,
    root: PathBuf,
    home: PathBuf,
    data: PathBuf,
    config: PathBuf,
    cache: PathBuf,
    bin: PathBuf,
    path_prefix: Option<PathBuf>,
}

impl Scratch {
    fn new(prefix: &str) -> Scratch {
        let dir = tempfile::Builder::new().prefix(prefix).tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        let home = root.join("home");
        std::fs::create_dir_all(&home).unwrap();
        Scratch {
            data: home.join(".local/share"),
            config: home.join(".config"),
            cache: home.join(".cache"),
            bin: home.join(".local/bin"),
            home,
            root,
            _dir: dir,
            path_prefix: None,
        }
    }
    fn app(&self) -> PathBuf {
        self.home.join(".local/lib/stagepack-studio")
    }
    /// A command with nothing from the real environment: only the scratch folders.
    fn command(&self, program: impl AsRef<std::ffi::OsStr>) -> Command {
        let mut command = Command::new(program);
        let system = "/usr/local/bin:/usr/bin:/bin";
        let path = match &self.path_prefix {
            Some(prefix) => format!("{}:{system}", prefix.display()),
            None => system.to_string(),
        };
        command
            .env_clear()
            .env("HOME", &self.home)
            .env("XDG_DATA_HOME", &self.data)
            .env("XDG_CONFIG_HOME", &self.config)
            .env("XDG_CACHE_HOME", &self.cache)
            .env("XDG_BIN_HOME", &self.bin)
            .env("XDG_DATA_DIRS", format!("{}:/usr/share", self.data.display()))
            .env("XDG_CURRENT_DESKTOP", "")
            .env("PATH", path)
            .env("LANG", "C.UTF-8");
        command
    }
    fn run_script(&self, script: &Path) -> Output {
        self.command("sh").arg(script).output().unwrap()
    }
    fn install(&self, folder: &Path) -> Output {
        self.run_script(&folder.join("install.sh"))
    }
    fn uninstall(&self) -> Output {
        self.run_script(&self.app().join("uninstall.sh"))
    }
    fn xdg_mime(&self, args: &[&str]) -> String {
        let out = self.command("xdg-mime").args(args).output().unwrap();
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }
}

fn said(output: &Output) -> String {
    format!("{}{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr))
}

/// Every path under `root` with what identifies it: a file's bytes, a link's target, a folder.
fn snapshot(root: &Path) -> BTreeMap<String, String> {
    fn walk(root: &Path, dir: &Path, into: &mut BTreeMap<String, String>) {
        let Ok(entries) = std::fs::read_dir(dir) else { return };
        for entry in entries {
            let path = entry.unwrap().path();
            let name = path.strip_prefix(root).unwrap().display().to_string();
            let meta = std::fs::symlink_metadata(&path).unwrap();
            if meta.is_dir() {
                into.insert(name, "dir".into());
                walk(root, &path, into);
            } else if meta.is_symlink() {
                into.insert(name, format!("link {}", std::fs::read_link(&path).unwrap().display()));
            } else {
                into.insert(name, format!("file {}", meta.len()) + &format!(" {:?}", std::fs::read(&path).map(|b| b.iter().fold(0u64, |h, &x| h.wrapping_mul(31).wrapping_add(x as u64)))));
            }
        }
    }
    let mut into = BTreeMap::new();
    walk(root, root, &mut into);
    into
}

/// What a home holds that an uninstall must leave as it found it: files and links, not folders, and
/// not what the system's own tools generate (the mime database and the menu cache). Folders made for
/// our files may stay, which is the rule: the only folder uninstall removes is the program's own.
fn kept(snapshot: BTreeMap<String, String>) -> BTreeMap<String, String> {
    snapshot.into_iter().filter(|(k, v)| v != "dir" && !k.starts_with(".local/share/mime/") && !k.ends_with("applications/mimeinfo.cache")).collect()
}

#[test]
fn install_puts_everything_in_its_place() {
    install_in(Scratch::new("plain-home-"), false);
    // A home with a space in its path: the menu entry's Exec path must be quoted. (xdg-mime itself splits
    // XDG_DATA_DIRS at spaces, so its answers are only checked for the plain home; here the files are.)
    install_in(Scratch::new("home with space "), true);
}

fn install_in(s: Scratch, spaced: bool) {
    let pristine = snapshot(&s.home);
    let out = s.install(package());
    assert_eq!(out.status.code(), Some(0), "{}", said(&out));
    let app = s.app();
    assert!(app.join("stagepack-studio").is_file() && app.join("stagepack-terminal").is_file());
    assert!(app.join("bundle/bin/rustc").exists() && app.join("uninstall.sh").is_file());
    assert!(app.join("install-manifest.txt").is_file());
    assert_eq!(std::fs::read_link(s.bin.join("stagepack-studio")).unwrap(), app.join("stagepack-studio"));

    let desktop = s.data.join("applications/stagepack-studio.desktop");
    let entry = std::fs::read_to_string(&desktop).unwrap();
    let exec = app.join("stagepack-studio");
    let expected = if spaced { format!("Exec=\"{}\" %f\n", exec.display()) } else { format!("Exec={} %f\n", exec.display()) };
    assert!(entry.contains(&expected), "{entry}");
    for line in ["MimeType=application/vnd.stagepack.talk;application/x-stagepack;", "Icon=stagepack-studio", "Categories=Office;Presentation;", "Terminal=false"] {
        assert!(entry.lines().any(|l| l == line), "{line} is missing from\n{entry}");
    }
    let valid = Command::new("desktop-file-validate").arg(&desktop).output().unwrap();
    assert!(valid.status.success(), "{}", said(&valid));

    assert!(s.data.join("mime/packages/stagepack-studio.xml").is_file());
    assert!(s.data.join("mime/globs2").is_file(), "the mime database wasn't rebuilt");
    for n in [16, 24, 32, 48, 64, 128, 256, 512] {
        assert!(s.data.join(format!("icons/hicolor/{n}x{n}/apps/stagepack-studio.png")).is_file(), "icon {n}");
        assert!(s.data.join(format!("icons/hicolor/{n}x{n}/mimetypes/application-vnd.stagepack.talk.png")).is_file(), "type icon {n}");
    }
    if spaced {
        let list = std::fs::read_to_string(s.config.join("mimeapps.list")).unwrap();
        assert!(list.lines().any(|l| l == format!("{TYPE}=stagepack-studio.desktop")), "{list}");
    } else {
        assert_eq!(s.xdg_mime(&["query", "default", TYPE]), "stagepack-studio.desktop");
        let talk = s.root.join("a talk.sptx");
        std::fs::write(&talk, b"PK\x03\x04 not really a zip").unwrap();
        assert_eq!(s.xdg_mime(&["query", "filetype", talk.to_str().unwrap()]), TYPE);
    }

    // The link runs the program, with no Rust around and nothing but the scratch home.
    let help = s.command(s.bin.join("stagepack-studio")).arg("help").output().unwrap();
    assert_eq!(help.status.code(), Some(0), "{}", said(&help));
    assert!(!said(&help).is_empty());
    assert!(!s.data.join("stagepack-studio").exists(), "the data folder must not be made by installing");
    assert!(said(&out).contains("isn't on your PATH"), "the PATH note: {}", said(&out));

    // Taking it away leaves the home as it was.
    let out = s.uninstall();
    assert_eq!(out.status.code(), Some(0), "{}", said(&out));
    assert_eq!(kept(snapshot(&s.home)), kept(pristine));
}

/// The environment changes a refusal case makes: a variable set, or removed (`None`).
type Env = Vec<(&'static str, Option<PathBuf>)>;

#[test]
fn install_refuses_what_it_shouldnt_touch() {
    let pkg = package();
    // Each case changes a fresh scratch home and must leave it exactly as it was.
    type Case = (&'static str, fn(&mut Scratch) -> Env);
    let cases: [Case; 24] = [
        ("HOME unset", |_| vec![("HOME", None)]),
        ("HOME empty", |_| vec![("HOME", Some("".into()))]),
        ("HOME is /", |_| vec![("HOME", Some("/".into()))]),
        ("HOME relative", |_| vec![("HOME", Some("home".into()))]),
        ("run as root", |s| {
            let shim = s.root.join("shim");
            std::fs::create_dir_all(&shim).unwrap();
            std::fs::write(shim.join("id"), "#!/bin/sh\necho 0\n").unwrap();
            std::fs::set_permissions(shim.join("id"), std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
            s.path_prefix = Some(shim);
            vec![]
        }),
        ("relative XDG_DATA_HOME", |_| vec![("XDG_DATA_HOME", Some("data".into()))]),
        ("relative XDG_CONFIG_HOME", |_| vec![("XDG_CONFIG_HOME", Some("./config".into()))]),
        ("relative XDG_CACHE_HOME", |_| vec![("XDG_CACHE_HOME", Some("cache".into()))]),
        ("relative XDG_BIN_HOME", |_| vec![("XDG_BIN_HOME", Some("bin".into()))]),
        ("the app folder is a file", |s| {
            std::fs::create_dir_all(s.app().parent().unwrap()).unwrap();
            std::fs::write(s.app(), "in the way").unwrap();
            vec![]
        }),
        ("the app folder is a link", |s| {
            std::fs::create_dir_all(s.app().parent().unwrap()).unwrap();
            std::fs::create_dir_all(s.root.join("elsewhere")).unwrap();
            std::os::unix::fs::symlink(s.root.join("elsewhere"), s.app()).unwrap();
            vec![]
        }),
        ("the app folder is someone else's folder", |s| {
            std::fs::create_dir_all(s.app()).unwrap();
            std::fs::write(s.app().join("notes.txt"), "mine").unwrap();
            vec![]
        }),
        ("the command name is taken", |s| {
            std::fs::create_dir_all(&s.bin).unwrap();
            std::fs::write(s.bin.join("stagepack-studio"), "mine").unwrap();
            vec![]
        }),
        // XDG_DATA_HOME=<home>/lib puts the program at <home>/lib/stagepack-studio, the app's own data folder.
        ("the program would land in the data folder", |s| vec![("XDG_DATA_HOME", Some(s.home.join("lib")))]),
        ("XDG_DATA_HOME is the home itself", |s| vec![("XDG_DATA_HOME", Some(s.home.clone()))]),
        ("XDG_DATA_HOME outside the home", |s| vec![("XDG_DATA_HOME", Some(s.root.join("elsewhere/share")))]),
        ("XDG_DATA_HOME has a .. in it", |s| vec![("XDG_DATA_HOME", Some(s.home.join(".local/share/stagepack-studio/..")))]),
        ("XDG_DATA_HOME is the app's data folder", |s| vec![("XDG_DATA_HOME", Some(s.home.join(".local/share/stagepack-studio")))]),
        ("XDG_DATA_HOME is inside the app's data folder", |s| vec![("XDG_DATA_HOME", Some(s.home.join(".local/share/stagepack-studio/lib/x")))]),
        ("XDG_BIN_HOME is inside the app's data folder", |s| vec![("XDG_BIN_HOME", Some(s.home.join(".local/share/stagepack-studio/bin")))]),
        ("a symlinked menu entry", |s| {
            std::fs::create_dir_all(s.data.join("applications")).unwrap();
            std::fs::create_dir_all(s.home.join("dotfiles")).unwrap();
            std::fs::write(s.home.join("dotfiles/mine.desktop"), "mine").unwrap();
            std::os::unix::fs::symlink(s.home.join("dotfiles/mine.desktop"), s.data.join("applications/stagepack-studio.desktop")).unwrap();
            vec![]
        }),
        ("someone else's icon", |s| {
            let dir = s.data.join("icons/hicolor/48x48/apps");
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join("stagepack-studio.png"), "my own icon").unwrap();
            vec![]
        }),
        ("a menu entry that isn't ours", |s| {
            std::fs::create_dir_all(s.data.join("applications")).unwrap();
            std::fs::write(s.data.join("applications/stagepack-studio.desktop"), "[Desktop Entry]\nName=Mine\n").unwrap();
            vec![]
        }),
        ("a quote in the path", |s| vec![("XDG_DATA_HOME", Some(s.home.join("a\"b/share")))]),
    ];
    for (name, change) in cases {
        let mut s = Scratch::new("refuse-");
        let env = change(&mut s);
        let mut command = s.command("sh");
        command.arg(pkg.join("install.sh"));
        for (key, value) in env {
            match value {
                Some(value) => command.env(key, value),
                None => command.env_remove(key),
            };
        }
        let before = snapshot(&s.root);
        let out = command.output().unwrap();
        let text = said(&out);
        assert_ne!(out.status.code(), Some(0), "{name}: should refuse: {text}");
        assert_eq!(text.trim().lines().count(), 1, "{name}: one sentence expected, got: {text}");
        assert!(text.contains("Nothing was changed."), "{name}: {text}");
        assert_eq!(snapshot(&s.root), before, "{name}: something was written");
    }

    // A folder that isn't a whole staged app.
    let s = Scratch::new("refuse-");
    let partial = s.root.join("partial");
    std::fs::create_dir_all(&partial).unwrap();
    std::fs::copy(pkg.join("install.sh"), partial.join("install.sh")).unwrap();
    let before = snapshot(&s.root);
    let out = s.install(&partial);
    assert_ne!(out.status.code(), Some(0));
    assert_eq!(said(&out).trim().lines().count(), 1, "{}", said(&out));
    assert_eq!(snapshot(&s.root), before);
}

/// The package folder again with one file changed and one added and one removed, as a newer version would be.
fn newer_package(s: &Scratch) -> PathBuf {
    let dir = s.root.join("newer/stagepack-studio-9.9.9");
    std::fs::create_dir_all(dir.parent().unwrap()).unwrap();
    let linked = Command::new("cp").arg("-al").arg(package()).arg(&dir).status().unwrap();
    assert!(linked.success());
    std::fs::remove_file(dir.join("LICENSE")).unwrap();
    std::fs::write(dir.join("LICENSE"), "a newer licence").unwrap();
    std::fs::write(dir.join("NEW-IN-9.9.9"), "new").unwrap();
    std::fs::remove_file(dir.join("guide/components.md")).unwrap();
    dir
}

#[test]
fn upgrade_and_uninstall_keep_the_data() {
    let s = Scratch::new("upgrade-");
    let data = s.data.join("stagepack-studio");
    std::fs::create_dir_all(data.join("work/deep")).unwrap();
    std::fs::write(data.join("build-permissions.json"), r#"{"always_build":["/x"]}"#).unwrap();
    std::fs::write(data.join("work/deep/slide.bin"), [0u8, 1, 2, 255]).unwrap();
    // Something of someone else's in the shared folders, which must be as it was at the end.
    std::fs::create_dir_all(s.data.join("applications")).unwrap();
    std::fs::write(s.data.join("applications/other.desktop"), "[Desktop Entry]\nType=Application\nName=Other\nExec=other\n").unwrap();
    let before_all = snapshot(&s.home);
    let before_data = snapshot(&data);

    let out = s.install(package());
    assert_eq!(out.status.code(), Some(0), "{}", said(&out));
    assert!(s.app().join("guide/components.md").exists());
    let newer = newer_package(&s);
    let out = s.install(&newer);
    assert_eq!(out.status.code(), Some(0), "{}", said(&out));
    assert_eq!(std::fs::read_to_string(s.app().join("LICENSE")).unwrap(), "a newer licence");
    assert!(s.app().join("NEW-IN-9.9.9").exists());
    assert!(!s.app().join("guide/components.md").exists(), "the old program's files should be gone");
    let leftovers: Vec<_> = std::fs::read_dir(s.app().parent().unwrap()).unwrap().map(|e| e.unwrap().file_name()).collect();
    assert_eq!(leftovers, ["stagepack-studio"], "scratch copies were left beside the program");
    assert_eq!(snapshot(&data), before_data, "installing changed the data folder");

    let out = s.uninstall();
    assert_eq!(out.status.code(), Some(0), "{}", said(&out));
    assert_eq!(snapshot(&data), before_data, "uninstalling changed the data folder");
    assert_eq!(kept(snapshot(&s.home)), kept(before_all), "left behind, changed or missing");
}

/// Stagepack, the neighbour app, as it really registers itself: its own type, a subclass of zip, with a
/// glob of weight 90, and a menu entry for that type.
const THEIR_TYPE: &str = "application/x-stagepack";
const THEIR_MIME: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<mime-info xmlns="http://www.freedesktop.org/standards/shared-mime-info">
  <mime-type type="application/x-stagepack">
    <comment>Stagepack presentation</comment>
    <sub-class-of type="application/zip"/>
    <glob pattern="*.sptx" weight="90"/>
  </mime-type>
</mime-info>
"#;

#[test]
fn stagepack_stays_under_open_with() {
    let s = Scratch::new("openwith-");
    let theirs = "[Desktop Entry]\nType=Application\nName=Stagepack\nExec=stagepack %f\nMimeType=application/x-stagepack;\n";
    std::fs::create_dir_all(s.data.join("applications")).unwrap();
    std::fs::write(s.data.join("applications/stagepack.desktop"), theirs).unwrap();
    std::fs::create_dir_all(s.data.join("mime/packages")).unwrap();
    std::fs::write(s.data.join("mime/packages/stagepack.xml"), THEIR_MIME).unwrap();
    let mime_db = s.command("update-mime-database").arg(s.data.join("mime")).output().unwrap();
    assert!(mime_db.status.success(), "{}", said(&mime_db));
    std::fs::create_dir_all(&s.config).unwrap();
    let original_list = format!("[Added Associations]\n{THEIR_TYPE}=stagepack.desktop;\n\n[Default Applications]\n{THEIR_TYPE}=stagepack.desktop;\n");
    std::fs::write(s.config.join("mimeapps.list"), &original_list).unwrap();
    let their_files = snapshot(&s.data.join("applications")).into_iter().filter(|(k, _)| k.starts_with("stagepack.desktop")).collect::<Vec<_>>();
    let talk = s.root.join("talk.sptx");
    std::fs::write(&talk, b"PK\x03\x04 not really a zip").unwrap();
    let talk = talk.to_str().unwrap();
    // Before: a .sptx is Stagepack's type and opens in Stagepack.
    assert_eq!(s.xdg_mime(&["query", "filetype", talk]), THEIR_TYPE);
    assert_eq!(s.xdg_mime(&["query", "default", THEIR_TYPE]), "stagepack.desktop");

    let out = s.install(package());
    assert_eq!(out.status.code(), Some(0), "{}", said(&out));
    // Stagepack's weight-90 glob still decides the file's type (ours has the default weight, so there is no tie that
    // could make the file ours and drop Stagepack from Open with), and that type's default is now ours.
    assert_eq!(s.xdg_mime(&["query", "filetype", talk]), THEIR_TYPE);
    for t in [TYPE, THEIR_TYPE] {
        assert_eq!(s.xdg_mime(&["query", "default", t]), "stagepack-studio.desktop", "{t}");
    }
    assert_eq!(std::fs::read_to_string(s.data.join("applications/stagepack.desktop")).unwrap(), theirs, "theirs was changed");
    assert_eq!(std::fs::read_to_string(s.data.join("mime/packages/stagepack.xml")).unwrap(), THEIR_MIME, "their type was changed");
    let cache = std::fs::read_to_string(s.data.join("applications/mimeinfo.cache")).unwrap();
    let line = cache.lines().find(|l| l.starts_with(&format!("{THEIR_TYPE}="))).expect("their type is in the cache");
    assert!(line.contains("stagepack.desktop") && line.contains("stagepack-studio.desktop"), "both programs should offer the type: {line}");

    let out = s.uninstall();
    assert_eq!(out.status.code(), Some(0), "{}", said(&out));
    assert_eq!(std::fs::read_to_string(s.data.join("applications/stagepack.desktop")).unwrap(), theirs, "theirs was changed by uninstalling");
    assert_eq!(std::fs::read_to_string(s.data.join("mime/packages/stagepack.xml")).unwrap(), THEIR_MIME, "their type was changed by uninstalling");
    let after = snapshot(&s.data.join("applications")).into_iter().filter(|(k, _)| k.starts_with("stagepack.desktop")).collect::<Vec<_>>();
    assert_eq!(after, their_files);
    let list = std::fs::read_to_string(s.config.join("mimeapps.list")).unwrap();
    assert_eq!(list, original_list, "Stagepack's own default should be back, byte for byte");
    assert_eq!(s.xdg_mime(&["query", "filetype", talk]), THEIR_TYPE);
    assert_eq!(s.xdg_mime(&["query", "default", THEIR_TYPE]), "stagepack.desktop");
    assert!(!s.data.join("applications/stagepack-studio.desktop").exists());
}

/// Each type's default, in each list, is recorded and given back on its own.
#[test]
fn each_types_default_comes_back_on_its_own() {
    let s = Scratch::new("two-types-");
    std::fs::create_dir_all(s.data.join("applications")).unwrap();
    for (name, t) in [("viewer.desktop", TYPE), ("stagepack.desktop", THEIR_TYPE)] {
        std::fs::write(s.data.join("applications").join(name), format!("[Desktop Entry]\nType=Application\nName={name}\nExec=x %f\nMimeType={t};\n")).unwrap();
    }
    std::fs::create_dir_all(&s.config).unwrap();
    let config_list = format!("[Default Applications]\n{TYPE}=viewer.desktop;\n{THEIR_TYPE}=stagepack.desktop;\n");
    let apps_list = format!("[Default Applications]\n{THEIR_TYPE}=viewer.desktop;\n");
    std::fs::write(s.config.join("mimeapps.list"), &config_list).unwrap();
    std::fs::write(s.data.join("applications/mimeapps.list"), &apps_list).unwrap();

    let out = s.install(package());
    assert_eq!(out.status.code(), Some(0), "{}", said(&out));
    let manifest = std::fs::read_to_string(s.app().join("install-manifest.txt")).unwrap();
    for line in [format!("prev_config:{TYPE}=viewer.desktop;"), format!("prev_config:{THEIR_TYPE}=stagepack.desktop;"), format!("prev_apps:{THEIR_TYPE}=viewer.desktop;")] {
        assert!(manifest.lines().any(|l| l == line), "{line} isn't in\n{manifest}");
    }
    // Hand-made: the apps list now names us for both types, as an older xdg-mime would have written it.
    std::fs::write(s.data.join("applications/mimeapps.list"), format!("[Default Applications]\n{THEIR_TYPE}=stagepack-studio.desktop;viewer.desktop;\n{TYPE}=stagepack-studio.desktop;\n")).unwrap();
    // An upgrade keeps what the first install recorded.
    let out = s.install(package());
    assert_eq!(out.status.code(), Some(0), "{}", said(&out));
    assert_eq!(std::fs::read_to_string(s.app().join("install-manifest.txt")).unwrap(), manifest);

    let out = s.uninstall();
    assert_eq!(out.status.code(), Some(0), "{}", said(&out));
    assert_eq!(std::fs::read_to_string(s.config.join("mimeapps.list")).unwrap(), config_list);
    assert_eq!(std::fs::read_to_string(s.data.join("applications/mimeapps.list")).unwrap(), format!("[Default Applications]\n{THEIR_TYPE}=viewer.desktop;\n"));
}

#[test]
#[ignore = "packs the whole app with zstd -19; run by test-all"]
fn the_tarball_unpacks_to_one_folder() {
    let out_dir = repo_root().join("target/package-test");
    let tarball = xtask::linux::pack(staged(), &out_dir).unwrap();
    let version = xtask::linux::version().unwrap();
    assert_eq!(tarball.file_name().unwrap().to_str().unwrap(), format!("stagepack-studio-{version}-linux-x86_64.tar.zst"));
    eprintln!("the tarball is {} MB", std::fs::metadata(&tarball).unwrap().len().div_ceil(1024 * 1024));
    let listing = Command::new("tar").args(["--use-compress-program=zstd", "-tvf"]).arg(&tarball).output().unwrap();
    assert!(listing.status.success(), "{}", said(&listing));
    let text = String::from_utf8_lossy(&listing.stdout);
    let top = format!("stagepack-studio-{version}");
    let mut installer = None;
    for line in text.lines() {
        // "-rwxr-xr-x 0/0 size date time name" (a link also has " -> target").
        let name = line.split_whitespace().nth(5).unwrap();
        assert!(name == format!("{top}/") || name.starts_with(&format!("{top}/")), "{name} is outside {top}");
        if name == format!("{top}/install.sh") {
            installer = Some(line.to_string());
        }
    }
    let installer = installer.expect("install.sh is in the tarball");
    assert!(installer.starts_with("-rwx"), "install.sh should be executable: {installer}");
    for part in ["uninstall.sh", "stagepack-studio", "stagepack-terminal", "bundle/bin/rustc", "stagepack-studio.desktop.in", "stagepack-studio-mime.xml", "NOTICES.txt"] {
        assert!(text.contains(&format!("{top}/{part}")), "{part} is not in the tarball");
    }
    // Unpacked and installed from the tarball itself, into a scratch home.
    let s = Scratch::new("tarball-");
    let unpacked = s.root.join("unpacked");
    std::fs::create_dir_all(&unpacked).unwrap();
    let extract = Command::new("tar").args(["--use-compress-program=zstd", "-xf"]).arg(&tarball).arg("-C").arg(&unpacked).status().unwrap();
    assert!(extract.success());
    let out = s.install(&unpacked.join(&top));
    assert_eq!(out.status.code(), Some(0), "{}", said(&out));
    assert_eq!(s.uninstall().status.code(), Some(0));
}

#[test]
fn packing_drops_the_container_builds_marker() {
    // Only scripts/package-linux.sh writes the marker, after its glibc-check; any packing removes it
    // first, so a tarball from `package linux` on the host is never taken for a container build.
    let out_dir = repo_root().join("target/package-marker-test");
    let _ = std::fs::remove_dir_all(&out_dir);
    std::fs::create_dir_all(&out_dir).unwrap();
    let marker = out_dir.join(xtask::linux::BUILT_IN_UBUNTU_22_04);
    std::fs::write(&marker, "0000000000000000000000000000000000000000\n").unwrap();
    // A staged folder that isn't there: packing fails, but the marker is gone already.
    assert!(xtask::linux::pack(&out_dir.join("no-such-staged-folder"), &out_dir).is_err());
    assert!(!marker.exists(), "the marker outlived a packing");
    std::fs::remove_dir_all(&out_dir).unwrap();
}

/// Installs into a fresh scratch home and returns it.
fn installed() -> Scratch {
    let s = Scratch::new("fix-");
    let out = s.install(package());
    assert_eq!(out.status.code(), Some(0), "{}", said(&out));
    s
}

#[test]
fn uninstall_removes_only_what_install_wrote() {
    let s = installed();
    // A planted manifest naming the owner's files, the home and a path with .. in it.
    std::fs::create_dir_all(s.home.join("Documents")).unwrap();
    std::fs::write(s.home.join("Documents/thesis.txt"), "mine").unwrap();
    let data = s.data.join("stagepack-studio");
    std::fs::create_dir_all(&data).unwrap();
    std::fs::write(data.join("permissions.json"), "{}").unwrap();
    let manifest = s.app().join("install-manifest.txt");
    let mut text = std::fs::read_to_string(&manifest).unwrap();
    for line in [format!("f {}/Documents/thesis.txt", s.home.display()), format!("f {}", data.join("permissions.json").display()), format!("d {}", s.home.display()), format!("f {}/Documents/../Documents/thesis.txt", s.home.display()), format!("t {}", s.home.display())] {
        text.push_str(&line);
        text.push('\n');
    }
    std::fs::write(&manifest, text).unwrap();
    // A menu entry the owner has since replaced stays too.
    std::fs::write(s.data.join("applications/stagepack-studio.desktop"), "[Desktop Entry]\nName=Mine now\n").unwrap();
    let before = snapshot(&s.home.join("Documents"));
    let out = s.uninstall();
    assert_eq!(out.status.code(), Some(0), "{}", said(&out));
    assert_eq!(snapshot(&s.home.join("Documents")), before);
    assert!(data.join("permissions.json").exists());
    assert!(s.data.join("applications/stagepack-studio.desktop").exists(), "an entry that isn't ours was removed");
    assert!(said(&out).contains("Left "), "{}", said(&out));
    assert!(!s.app().exists() && s.home.exists());
}

#[test]
fn uninstall_refuses_a_manifest_that_points_at_the_data_folder() {
    let s = installed();
    let manifest = s.app().join("install-manifest.txt");
    let text = std::fs::read_to_string(&manifest).unwrap();
    let data = s.data.join("stagepack-studio");
    std::fs::create_dir_all(&data).unwrap();
    std::fs::write(data.join("a"), "x").unwrap();
    let tampered: String = text.lines().map(|l| if l.starts_with("DATA=") { format!("DATA={}\n", data.display()) } else { format!("{l}\n") }).collect();
    std::fs::write(&manifest, tampered).unwrap();
    let out = s.uninstall();
    assert_ne!(out.status.code(), Some(0));
    assert!(data.join("a").exists() && s.app().exists());
    // Uninstalling in an environment other than install's still works, by the recorded settings.
    std::fs::write(&manifest, text).unwrap();
    let mut command = s.command("sh");
    command.arg(s.app().join("uninstall.sh")).env_remove("XDG_DATA_HOME").env_remove("XDG_CONFIG_HOME").env_remove("XDG_BIN_HOME");
    // (the scratch data folder is the default one, so make it the recorded one explicitly)
    let out = command.output().unwrap();
    assert_eq!(out.status.code(), Some(0), "{}", said(&out));
    assert!(!s.app().exists());
    assert!(!s.bin.join("stagepack-studio").exists());
}

#[test]
fn install_never_writes_through_a_link() {
    let s = Scratch::new("link-");
    // Installing over a dotfiles link is refused (see the refusal cases); an old link of ours after an install stays a link.
    let out = s.install(package());
    assert_eq!(out.status.code(), Some(0), "{}", said(&out));
    let entry = s.data.join("applications/stagepack-studio.desktop");
    let mine = s.home.join("dotfiles/mine.desktop");
    std::fs::create_dir_all(mine.parent().unwrap()).unwrap();
    std::fs::write(&mine, "mine").unwrap();
    std::fs::remove_file(&entry).unwrap();
    std::os::unix::fs::symlink(&mine, &entry).unwrap();
    let out = s.install(package());
    assert_ne!(out.status.code(), Some(0));
    assert_eq!(std::fs::read_to_string(&mine).unwrap(), "mine");
    let out = s.uninstall();
    assert_eq!(out.status.code(), Some(0), "{}", said(&out));
    assert_eq!(std::fs::read_to_string(&mine).unwrap(), "mine", "uninstall removed through a link");
}

#[test]
fn a_cut_short_install_tidies_up() {
    for stop in ["copy", "swap"] {
        // First install cut during the copy, and an upgrade cut between the two moves.
        let s = Scratch::new("cut-");
        let lib = s.app().parent().unwrap().to_path_buf();
        if stop == "swap" {
            assert_eq!(s.install(package()).status.code(), Some(0));
        }
        let before = snapshot(&s.home);
        let out = s.command("sh").arg(package().join("install.sh")).env("STAGEPACK_INSTALL_STOP_AT", stop).output().unwrap();
        assert_ne!(out.status.code(), Some(0), "{stop}: {}", said(&out));
        let names: Vec<_> = std::fs::read_dir(&lib).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().to_string()).collect();
        assert!(names.iter().all(|n| n == "stagepack-studio"), "{stop}: scratch copies left: {names:?}");
        if stop == "swap" {
            assert_eq!(snapshot(&s.home), before, "the old install should be back exactly");
        } else {
            assert!(!s.app().exists());
        }
        assert!(said(&out).contains("stopped before finishing"));
    }
}

#[test]
fn a_default_of_stars_is_not_expanded() {
    let s = installed();
    let list = s.config.join("mimeapps.list");
    std::fs::write(&list, format!("[Default Applications]\n{TYPE}=stagepack-studio.desktop;*;\n")).unwrap();
    assert_eq!(s.uninstall().status.code(), Some(0));
    assert_eq!(std::fs::read_to_string(&list).unwrap(), format!("[Default Applications]\n{TYPE}=*;\n"));
}

#[test]
fn install_leaves_the_users_own_old_and_new_folders_alone() {
    let s = Scratch::new("sweep-");
    let lib = s.app().parent().unwrap().to_path_buf();
    let mine = ["stagepack-studio.old-0.1.0", "stagepack-studio.old-photos", "stagepack-studio.new-ideas", "stagepack-studio.old-123"];
    for name in mine {
        std::fs::create_dir_all(lib.join(name)).unwrap();
        std::fs::write(lib.join(name).join("mine.txt"), name).unwrap();
    }
    let before = snapshot(&lib);
    let out = s.install(package());
    assert_eq!(out.status.code(), Some(0), "{}", said(&out));
    for name in mine {
        assert_eq!(std::fs::read_to_string(lib.join(name).join("mine.txt")).unwrap(), name);
    }
    assert!(s.app().join("stagepack-studio").is_file());
    // And an upgrade (with the user's backup beside it, named by hand) keeps them too.
    let out = s.install(&newer_package(&s));
    assert_eq!(out.status.code(), Some(0), "{}", said(&out));
    let after = snapshot(&lib);
    for (k, v) in &before {
        assert_eq!(after.get(k), Some(v), "{k} changed");
    }
}

#[test]
fn an_upgrade_under_other_settings_is_refused() {
    let s = installed();
    let before = snapshot(&s.home);
    let out = s.command("sh").arg(package().join("install.sh")).env("XDG_BIN_HOME", s.home.join("other-bin")).output().unwrap();
    assert_ne!(out.status.code(), Some(0), "{}", said(&out));
    assert!(said(&out).contains("earlier install used BIN="), "{}", said(&out));
    assert_eq!(snapshot(&s.home), before);
}

/// The package folder again, with a program that can't start here, as on a system with too old a glibc.
fn unstartable_package(s: &Scratch) -> PathBuf {
    let dir = s.root.join("unstartable/stagepack-studio-9.9.9");
    std::fs::create_dir_all(dir.parent().unwrap()).unwrap();
    assert!(Command::new("cp").arg("-al").arg(package()).arg(&dir).status().unwrap().success());
    let program = dir.join("stagepack-studio");
    std::fs::remove_file(&program).unwrap();
    std::fs::write(&program, "#!/bin/sh\necho \"stagepack-studio: /lib/x86_64-linux-gnu/libc.so.6: version \\`GLIBC_2.39' not found\" >&2\nexit 127\n").unwrap();
    std::fs::set_permissions(&program, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
    dir
}

#[test]
fn a_program_that_cant_start_here_is_not_installed() {
    // A first install: refused, and nothing is left.
    let s = Scratch::new("cant-start-");
    let broken = unstartable_package(&s);
    let before = snapshot(&s.home);
    let out = s.install(&broken);
    let text = said(&out);
    assert_ne!(out.status.code(), Some(0), "{text}");
    assert_eq!(text.lines().filter(|l| l.starts_with("install.sh:")).count(), 1, "one sentence expected: {text}");
    assert!(text.contains("GLIBC_2.39' not found") && text.contains("glibc 2.35") && text.contains("Nothing was changed."), "{text}");
    assert_eq!(snapshot(&s.home), before);

    // An upgrade: refused, and the earlier install is exactly as it was.
    let s = installed();
    let broken = unstartable_package(&s);
    let before = snapshot(&s.home);
    let out = s.install(&broken);
    let text = said(&out);
    assert_ne!(out.status.code(), Some(0), "{text}");
    assert!(text.contains("GLIBC_2.39' not found") && text.contains("Nothing was changed."), "{text}");
    assert!(!text.contains("stopped before finishing"), "{text}");
    assert_eq!(snapshot(&s.home), before, "the earlier install should be untouched");
}

#[test]
fn what_install_says_matches_what_it_did() {
    let s = Scratch::new("say-");
    let out = s.install(package());
    assert_eq!(out.status.code(), Some(0), "{}", said(&out));
    assert!(!said(&out).contains("restart"), "a first install has nothing to restart: {}", said(&out));
    // An upgrade: a running copy keeps the old, deleted folder until it restarts.
    let out = s.install(package());
    assert_eq!(out.status.code(), Some(0), "{}", said(&out));
    assert!(said(&out).contains("If Stagepack Studio is running, restart it."), "{}", said(&out));
    // Stopped once the new program is in place: it mustn't say the earlier one still is.
    let out = s.command("sh").arg(package().join("install.sh")).env("STAGEPACK_INSTALL_STOP_AT", "placed").output().unwrap();
    let text = said(&out);
    assert_ne!(out.status.code(), Some(0), "{text}");
    assert!(!text.contains("is still in place") && !text.contains("Nothing was changed"), "{text}");
    assert!(text.contains("The new program is in place") && text.contains("install.sh again"), "{text}");
    assert!(s.app().join("stagepack-studio").is_file());
}
