"""Builds docs/demo.gif from the frames that `CrowFO.exe --demo` writes.

The demo needs a data folder with one hover folder and one panel, for example:

    set CROW_DATA=C:\\path\\to\\demo-data
    CrowFO.exe --demo C:\\path\\to\\frames
    python tools/demo-gif.py C:\\path\\to\\frames docs/demo.gif

It needs Pillow (pip install pillow). The frames are raw and large, so delete
the frames folder afterwards.
"""
import os
import sys

from PIL import Image, ImageDraw

src, out = sys.argv[1], sys.argv[2]
lines = open(os.path.join(src, "cursor.txt")).read().split("\n")
w, h, n, dt = map(int, lines[0].split())
cur = [tuple(map(int, l.split())) for l in lines[1:1 + n]]


def arrow(scale):
    """A white arrow pointer with a dark outline, drawn 4x and scaled down."""
    k = 4
    pts = [(0, 0), (0, 17), (4.2, 13.2), (7, 19.5), (9.6, 18.4), (6.9, 12.3), (12.2, 12.3)]
    s = scale * k
    img = Image.new("RGBA", (int(16 * s), int(24 * s)), (0, 0, 0, 0))
    d = ImageDraw.Draw(img)
    poly = [(1.5 * s + x * s, 1.5 * s + y * s) for x, y in pts]
    d.polygon(poly, fill=(255, 255, 255, 255), outline=(20, 20, 24, 255), width=int(1.3 * s))
    return img.resize((img.width // k, img.height // k), Image.LANCZOS)


# The pointer shrinks a little while the button is pressed.
normal, pressed = arrow(1.25), arrow(1.1)

frames = []
for i in range(n):
    raw = open(os.path.join(src, "f%04d.bgra" % i), "rb").read()
    im = Image.frombuffer("RGBA", (w, h), raw, "raw", "BGRA", 0, 1).convert("RGB")
    x, y, p = cur[i]
    c = pressed if p else normal
    im.paste(c, (x - 2, y - 2), c)
    frames.append(im)

# One palette for the whole animation, taken from a spread of frames.
picks = frames[:: max(1, n // 16)]
montage = Image.new("RGB", (w, h * len(picks)))
for k, f in enumerate(picks):
    montage.paste(f, (0, h * k))
pal = montage.quantize(colors=255, method=Image.Quantize.MEDIANCUT)
q = [f.quantize(palette=pal, dither=Image.Dither.NONE) for f in frames]

# Identical frames become one longer frame, which keeps the file small.
out_frames, durations = [], []
for f in q:
    if out_frames and f.tobytes() == out_frames[-1].tobytes():
        durations[-1] += dt
    else:
        out_frames.append(f)
        durations.append(dt)
out_frames[0].save(out, save_all=True, append_images=out_frames[1:], duration=durations, loop=0, disposal=1)
print("%d frames, %d KB" % (len(out_frames), os.path.getsize(out) // 1024))
