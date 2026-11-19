#!/usr/bin/env python3
"""Google Play screenshots from raw captures of the Android app.

Takes screencaps of the real app (an emulator per size, see
packaging/android/play/README.md), crops tablets' taskbar, adds the caption
pill the App Store set uses, and writes 24-bit PNGs (Play wants no alpha) to
packaging/android/play/<size>/NN-<name>.png.

    python3 scripts/ad/play-shots.py RAWDIR
RAWDIR holds phone/, tab7/ and tab10/, each with street.png, deep.png,
diagrams.png, design.png and sketch.png.
"""
import sys
from PIL import Image, ImageDraw, ImageFont

RAW = sys.argv[1]
OUT = "packaging/android/play"
FONT = "/usr/share/fonts/noto/NotoSans-Bold.ttf"

SHOTS = [  # (raw name, file name, caption)
    ("street", "01-street", "A street that never ends"),
    ("diagrams", "02-diagrams", "Diagrams that stay connected"),
    ("design", "03-design", "Gradients, curves, words on a path"),
    ("deep", "04-deep", "10²⁴ times deeper. Sharp."),
    ("sketch", "05-tools", "Your tools, kept as you set them"),
]
# Per size: caption font px, caption top px (over the status bar, above the
# settings button), and the height kept (tablets lose their taskbar, 120 px
# at 320 dpi).
SIZES = {
    "phone": dict(font=54, top=28, keep=None),
    "tab7": dict(font=60, top=28, keep=1800),
    "tab10": dict(font=70, top=28, keep=2440),
}

for size, o in SIZES.items():
    font = ImageFont.truetype(FONT, o["font"])
    for raw, name, text in SHOTS:
        im = Image.open(f"{RAW}/{size}/{raw}.png").convert("RGB")
        if o["keep"]:
            im = im.crop((0, 0, im.width, o["keep"]))
        d = ImageDraw.Draw(im, "RGBA")
        l, t, r, b = d.textbbox((0, 0), text, font=font)
        px, py = round(o["font"] * 0.8), round(o["font"] * 0.42)
        w, h = r - l + 2 * px, b - t + 2 * py
        x0, y0 = (im.width - w) // 2, o["top"]
        d.rounded_rectangle((x0, y0, x0 + w, y0 + h), radius=round(h * 0.38), fill=(28, 28, 36, 230))
        d.text((x0 + px - l, y0 + py - t), text, font=font, fill=(255, 255, 255, 255))
        path = f"{OUT}/{size}/{name}.png"
        im.save(path, optimize=True)
        print(path, im.size)
