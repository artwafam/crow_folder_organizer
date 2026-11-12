// Build script for the Linux app. It only reads files of the Windows build,
// never changes them:
//  - lang/*.ini becomes the embedded language table, exactly like ../build.rs.
//  - The types and constants of ../src/ffi.rs are copied (without the
//    `extern "system"` import blocks) so the shared code sees the same
//    definitions. src/ffi implements the functions on top of X11.
//  - The version, authors and repository come from [workspace.package] in
//    ../Cargo.toml, the single place they live.
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

fn main() {
    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap());
    let root = Path::new("..");
    langs(root, &out);
    ffi_types(root, &out);
    package_info(root);
    println!("cargo:rerun-if-changed=build.rs");
}

fn langs(root: &Path, out: &Path) {
    let dir = root.join("lang");
    let mut langs: Vec<(String, PathBuf)> = std::fs::read_dir(&dir)
        .expect("lang folder missing")
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "ini"))
        .map(|p| (p.file_stem().unwrap().to_string_lossy().into_owned(), std::fs::canonicalize(&p).unwrap()))
        .collect();
    langs.sort_by(|a, b| (a.0 != "en", &a.0).cmp(&(b.0 != "en", &b.0)));
    // Linux wording (lang/ next to this file) goes first: the first match of a
    // key wins, so it replaces the few texts that name Windows.
    let mut src = String::from("pub static LANGS: &[(&str, &str)] = &[\n");
    for (code, path) in &langs {
        let shared = std::fs::read_to_string(path).unwrap();
        let ours = std::fs::read_to_string(Path::new("lang").join(format!("{code}.ini"))).unwrap_or_default();
        let merged = out.join(format!("lang-{code}.ini"));
        std::fs::write(&merged, format!("{ours}\n{}", shared.trim_start_matches('\u{feff}'))).unwrap();
        writeln!(src, "    ({:?}, include_str!({:?})),", code, merged.to_string_lossy()).unwrap();
    }
    src.push_str("];\n");
    std::fs::write(out.join("langs.rs"), src).unwrap();
    println!("cargo:rerun-if-changed={}", dir.display());
    println!("cargo:rerun-if-changed=lang");
}

/// Copies ../src/ffi.rs without its `#[link(...)] extern "system" { ... }`
/// blocks and without its inner attributes.
fn ffi_types(root: &Path, out: &Path) {
    let path = root.join("src/ffi.rs");
    let src = std::fs::read_to_string(&path).expect("src/ffi.rs missing");
    let mut keep = String::new();
    let mut skip = false;
    for line in src.lines() {
        let t = line.trim_start();
        if skip {
            if line.starts_with('}') {
                skip = false;
            }
            continue;
        }
        if t.starts_with("#[link(") {
            skip = true;
            continue;
        }
        if t.starts_with("#![") || t.starts_with("//!") || t.starts_with("use core::ffi::c_void") {
            continue;
        }
        keep.push_str(line);
        keep.push('\n');
    }
    std::fs::write(out.join("ffi_types.rs"), keep).unwrap();
    println!("cargo:rerun-if-changed={}", path.display());
}

fn package_info(root: &Path) {
    let path = root.join("Cargo.toml");
    let toml = std::fs::read_to_string(&path).expect("../Cargo.toml missing");
    let mut in_pkg = false;
    let (mut version, mut authors, mut repo) = (String::new(), String::new(), String::new());
    for line in toml.lines() {
        let t = line.trim();
        if t.starts_with('[') {
            in_pkg = t == "[workspace.package]";
            continue;
        }
        if !in_pkg {
            continue;
        }
        let Some((k, v)) = t.split_once('=') else { continue };
        let v = v.trim();
        match k.trim() {
            "version" => version = v.trim_matches('"').to_string(),
            "repository" => repo = v.trim_matches('"').to_string(),
            "authors" => {
                authors = v.trim_matches(['[', ']']).split(',').map(|a| a.trim().trim_matches('"')).filter(|a| !a.is_empty()).collect::<Vec<_>>().join(":")
            }
            _ => {}
        }
    }
    assert!(!version.is_empty(), "no version under [workspace.package] in ../Cargo.toml");
    let label = std::env::var("CROW_VERSION_LABEL").unwrap_or_default();
    let label = label.trim();
    let display = if label.is_empty() {
        version.clone()
    } else if version.contains('-') {
        format!("{version}.{label}")
    } else {
        format!("{version}-{label}")
    };
    println!("cargo:rustc-env=CARGO_PKG_VERSION={version}");
    println!("cargo:rustc-env=CROW_VERSION={display}");
    println!("cargo:rustc-env=CARGO_PKG_AUTHORS={authors}");
    println!("cargo:rustc-env=CARGO_PKG_REPOSITORY={repo}");
    println!("cargo:rerun-if-changed={}", path.display());
    println!("cargo:rerun-if-env-changed=CROW_VERSION_LABEL");
}
