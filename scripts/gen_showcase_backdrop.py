#!/usr/bin/env python3
"""Generate the abstract "backdrop" asset for the showcase fixture (stdlib only).

1600x1400 RGB PNG: deep espresso base with soft warm colour fields
(amber, burnt orange, wine, cream highlight), a gentle vignette and fine
monochrome grain from a fixed-seed LCG. Synthetic; no photograph is used.

Usage: python3 -I scripts/gen_showcase_backdrop.py <out.png>
"""
import math
import struct
import sys
import zlib

W, H = 1600, 1400
BASE = (26, 15, 10)
# (cx, cy, radius) in units of width, colour, peak weight
FIELDS = [
    (0.74, 0.30, 0.42, (242, 165, 65), 0.95),
    (0.22, 0.52, 0.38, (196, 82, 30), 0.85),
    (0.58, 0.86, 0.36, (122, 30, 30), 0.80),
    (0.86, 0.16, 0.16, (255, 226, 176), 0.70),
    (0.08, 0.10, 0.22, (90, 40, 20), 0.60),
]


def lcg(seed):
    x = seed
    while True:
        x = (1103515245 * x + 12345) & 0x7FFFFFFF
        yield x


def main(out):
    rnd = lcg(20261010)
    fields = [(cx * W, cy * W, r * W, c, s) for cx, cy, r, c, s in FIELDS]
    raw = bytearray()
    for y in range(H):
        raw.append(0)
        for x in range(W):
            r, g, b = BASE
            for cx, cy, rad, (fr, fg, fb), s in fields:
                d2 = ((x - cx) ** 2 + (y - cy) ** 2) / (rad * rad)
                if d2 < 9.0:
                    w = s * math.exp(-2.2 * d2)
                    r += (fr - r) * w
                    g += (fg - g) * w
                    b += (fb - b) * w
            vx, vy = (x / W - 0.5) * 1.6, (y / H - 0.5) * 1.6
            v = max(0.0, 1.0 - 0.35 * (vx * vx + vy * vy))
            n = (next(rnd) % 13) - 6
            raw += bytes(max(0, min(255, int(c * v + n))) for c in (r, g, b))
    png = b"\x89PNG\r\n\x1a\n"
    png += chunk(b"IHDR", struct.pack(">IIBBBBB", W, H, 8, 2, 0, 0, 0))
    png += chunk(b"IDAT", zlib.compress(bytes(raw), 9))
    png += chunk(b"IEND", b"")
    with open(out, "wb") as f:
        f.write(png)


def chunk(tag, data):
    return struct.pack(">I", len(data)) + tag + data + struct.pack(">I", zlib.crc32(tag + data) & 0xFFFFFFFF)


if __name__ == "__main__":
    main(sys.argv[1])
