// Linker settings and version info for crow_menu.dll.
include!("../build/res.rs");

fn main() {
    let out = std::path::PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("crow_menu.res");
    res::write(&out, "Crow FO context menu", "crow_menu.dll", true, &[]);
    println!("cargo:rerun-if-changed=../build/res.rs");
    println!("cargo:rustc-link-arg-cdylib={}", out.display());
    println!("cargo:rustc-link-arg-cdylib=/ENTRY:DllMain");
    println!("cargo:rustc-link-arg-cdylib=/SUBSYSTEM:WINDOWS,6.02");
}
