// Builds the installer's resources: icon, version info, manifest and the
// crow.exe it installs (stored as RCDATA #1). Build crow first:
//   cargo build --release -p crow
//   cargo build --release -p crow-setup
include!("../build/res.rs");

fn main() {
    let root = std::path::PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap()).join("..");
    let exe = std::env::var("CROW_EXE").map(std::path::PathBuf::from)
        .unwrap_or_else(|_| root.join("target/x86_64-pc-windows-msvc/release/crow.exe"));
    let payload = std::fs::read(&exe).unwrap_or_else(|_| {
        panic!("{} not found. Build the app first: cargo build --release -p crow", exe.display())
    });
    let ico = std::fs::read(root.join("packaging/crow_embed.ico")).ok();
    let out = std::path::PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("setup.res");
    res::write_with(&out, "Crow setup", "CrowSetup.exe", false, ico.as_deref(), Some(&payload));
    println!("cargo:rerun-if-changed={}", exe.display());
    println!("cargo:rerun-if-env-changed=CROW_EXE");
    println!("cargo:rustc-link-arg-bins={}", out.display());
    println!("cargo:rustc-link-arg-bins=/ENTRY:entry");
    println!("cargo:rustc-link-arg-bins=/SUBSYSTEM:WINDOWS,6.02");
}
