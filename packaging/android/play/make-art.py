#!/usr/bin/env python3
"""Google Play listing art: the 512 x 512 icon and the 1024 x 500 feature graphic.

Both from the app icon (packaging/apple/icon-1024.svg): off-white lines on
#16181d, no gradients. Play rounds the icon's corners itself, so it is the
full square. Needs rsvg-convert and ImageMagick. Run from the repo root:
    python3 packaging/android/play/make-art.py
"""
import re
import subprocess

HERE = "packaging/android/play"
BG, FG = "#16181d", "#eef0e8"

icon = open("packaging/apple/icon-1024.svg").read()
art = re.sub(r"^<svg[^>]*>|</svg>\s*$", "", icon.strip())
art = art.replace(f'<rect width="1024" height="1024" fill="{BG}"/>', "", 1)

feature = f"""<svg xmlns="http://www.w3.org/2000/svg" width="1024" height="500" viewBox="0 0 1024 500">
<rect width="1024" height="500" fill="{BG}"/>
<g transform="translate(70 70) scale({360 / 1024:.5f})">{art}</g>
<text x="470" y="235" font-family="Noto Sans, sans-serif" font-weight="800" font-size="104" fill="{FG}">OG Paper</text>
<text x="474" y="305" font-family="Noto Sans, sans-serif" font-weight="600" font-size="36" fill="{FG}" fill-opacity="0.75">An endless canvas.</text>
<text x="474" y="352" font-family="Noto Sans, sans-serif" font-weight="600" font-size="36" fill="{FG}" fill-opacity="0.75">Zoom in forever.</text>
</svg>
"""
open(f"{HERE}/feature-graphic.svg", "w").write(feature)

png = subprocess.run(["rsvg-convert", "-w", "1024", "-h", "500", f"{HERE}/feature-graphic.svg"],
                     check=True, capture_output=True).stdout
# Play wants the feature graphic without alpha (24-bit PNG or JPEG).
subprocess.run(["magick", "-", "-background", BG, "-flatten", "-strip", f"PNG24:{HERE}/feature-graphic.png"],
               input=png, check=True)
subprocess.run(["magick", "packaging/apple/icon-1024.png", "-resize", "512x512", "-strip",
                f"PNG32:{HERE}/icon-512.png"], check=True)
print(f"wrote {HERE}/feature-graphic.png, {HERE}/icon-512.png")
