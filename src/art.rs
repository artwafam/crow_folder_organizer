//! Handles `crow.exe --export-art <dir>`. It writes crow.ico and the package
//! logos, drawn from the same vector folder icon the app uses.

use crate::glyph;
use crate::util::*;
use alloc::vec::Vec;

const COLOR: u32 = 0xFFF5B841;

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
            let px = glyph::folder(s, COLOR, 0.0);
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
    // A smaller set is embedded in crow.exe to keep the binary small
    ok &= write_file(&wcat(&[dir, w!("\\crow_embed.ico")]), &ico(&[16, 32, 48]));
    for (name, s) in [(w!("\\Logo44.png") as &[u16], 44), (w!("\\Logo150.png"), 150), (w!("\\StoreLogo.png"), 50)] {
        ok &= write_file(&wcat(&[&assets, name]), &png(&glyph::folder(s, COLOR, 0.0), s));
    }
    ok
}
