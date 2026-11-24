//! Reading which glibc an ELF file needs, so that the Linux package is held to its ceiling.

use std::path::Path;

#[test]
fn the_highest_glibc_need_is_found() {
    let out = "0000 DF *UND* 0 (GLIBC_2.2.5) malloc\n0000 DF *UND* 0 (GLIBC_2.34) __libc_start_main\n0000 DF *UND* 0 (GLIBC_2.17) clock_gettime\n0000 DF *UND* 0 (GLIBCXX_3.4.30) _ZSt4cout\n";
    assert_eq!(xtask::elf::parse_glibc_needs(out), Some((2, 34)));
}

#[test]
fn no_glibc_need_is_none() {
    assert_eq!(xtask::elf::parse_glibc_needs("0000 DF *UND* 0 (GLIBCXX_3.4) x\n"), None);
}

#[test]
fn two_point_ten_is_above_two_point_nine() {
    assert_eq!(xtask::elf::parse_glibc_needs("(GLIBC_2.9) a\n(GLIBC_2.10) b\n"), Some((2, 10)));
}

#[test]
fn elf_files_finds_only_elf_and_skips_links() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("sub")).unwrap();
    std::fs::copy("/bin/true", dir.path().join("sub/true")).unwrap();
    std::fs::write(dir.path().join("notes.txt"), "not a program").unwrap();
    std::os::unix::fs::symlink("sub/true", dir.path().join("link")).unwrap();
    assert_eq!(xtask::elf::elf_files(dir.path()), vec![dir.path().join("sub/true")]);
}

#[test]
fn this_machines_true_needs_some_glibc() {
    assert!(xtask::elf::max_glibc(Path::new("/bin/true")).unwrap().is_some());
}

#[test]
fn the_ceiling_is_ubuntu_22_04s_glibc() {
    assert_eq!(xtask::elf::GLIBC_CEILING, (2, 35));
}
