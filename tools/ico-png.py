"""Rewrites the 32-bpp BMP images inside .ico files as compressed PNG.

The result is pixel for pixel the same, only much smaller, which keeps
CrowFO.exe and CrowFOSetup.exe light. Windows reads PNG icon images of any
size. Run it after `CrowFO.exe --export-art packaging`:

    python tools/ico-png.py packaging/*.ico

Images that are already PNG are kept as they are.
"""
import struct
import sys
import zlib


def png(w, h, rgba):
    raw = b''.join(b'\0' + rgba[y * w * 4:(y + 1) * w * 4] for y in range(h))

    def chunk(t, d):
        return struct.pack('>I', len(d)) + t + d + struct.pack('>I', zlib.crc32(t + d) & 0xFFFFFFFF)

    return (b'\x89PNG\r\n\x1a\n'
            + chunk(b'IHDR', struct.pack('>IIBBBBB', w, h, 8, 6, 0, 0, 0))
            + chunk(b'IDAT', zlib.compress(raw, 9))
            + chunk(b'IEND', b''))


def convert(path):
    d = open(path, 'rb').read()
    n = struct.unpack_from('<H', d, 4)[0]
    imgs = []
    for i in range(n):
        w, h, _, _, _, bpp, size, off = struct.unpack_from('<BBBBHHII', d, 6 + 16 * i)
        blob = d[off:off + size]
        if blob[:4] == b'\x89PNG':
            imgs.append((w, h, bpp, blob))
            continue
        if bpp != 32:
            sys.exit(f'{path}: only 32-bpp images are supported')
        W, H = w or 256, h or 256
        hs = struct.unpack_from('<I', blob, 0)[0]
        px = blob[hs:hs + W * H * 4]
        rgba = bytearray()
        for y in range(H - 1, -1, -1):  # BMP rows go bottom up
            row = px[y * W * 4:(y + 1) * W * 4]
            for k in range(W):
                b, g, r, a = row[k * 4:k * 4 + 4]
                rgba += bytes((r, g, b, a))
        imgs.append((w, h, bpp, png(W, H, bytes(rgba))))
    out = struct.pack('<HHH', 0, 1, n)
    off = 6 + 16 * n
    for w, h, bpp, data in imgs:
        out += struct.pack('<BBBBHHII', w, h, 0, 0, 1, bpp, len(data), off)
        off += len(data)
    out += b''.join(x[3] for x in imgs)
    open(path, 'wb').write(out)
    print(f'{path}: {len(d)} -> {len(out)} bytes')


for p in sys.argv[1:]:
    convert(p)
