# The splash (branding/splash.png, 1280 x 720) as the FANTOM's console frame buffer takes it: the
# 720 x 1280 panel's own rows, the picture turned a quarter turn clockwise (the profile's rotation,
# 90), 32-bit XRGB little-endian (B, G, R, 0 per pixel), gzipped; oscairn-fantom-start writes it to
# /dev/fb0. Only the standard library, for the bundle's build:
#   python3 splash_raw.py splash.png splash-720x1280.bgrx.gz
import gzip, struct, sys, zlib

def read_png(path):
    """Rows of RGB bytes from an 8-bit, non-interlaced RGB or RGBA PNG."""
    data = open(path, "rb").read()
    assert data[:8] == b"\x89PNG\r\n\x1a\n", "not a PNG"
    pos, idat = 8, b""
    while pos < len(data):
        n, kind = struct.unpack(">I4s", data[pos:pos + 8])
        body = data[pos + 8:pos + 8 + n]
        if kind == b"IHDR":
            w, h, depth, color, _, _, interlace = struct.unpack(">IIBBBBB", body)
            assert depth == 8 and color in (2, 6) and interlace == 0, "only 8-bit RGB or RGBA, not interlaced"
        elif kind == b"IDAT":
            idat += body
        pos += 12 + n
    bpp = 3 if color == 2 else 4
    raw, stride, prev, rows = zlib.decompress(idat), w * bpp, bytearray(w * bpp), []
    for y in range(h):
        f, line = raw[y * (stride + 1)], bytearray(raw[y * (stride + 1) + 1:(y + 1) * (stride + 1)])
        for i in range(stride):
            a = line[i - bpp] if i >= bpp else 0
            b, c = prev[i], prev[i - bpp] if i >= bpp else 0
            if f == 1: line[i] = (line[i] + a) & 255
            elif f == 2: line[i] = (line[i] + b) & 255
            elif f == 3: line[i] = (line[i] + (a + b) // 2) & 255
            elif f == 4:
                p = a + b - c
                pa, pb, pc = abs(p - a), abs(p - b), abs(p - c)
                line[i] = (line[i] + (a if pa <= pb and pa <= pc else b if pb <= pc else c)) & 255
        rows.append(bytes(line) if bpp == 3 else bytes(v for k, v in enumerate(line) if k % 4 != 3))
        prev = line
    return w, h, rows

w, h, rows = read_png(sys.argv[1])
assert (w, h) == (1280, 720), f"the splash must be 1280 x 720, not {w} x {h}"
out = bytearray()
for v in range(w):  # the panel's row v: the picture's column v, from its bottom up
    for u in range(h):
        r, g, b = rows[h - 1 - u][v * 3:v * 3 + 3]
        out += bytes((b, g, r, 0))
with gzip.open(sys.argv[2], "wb", 9) as f:
    f.write(out)
