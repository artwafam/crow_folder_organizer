// Shared by both build scripts through include!. It writes a Win32 .res file
// with version info, an application manifest and optionally the app icon.
// rust-lld links it straight into the binary, so neither rc.exe nor the
// Windows SDK is needed. Binaries that carry proper version info and a
// manifest look like normal software to antivirus heuristics.

#[allow(dead_code)]
mod res {
    use std::io::Write;

    pub const LANG: u16 = 0x0416; // pt-BR
    const VERSION: [u16; 4] = [1, 0, 0, 0];

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
        let ms = ((VERSION[0] as u32) << 16) | VERSION[1] as u32;
        let ls = ((VERSION[2] as u32) << 16) | VERSION[3] as u32;
        let mut fixed = Vec::new();
        for d in [0xFEEF04BDu32, 0x0001_0000, ms, ls, ms, ls, 0x3F, 0, 0x0004_0004, if dll { 2 } else { 1 }, 0, 0, 0] {
            fixed.extend_from_slice(&d.to_le_bytes());
        }
        let ver = format!("{}.{}.{}.{}", VERSION[0], VERSION[1], VERSION[2], VERSION[3]);
        let table = node("041604B0", &[], 0, true, &[
            string("CompanyName", "Crow"),
            string("FileDescription", desc),
            string("FileVersion", &ver),
            string("InternalName", file.trim_end_matches(".exe").trim_end_matches(".dll")),
            string("LegalCopyright", "Copyright (c) Crow"),
            string("OriginalFilename", file),
            string("ProductName", "Crow"),
            string("ProductVersion", &ver),
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
  <assemblyIdentity type="win32" name="Crow.FolderOrganizer" version="1.0.0.0" processorArchitecture="amd64"/>
  <description>Crow - organizador de pastas</description>
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

    /// Splits a .ico file into RT_ICON images and an RT_GROUP_ICON directory.
    fn icon(out: &mut Vec<u8>, ico: &[u8]) {
        let rd16 = |o: usize| u16::from_le_bytes([ico[o], ico[o + 1]]);
        let rd32 = |o: usize| u32::from_le_bytes([ico[o], ico[o + 1], ico[o + 2], ico[o + 3]]);
        let n = rd16(4) as usize;
        let mut group = vec![0, 0, 1, 0];
        group.extend_from_slice(&(n as u16).to_le_bytes());
        for i in 0..n {
            let e = 6 + i * 16;
            let (size, off) = (rd32(e + 8) as usize, rd32(e + 12) as usize);
            entry(out, 3, (i + 1) as u16, 0x1010, &ico[off..off + size]);
            group.extend_from_slice(&ico[e..e + 8]); // w, h, colors, reserved, planes, bpp
            group.extend_from_slice(&(size as u32).to_le_bytes());
            group.extend_from_slice(&((i + 1) as u16).to_le_bytes());
        }
        entry(out, 14, 1, 0x1030, &group);
    }

    pub fn write(path: &std::path::Path, desc: &str, file: &str, dll: bool, ico: Option<&[u8]>) {
        write_with(path, desc, file, dll, ico, None);
    }

    /// Same as `write`, plus an optional binary payload stored as RCDATA #1
    /// (the installer carries crow.exe this way).
    pub fn write_with(path: &std::path::Path, desc: &str, file: &str, dll: bool, ico: Option<&[u8]>, rcdata: Option<&[u8]>) {
        let mut out = vec![0u8; 32];
        out[4] = 32;
        out[8..12].copy_from_slice(&[0xFF, 0xFF, 0, 0]);
        out[12..16].copy_from_slice(&[0xFF, 0xFF, 0, 0]);
        if let Some(data) = rcdata {
            entry(&mut out, 10, 1, 0x0030, data); // RT_RCDATA
        }
        if let Some(ico) = ico {
            icon(&mut out, ico);
        }
        entry(&mut out, 16, 1, 0x0030, &version_info(desc, file, dll));
        if !dll {
            entry(&mut out, 24, 1, 0x0030, MANIFEST.as_bytes());
        }
        std::fs::File::create(path).unwrap().write_all(&out).unwrap();
    }
}
