// Shared by both build scripts through include!. It writes a Win32 .res file
// with version info, an application manifest and optionally the app icon.
// rust-lld links it straight into the binary, so neither rc.exe nor the
// Windows SDK is needed. Binaries that carry proper version info and a
// manifest look like normal software to antivirus heuristics.

#[allow(dead_code)]
mod res {
    use std::io::Write;

    pub const LANG: u16 = 0x0409; // English (US)

    /// The Cargo.toml version as four numbers. The last one is the build
    /// number from CROW_BUILD, which the GitHub workflow sets. It stays 0 on
    /// local builds.
    fn version() -> [u16; 4] {
        let mut v = [0u16; 4];
        let s = std::env::var("CARGO_PKG_VERSION").unwrap_or_default();
        for (i, n) in s.split(['.', '-', '+']).take(3).enumerate() {
            v[i] = n.parse().unwrap_or(0);
        }
        v[3] = std::env::var("CROW_BUILD").ok().and_then(|b| b.trim().parse::<u32>().ok()).unwrap_or(0).min(65535) as u16;
        v
    }

    /// Version shown in the About page and in the file properties. Test
    /// builds from the workflow get a label at the end, like
    /// "1.2.0-dev.57+3f2a1bc".
    pub fn display_version() -> String {
        let base = std::env::var("CARGO_PKG_VERSION").unwrap_or_default();
        let label = std::env::var("CROW_VERSION_LABEL").unwrap_or_default();
        let label = label.trim();
        if label.is_empty() {
            base
        } else if base.contains('-') {
            format!("{base}.{label}")
        } else {
            format!("{base}-{label}")
        }
    }

    /// The authors from Cargo.toml, shown as the publisher of the file.
    fn author() -> String {
        let a = std::env::var("CARGO_PKG_AUTHORS").unwrap_or_default().replace(':', ", ");
        if a.is_empty() { "Crow".into() } else { a }
    }

    fn w(s: &str) -> Vec<u8> {
        s.encode_utf16().chain(std::iter::once(0)).flat_map(|c| c.to_le_bytes()).collect()
    }
    fn pad4(v: &mut Vec<u8>) {
        while v.len() % 4 != 0 {
            v.push(0);
        }
    }

    /// One VS_VERSIONINFO style node with its header, key, value and children.
    fn node(key: &str, value: &[u8], value_len: u16, text: bool, children: &[Vec<u8>]) -> Vec<u8> {
        let mut v = vec![0u8; 6];
        v[2..4].copy_from_slice(&value_len.to_le_bytes());
        v[4..6].copy_from_slice(&(text as u16).to_le_bytes());
        v.extend(w(key));
        pad4(&mut v);
        v.extend_from_slice(value);
        for c in children {
            pad4(&mut v);
            v.extend_from_slice(c);
        }
        let len = v.len() as u16;
        v[0..2].copy_from_slice(&len.to_le_bytes());
        v
    }
    fn string(key: &str, val: &str) -> Vec<u8> {
        let data = w(val);
        node(key, &data, (data.len() / 2) as u16, true, &[])
    }

    fn version_info(desc: &str, file: &str, dll: bool) -> Vec<u8> {
        let version = version();
        let ms = ((version[0] as u32) << 16) | version[1] as u32;
        let ls = ((version[2] as u32) << 16) | version[3] as u32;
        let mut fixed = Vec::new();
        let display = display_version();
        // test builds are flagged as prerelease (VS_FF_PRERELEASE)
        let flags = if display.contains('-') { 2 } else { 0 };
        for d in [0xFEEF04BDu32, 0x0001_0000, ms, ls, ms, ls, 0x3F, flags, 0x0004_0004, if dll { 2 } else { 1 }, 0, 0, 0] {
            fixed.extend_from_slice(&d.to_le_bytes());
        }
        let ver = format!("{}.{}.{}.{}", version[0], version[1], version[2], version[3]);
        let author = author();
        let table = node("040904B0", &[], 0, true, &[
            string("CompanyName", &author),
            string("FileDescription", desc),
            string("FileVersion", &ver),
            string("InternalName", file.trim_end_matches(".exe").trim_end_matches(".dll")),
            string("LegalCopyright", &format!("Copyright (c) {author}")),
            string("OriginalFilename", file),
            string("ProductName", "Crow FO"),
            string("ProductVersion", &display),
        ]);
        let sfi = node("StringFileInfo", &[], 0, true, &[table]);
        let mut tr = Vec::new();
        tr.extend_from_slice(&LANG.to_le_bytes());
        tr.extend_from_slice(&1200u16.to_le_bytes());
        let var = node("Translation", &tr, 4, false, &[]);
        let vfi = node("VarFileInfo", &[], 0, true, &[var]);
        node("VS_VERSION_INFO", &fixed, fixed.len() as u16, false, &[sfi, vfi])
    }

    const MANIFEST: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<assembly xmlns="urn:schemas-microsoft-com:asm.v1" manifestVersion="1.0">
  <assemblyIdentity type="win32" name="Crow.FolderOrganizer" version="{VERSION}" processorArchitecture="amd64"/>
  <description>Crow FO</description>
  <trustInfo xmlns="urn:schemas-microsoft-com:asm.v3">
    <security><requestedPrivileges><requestedExecutionLevel level="asInvoker" uiAccess="false"/></requestedPrivileges></security>
  </trustInfo>
  <compatibility xmlns="urn:schemas-microsoft-com:compatibility.v1">
    <application><supportedOS Id="{8e0f7a12-bfb3-4fe8-b9a5-48fd50a15a9a}"/></application>
  </compatibility>
  <application xmlns="urn:schemas-microsoft-com:asm.v3">
    <windowsSettings>
      <dpiAware xmlns="http://schemas.microsoft.com/SMI/2005/WindowsSettings">true/pm</dpiAware>
      <dpiAwareness xmlns="http://schemas.microsoft.com/SMI/2016/WindowsSettings">PerMonitorV2</dpiAwareness>
    </windowsSettings>
  </application>
  <dependency>
    <dependentAssembly>
      <assemblyIdentity type="win32" name="Microsoft.Windows.Common-Controls" version="6.0.0.0" processorArchitecture="*" publicKeyToken="6595b64144ccf1df" language="*"/>
    </dependentAssembly>
  </dependency>
</assembly>
"#;

    /// One .res entry with ordinal type and id.
    fn entry(out: &mut Vec<u8>, ty: u16, id: u16, flags: u16, data: &[u8]) {
        out.extend_from_slice(&(data.len() as u32).to_le_bytes());
        out.extend_from_slice(&32u32.to_le_bytes());
        out.extend_from_slice(&[0xFF, 0xFF]);
        out.extend_from_slice(&ty.to_le_bytes());
        out.extend_from_slice(&[0xFF, 0xFF]);
        out.extend_from_slice(&id.to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes()); // DataVersion
        out.extend_from_slice(&flags.to_le_bytes());
        out.extend_from_slice(&LANG.to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes()); // Version
        out.extend_from_slice(&0u32.to_le_bytes()); // Characteristics
        out.extend_from_slice(data);
        pad4(out);
    }

    /// Splits a .ico file into RT_ICON images and RT_GROUP_ICON `gid`. Image ids
    /// start after `*next` so several icons can share one file.
    fn icon(out: &mut Vec<u8>, ico: &[u8], gid: u16, next: &mut u16) {
        let rd16 = |o: usize| u16::from_le_bytes([ico[o], ico[o + 1]]);
        let rd32 = |o: usize| u32::from_le_bytes([ico[o], ico[o + 1], ico[o + 2], ico[o + 3]]);
        let n = rd16(4) as usize;
        let mut group = vec![0, 0, 1, 0];
        group.extend_from_slice(&(n as u16).to_le_bytes());
        for i in 0..n {
            let e = 6 + i * 16;
            let (size, off) = (rd32(e + 8) as usize, rd32(e + 12) as usize);
            *next += 1;
            entry(out, 3, *next, 0x1010, &ico[off..off + size]);
            group.extend_from_slice(&ico[e..e + 8]); // w, h, colors, reserved, planes, bpp
            group.extend_from_slice(&(size as u32).to_le_bytes());
            group.extend_from_slice(&next.to_le_bytes());
        }
        entry(out, 14, gid, 0x1030, &group);
    }

    /// `icons[0]` becomes icon #1 (the file icon), the next ones #2, #3...
    pub fn write(path: &std::path::Path, desc: &str, file: &str, dll: bool, icons: &[&[u8]]) {
        write_with(path, desc, file, dll, icons, None);
    }

    /// Same as `write`, plus an optional binary payload stored as RCDATA #1
    /// (the installer carries CrowFO.exe this way).
    pub fn write_with(path: &std::path::Path, desc: &str, file: &str, dll: bool, icons: &[&[u8]], rcdata: Option<&[u8]>) {
        let mut out = vec![0u8; 32];
        out[4] = 32;
        out[8..12].copy_from_slice(&[0xFF, 0xFF, 0, 0]);
        out[12..16].copy_from_slice(&[0xFF, 0xFF, 0, 0]);
        if let Some(data) = rcdata {
            entry(&mut out, 10, 1, 0x0030, data); // RT_RCDATA
        }
        let mut next = 0;
        for (i, ico) in icons.iter().enumerate() {
            icon(&mut out, ico, i as u16 + 1, &mut next);
        }
        entry(&mut out, 16, 1, 0x0030, &version_info(desc, file, dll));
        if !dll {
            let v = version();
            let manifest = MANIFEST.replace("{VERSION}", &format!("{}.{}.{}.{}", v[0], v[1], v[2], v[3]));
            entry(&mut out, 24, 1, 0x0030, manifest.as_bytes());
        }
        std::fs::File::create(path).unwrap().write_all(&out).unwrap();
        // the app reads it with env!("CROW_VERSION")
        println!("cargo:rustc-env=CROW_VERSION={}", display_version());
        println!("cargo:rerun-if-env-changed=CROW_BUILD");
        println!("cargo:rerun-if-env-changed=CROW_VERSION_LABEL");
    }
}
