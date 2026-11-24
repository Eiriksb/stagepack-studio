use xtask::notices::{self, Notice, PLATFORMS};
use xtask::{draw_icon, write_ico};

fn png(size: u32) -> Vec<u8> {
    draw_icon(size).encode_png().unwrap()
}

#[test]
fn the_icon_is_drawn_at_every_size() {
    for size in xtask::icon::PNG_SIZES {
        let decoded = tiny_skia::Pixmap::decode_png(&png(size)).unwrap();
        assert_eq!((decoded.width(), decoded.height()), (size, size));
        let first = decoded.pixel(0, 0).unwrap();
        let px = decoded.pixels();
        assert!(px.iter().any(|p| p != &first), "the {size} px icon is one colour");
    }
    let small = draw_icon(16);
    assert_eq!(small.pixel(8, 8).unwrap().alpha(), 255, "the centre of the 16 px icon is opaque");
    assert_eq!(small.pixel(0, 0).unwrap().alpha(), 0, "the corner is rounded off");
}

#[test]
fn the_ico_holds_each_size() {
    let sizes = [16u32, 24, 32, 48, 64, 128, 256];
    let ico = write_ico(&sizes.iter().map(|&s| (s, png(s))).collect::<Vec<_>>());
    assert_eq!(&ico[..4], &[0, 0, 1, 0]);
    assert_eq!(u16::from_le_bytes([ico[4], ico[5]]) as usize, sizes.len());
    for (i, &size) in sizes.iter().enumerate() {
        let e = &ico[6 + 16 * i..6 + 16 * (i + 1)];
        let dim = if size == 256 { 0 } else { size as u8 };
        assert_eq!((e[0], e[1]), (dim, dim));
        let len = u32::from_le_bytes(e[8..12].try_into().unwrap()) as usize;
        let off = u32::from_le_bytes(e[12..16].try_into().unwrap()) as usize;
        let decoded = tiny_skia::Pixmap::decode_png(&ico[off..off + len]).unwrap();
        assert_eq!(decoded.width(), size);
    }
}

#[test]
fn every_dependency_has_its_notice() {
    let metas: Vec<Vec<u8>> = PLATFORMS.iter().map(|p| notices::run_metadata(p).unwrap()).collect();
    let extra = notices::extra_notices(&metas[0]).unwrap();
    let refs: Vec<&[u8]> = metas.iter().map(Vec::as_slice).collect();
    let text = notices::notices_multi(&refs, &extra).unwrap();
    for m in &metas {
        let graph = notices::graph(m).unwrap();
        assert!(graph.len() > 100, "the graph looks too small: {}", graph.len());
        for p in graph {
            assert!(text.contains(&p.label()), "{} is missing from the notices", p.label());
        }
    }
    // Packages only one platform needs are in too.
    assert!(text.contains("windows-sys 0.61.2"));
    assert!(text.contains("portable-pty 0.9.0"));
    // The fonts, the toolchain and OpenH264.
    for needle in ["Inter Project Authors", "JetBrains Mono", "Noto", "Apache License", "LLVM-exception", "OpenH264", "Cisco"] {
        assert!(text.contains(needle), "'{needle}' is missing from the notices");
    }
}

#[test]
fn the_slidev_add_ons_crates_have_their_notices_under_its_heading() {
    let metas: Vec<Vec<u8>> = PLATFORMS.iter().map(|p| notices::run_metadata(p).unwrap()).collect();
    let addon = notices::run_metadata(notices::ADDON_PLATFORM).unwrap();
    let graph = notices::addon_graph(&addon, &notices::run_addon_tree().unwrap()).unwrap();
    let refs: Vec<&[u8]> = metas.iter().map(Vec::as_slice).collect();
    let text = notices::notices_with_addon(&refs, Some(graph.clone()), &[]).unwrap();
    let heading = text.find(&format!("\n{}\n", notices::ADDON_HEADING)).expect("the add-on has its heading");
    let after = &text[heading..];
    // Its own crates, those only it uses among them.
    for name in ["slidev-addon", "addon-abi", "slides-format", "yaml-rust2", "hashlink", "serde_json"] {
        let package = graph.iter().find(|p| p.name == name).unwrap_or_else(|| panic!("{name} isn't in the add-on's graph"));
        assert!(after.contains(&package.label()), "{} is missing under the add-on's heading", package.label());
    }
    for p in &graph {
        assert!(after.contains(&p.label()), "{} is missing under the add-on's heading", p.label());
    }
    // Its dev-dependencies are not built into it, nor what only the app turns on in addon-abi.
    for name in ["slides-components", "wasmtime", "zip", "ed25519-dalek"] {
        assert!(!graph.iter().any(|p| p.name == name), "{name} is listed as built into the add-on");
    }
}

#[test]
fn a_package_with_no_licence_fails() {
    let dir = std::env::temp_dir().join(format!("xtask-nolicence-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let manifest = dir.join("Cargo.toml");
    let json = serde_json::json!({
        "packages": [
            {"id": "root 0.1.0", "name": "stagepack-studio", "version": "0.1.0", "license": "MIT", "manifest_path": manifest, "license_file": null},
            {"id": "bare 1.2.3", "name": "bare", "version": "1.2.3", "license": null, "manifest_path": manifest, "license_file": null}
        ],
        "workspace_root": dir,
        "resolve": {"nodes": [
            {"id": "root 0.1.0", "deps": [{"pkg": "bare 1.2.3", "dep_kinds": [{"kind": null, "target": null}]}]},
            {"id": "bare 1.2.3", "deps": []}
        ]}
    });
    let err = notices::notices(json.to_string().as_bytes(), &[]).unwrap_err();
    assert!(err.contains("bare 1.2.3"), "{err}");
    // A dev-only dependency is not part of the shipped graph, so it doesn't fail.
    let dev = json.to_string().replace("\"kind\":null", "\"kind\":\"dev\"");
    let ok = notices::notices(dev.as_bytes(), &[Notice { name: "X".into(), licence: "MIT".into(), texts: vec![] }]).unwrap();
    assert!(!ok.contains("bare 1.2.3"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_package_with_no_files_gets_the_standard_texts() {
    let dir = std::env::temp_dir().join(format!("xtask-nofiles-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let manifest = dir.join("Cargo.toml");
    let json = serde_json::json!({
        "packages": [
            {"id": "root 0.1.0", "name": "stagepack-studio", "version": "0.1.0", "license": "MIT", "manifest_path": manifest, "license_file": null},
            {"id": "dual 1.0.0", "name": "dual", "version": "1.0.0", "license": "MIT OR Apache-2.0", "manifest_path": manifest, "license_file": null}
        ],
        "workspace_root": dir,
        "resolve": {"nodes": [
            {"id": "root 0.1.0", "deps": [{"pkg": "dual 1.0.0", "dep_kinds": [{"kind": null, "target": null}]}]},
            {"id": "dual 1.0.0", "deps": []}
        ]}
    });
    let text = notices::notices(json.to_string().as_bytes(), &[]).unwrap();
    assert!(text.contains("--- MIT (standard text) ---") && text.contains("--- Apache-2.0 (standard text) ---"), "{text}");
    assert!(text.contains("Apache License"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_toolchain_copyright_file_is_found() {
    assert!(notices::toolchain_copyright().unwrap().is_file());
}

#[test]
fn glibc_check_names_each_program_and_the_highest_need() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("bin")).unwrap();
    std::fs::copy("/bin/true", dir.path().join("bin/true")).unwrap();
    std::fs::write(dir.path().join("README"), "text").unwrap();
    let need = xtask::elf::max_glibc(&dir.path().join("bin/true")).unwrap().unwrap();
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_package")).arg("glibc-check").arg(dir.path()).output().unwrap();
    let said = String::from_utf8_lossy(&output.stdout);
    let text = xtask::elf::version_text(need);
    assert!(said.lines().any(|l| l == format!("{text} bin/true")), "{said}");
    assert!(said.contains(&format!("highest: {text}")), "{said}");
    assert!(!said.contains("README"), "{said}");
    let expected = if need > xtask::elf::GLIBC_CEILING { 1 } else { 0 };
    assert_eq!(output.status.code(), Some(expected), "{said}{}", String::from_utf8_lossy(&output.stderr));
}

#[test]
fn glibc_check_of_a_folder_without_programs_fails() {
    let dir = tempfile::tempdir().unwrap();
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_package")).arg("glibc-check").arg(dir.path()).output().unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&output.stderr).contains("no ELF files"));
}
