// Linker settings, embedded resources and the language table for CrowFO.exe.
include!("build/res.rs");

use std::fmt::Write as _;

fn main() {
    let out_dir = std::path::PathBuf::from(std::env::var("OUT_DIR").unwrap());

    // Every lang/*.ini file becomes an embedded language. English comes first
    // because it is the fallback for missing keys.
    let mut langs: Vec<(String, std::path::PathBuf)> = std::fs::read_dir("lang")
        .expect("lang folder missing")
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "ini"))
        .map(|p| (p.file_stem().unwrap().to_string_lossy().into_owned(), std::fs::canonicalize(&p).unwrap()))
        .collect();
    langs.sort_by(|a, b| (a.0 != "en", &a.0).cmp(&(b.0 != "en", &b.0)));
    let mut src = String::from("pub static LANGS: &[(&str, &str)] = &[\n");
    for (code, path) in &langs {
        writeln!(src, "    ({:?}, include_str!({:?})),", code, path.to_string_lossy()).unwrap();
    }
    src.push_str("];\n");
    std::fs::write(out_dir.join("langs.rs"), src).unwrap();
    println!("cargo:rerun-if-changed=lang");

    let res = out_dir.join("crow.res");
    // #1 is the app icon, #2 to #4 are used by the desktop right-click menu
    let names = ["crow_embed", "menu_new", "menu_panel", "menu_settings"];
    let icons: Vec<Vec<u8>> = names.iter().map(|n| std::fs::read(format!("packaging/{n}.ico")).expect("icon missing")).collect();
    let icons: Vec<&[u8]> = icons.iter().map(|v| v.as_slice()).collect();
    res::write(&res, "Crow FO", "CrowFO.exe", false, &icons);
    println!("cargo:rerun-if-changed=build/res.rs");
    for n in names {
        println!("cargo:rerun-if-changed=packaging/{n}.ico");
    }
    println!("cargo:rustc-link-arg-bins={}", res.display());
    println!("cargo:rustc-link-arg-bins=/ENTRY:entry");
    println!("cargo:rustc-link-arg-bins=/SUBSYSTEM:WINDOWS,6.02");
}
