# Draws the device splash (splash.png, 1280 x 720): the mark and "Oscairn" centered on the
# theme's background, shown while a FANTOM's display passes from Roland's to Oscairn's at power-on
# (devices/roland-fantom0/bundle/oscairn-fantom-start). Needs Pillow:
#   python3 branding/splash.py
# The bundle's build turns it for the panel (devices/roland-fantom0/tools/splash_raw.py).
import re
from pathlib import Path
from PIL import Image, ImageDraw, ImageFont

here = Path(__file__).resolve().parent
font = here.parent / "hosts/standalone/themes/oscairn-classic/fonts/Inter-Bold.ttf"
background, light = "#15181D", "#f3efe8"  # the classic theme's background; the mark's light
width, height, ss = 1280, 720, 4  # drawn 4 times larger, then scaled down for smooth edges
mark_size, gap, text_size = 220, 36, 84

img = Image.new("RGB", (width * ss, height * ss), background)
draw = ImageDraw.Draw(img)
text_font = ImageFont.truetype(str(font), text_size * ss)
left, top, right, bottom = draw.textbbox((0, 0), "Oscairn", font=text_font)
block = mark_size * ss + gap * ss + (bottom - top)
y0 = (height * ss - block) // 2
x0 = (width * ss - mark_size * ss) // 2
scale = mark_size * ss / 100

# The mark's paths (mark-on-dark.svg): "M x y Q cx cy x y T x y", round caps and joins.
svg = (here / "mark-on-dark.svg").read_text()
for d, color, stroke in re.findall(r'<path d="([^"]+)" fill="none" stroke="([^"]+)" stroke-width="([^"]+)"', svg):
    n = [float(v) for v in re.findall(r"-?[\d.]+", d)]
    p0, c, p1, p2 = (n[0], n[1]), (n[2], n[3]), (n[4], n[5]), (n[6], n[7])
    c2 = (2 * p1[0] - c[0], 2 * p1[1] - c[1])  # T: the control point reflected
    points = []
    for a, b, e in ((p0, c, p1), (p1, c2, p2)):
        for i in range(101):
            t = i / 100
            q = [(1 - t) ** 2 * a[k] + 2 * (1 - t) * t * b[k] + t * t * e[k] for k in (0, 1)]
            points.append((x0 + scale * q[0], y0 + scale * q[1]))
    w = float(stroke) * scale
    draw.line(points, fill=color, width=round(w))
    for x, y in points:
        draw.ellipse((x - w / 2, y - w / 2, x + w / 2, y + w / 2), fill=color)

draw.text(((width * ss - (right - left)) // 2 - left, y0 + (mark_size + gap) * ss - top), "Oscairn", font=text_font, fill=light)
img.resize((width, height), Image.LANCZOS).save(here / "splash.png", optimize=True)
