//! The Crow logo, and `CrowFO.exe --export-art <dir>`, which writes crow.ico and
//! the package logos from it. Folders keep their own folder icon.

use crate::util::*;
use alloc::vec::Vec;

/// A crow on an amber tile, in the same color as the default folders.
const LOGO: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 256 256">
<defs><linearGradient id="t" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="#FFCF5C"/><stop offset="1" stop-color="#EE9A2E"/></linearGradient></defs>
<rect x="16" y="16" width="224" height="224" rx="54" fill="url(#t)"/>
<g transform="translate(-10 2)">
<path fill="#17181C" d="M114 104 C116 78 134 58 158 57 C173 56 185 63 192 72 L234 92 C220 99 206 104 192 106 C197 128 190 153 170 170 C154 184 132 191 110 191 L58 214 L70 196 L46 197 C66 182 82 167 92 150 C100 136 107 120 114 104 Z"/>
<path fill="#2C2E36" d="M122 112 C137 116 150 130 151 148 C152 165 139 178 115 185 L80 194 C95 177 105 161 110 144 C113 131 116 120 122 112 Z"/>
<path fill="#2C2E36" d="M192 72 L234 92 C224 92 206 89 192 86 Z"/>
<circle cx="166" cy="76" r="6.5" fill="#F5B841"/>
<path d="M136 189 L132 210 M154 184 L152 210" stroke="#17181C" stroke-width="7" stroke-linecap="round"/>
</g></svg>"##;

/// The logo at s by s pixels, premultiplied BGRA.
pub fn logo(s: i32) -> Vec<u32> {
    crate::svg::render(LOGO.as_bytes(), s, 0).unwrap_or_else(|| alloc::vec![0; (s * s) as usize])
}

/// Converts a premultiplied BGRA pixel to straight RGBA.
fn straight(c: u32) -> [u8; 4] {
    let a = c >> 24;
    if a == 0 {
        return [0, 0, 0, 0];
    }
    let un = |v: u32| ((v * 255 + a / 2) / a).min(255) as u8;
    [un((c >> 16) & 255), un((c >> 8) & 255), un(c & 255), a as u8]
}

fn crc_table() -> [u32; 256] {
    let mut t = [0u32; 256];
    for (n, e) in t.iter_mut().enumerate() {
        let mut c = n as u32;
        for _ in 0..8 {
            c = if c & 1 != 0 { 0xEDB8_8320 ^ (c >> 1) } else { c >> 1 };
        }
        *e = c;
    }
    t
}

fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8], t: &[u32; 256]) {
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    let start = out.len();
    out.extend_from_slice(kind);
    out.extend_from_slice(data);
    let mut c = 0xFFFF_FFFFu32;
    for &b in &out[start..] {
        c = t[((c ^ b as u32) & 255) as usize] ^ (c >> 8);
    }
    out.extend_from_slice(&(!c).to_be_bytes());
}

/// A minimal PNG encoder. It writes RGBA8 with uncompressed zlib blocks, which
/// keeps the code tiny because no deflate implementation is needed.
pub fn png(px: &[u32], s: i32) -> Vec<u8> {
    png_wh(px, s, s)
}

pub fn png_wh(px: &[u32], w: i32, h: i32) -> Vec<u8> {
    let s = w;
    let t = crc_table();
    let mut raw = Vec::with_capacity((h * (s * 4 + 1)) as usize);
    for y in 0..h {
        raw.push(0); // no filter
        for x in 0..s {
            raw.extend_from_slice(&straight(px[(y * s + x) as usize]));
        }
    }
    let mut z = alloc::vec![0x78, 0x01];
    for (i, block) in raw.chunks(65535).enumerate() {
        let last = (i + 1) * 65535 >= raw.len();
        z.push(last as u8);
        let n = block.len() as u16;
        z.extend_from_slice(&n.to_le_bytes());
        z.extend_from_slice(&(!n).to_le_bytes());
        z.extend_from_slice(block);
    }
    let (mut a, mut b) = (1u32, 0u32);
    for &v in &raw {
        a = (a + v as u32) % 65521;
        b = (b + a) % 65521;
    }
    z.extend_from_slice(&((b << 16) | a).to_be_bytes());

    let mut out = alloc::vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&(w as u32).to_be_bytes());
    ihdr.extend_from_slice(&(h as u32).to_be_bytes());
    ihdr.extend_from_slice(&[8, 6, 0, 0, 0]);
    chunk(&mut out, b"IHDR", &ihdr, &t);
    chunk(&mut out, b"IDAT", &z, &t);
    chunk(&mut out, b"IEND", &[], &t);
    out
}

/// Multi-size .ico with 32-bpp BMP entries.
fn ico(sizes: &[i32]) -> Vec<u8> {
    let imgs: Vec<Vec<u8>> = sizes
        .iter()
        .map(|&s| {
            let px = logo(s);
            let mut d = Vec::new();
            for v in [40u32, s as u32, (s * 2) as u32] {
                d.extend_from_slice(&v.to_le_bytes());
            }
            d.extend_from_slice(&1u16.to_le_bytes());
            d.extend_from_slice(&32u16.to_le_bytes());
            d.extend_from_slice(&[0u8; 24]);
            for y in (0..s).rev() {
                for x in 0..s {
                    let [r, g, b, a] = straight(px[(y * s + x) as usize]);
                    d.extend_from_slice(&[b, g, r, a]);
                }
            }
            let mask_row = (((s + 31) / 32) * 4) as usize;
            d.resize(d.len() + mask_row * s as usize, 0);
            d
        })
        .collect();
    let mut out = Vec::new();
    out.extend_from_slice(&[0, 0, 1, 0]);
    out.extend_from_slice(&(sizes.len() as u16).to_le_bytes());
    let mut off = 6 + 16 * sizes.len();
    for (i, &s) in sizes.iter().enumerate() {
        let dim = if s >= 256 { 0 } else { s as u8 };
        out.extend_from_slice(&[dim, dim, 0, 0]);
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&32u16.to_le_bytes());
        out.extend_from_slice(&(imgs[i].len() as u32).to_le_bytes());
        out.extend_from_slice(&(off as u32).to_le_bytes());
        off += imgs[i].len();
    }
    for d in &imgs {
        out.extend_from_slice(d);
    }
    out
}

pub fn export(dir: &[u16]) -> bool {
    mkdir(dir);
    let assets = wcat(&[dir, w!("\\Assets")]);
    mkdir(&assets);
    let mut ok = write_file(&wcat(&[dir, w!("\\crow.ico")]), &ico(&[16, 20, 24, 32, 40, 48, 64, 256]));
    // A smaller set is embedded in CrowFO.exe to keep the binary small
    ok &= write_file(&wcat(&[dir, w!("\\crow_embed.ico")]), &ico(&[16, 32, 48]));
    for (name, s) in [(w!("\\Logo44.png") as &[u16], 44), (w!("\\Logo150.png"), 150), (w!("\\StoreLogo.png"), 50)] {
        ok &= write_file(&wcat(&[&assets, name]), &png(&logo(s), s));
    }
    ok
}
