#!/usr/bin/env python3
"""Draws the OG Paper icon and writes every size the apps use.

The icon: three rounded boxes nested toward the lower-right corner (zooming
into it) with a paintbrush over them, off-white on near-black (the OG
TestDesk style), the brush tip in OG Paper's pink.

Needs rsvg-convert (librsvg) and ImageMagick. Run from the repo root:
    python3 packaging/make-icons.py
"""
import os
import subprocess
import tempfile

BG, FG, PINK = "#16181d", "#eef0e8", "#e2457a"
W = 60  # line width on the 1024 grid

# Boxes: outer, middle, inner (x0, y0, x1, y1), and their line widths.
BOXES = [((196, 196, 828, 828), W), ((392, 392, 732, 732), W * 0.82), ((528, 528, 664, 664), W * 0.66)]


def art():
    """The icon's lines on a 1024 grid (no background)."""
    out = []
    for (x0, y0, x1, y1), w in BOXES:
        r = (x1 - x0) * 0.17
        out.append(f'<rect x="{x0}" y="{y0}" width="{x1 - x0}" height="{y1 - y0}" rx="{r:.0f}" '
                   f'fill="none" stroke="{FG}" stroke-width="{w:.0f}"/>')
    # The brush, along +x with its tip at the origin, then turned onto the
    # smallest box. A dark halo keeps it apart from the boxes under it.
    s, hw, L = 0.78, 34, 470
    handle = f"M {150*s} {-hw*s} L {L*s} {-(hw+10)*s} Q {(L+46)*s} 0 {L*s} {(hw+10)*s} L {150*s} {hw*s} Z"
    fx, fy, fw, fh, fr = 88 * s, -46 * s, 82 * s, 92 * s, 16 * s
    tip = f"M {96*s} {-48*s} C {36*s} {-48*s} {-8*s} {-16*s} 0 0 C {-8*s} {16*s} {36*s} {48*s} {96*s} {48*s} Z"
    out.append(
        f'<g transform="translate(596 596) rotate(-45)">'
        f'<g fill="{BG}" stroke="{BG}" stroke-width="64" stroke-linejoin="round">'
        f'<path d="{handle}"/><rect x="{fx}" y="{fy}" width="{fw}" height="{fh}"/><path d="{tip}"/></g>'
        f'<path d="{handle}" fill="{BG}" stroke="{FG}" stroke-width="{W*0.6:.0f}" stroke-linejoin="round"/>'
        f'<rect x="{fx}" y="{fy}" width="{fw}" height="{fh}" rx="{fr}" fill="{FG}"/>'
        f'<path d="{tip}" fill="{PINK}" stroke="{FG}" stroke-width="{W*0.3:.0f}" stroke-linejoin="round"/>'
        f"</g>")
    return "".join(out)


def svg(kind):
    """full: square, edge to edge (iOS and the stores round it themselves).
    tile: rounded square with transparent corners (web, Android, favicon).
    mac: the macOS plate (824 in a 1024 frame, with a soft shadow)."""
    head = '<svg xmlns="http://www.w3.org/2000/svg" width="1024" height="1024" viewBox="0 0 1024 1024">'
    if kind == "full":
        return f'{head}<rect width="1024" height="1024" fill="{BG}"/>{art()}</svg>'
    if kind == "tile":
        return f'{head}<rect width="1024" height="1024" rx="225" fill="{BG}"/>{art()}</svg>'
    k = 824 / 1024
    return (f'{head}<defs><filter id="s" x="-10%" y="-10%" width="120%" height="125%">'
            f'<feDropShadow dx="0" dy="10" stdDeviation="12" flood-opacity="0.35"/></filter></defs>'
            f'<rect x="100" y="100" width="824" height="824" rx="185" fill="{BG}" filter="url(#s)"/>'
            f'<g transform="translate(100 100) scale({k})">{art()}</g></svg>')


def render(kind, size, out, flatten=False):
    os.makedirs(os.path.dirname(out) or ".", exist_ok=True)
    with tempfile.NamedTemporaryFile("w", suffix=".svg", delete=False) as f:
        f.write(svg(kind))
    subprocess.run(["rsvg-convert", "-w", str(size), "-h", str(size), f.name, "-o", out], check=True)
    os.unlink(f.name)
    if flatten:
        # App Store icons must have no transparency.
        subprocess.run(["magick", out, "-background", BG, "-alpha", "remove", "-alpha", "off", out], check=True)


if __name__ == "__main__":
    for kind, path in (("full", "packaging/apple/icon-1024.svg"), ("mac", "packaging/apple/icon-mac-1024.svg"),
                       ("tile", "packaging/icon.svg")):
        open(path, "w").write(svg(kind) + "\n")
    render("full", 1024, "packaging/apple/icon-1024.png", flatten=True)
    render("mac", 1024, "packaging/apple/icon-mac-1024.png")
    render("tile", 192, "web/app/icon-192.png")
    render("tile", 512, "web/app/icon-512.png")
    render("full", 512, "web/app/icon-maskable-512.png")
    for name, size in (("mdpi", 48), ("hdpi", 72), ("xhdpi", 96), ("xxhdpi", 144), ("xxxhdpi", 192)):
        render("tile", size, f"crates/og-paper/res/mipmap-{name}/ic_launcher.png")
    print("icons written")
