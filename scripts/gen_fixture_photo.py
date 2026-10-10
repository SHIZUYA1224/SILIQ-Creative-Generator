#!/usr/bin/env python3
"""Generate the synthetic "photo" fixture deterministically (stdlib only).

1600x1000 RGBA: diagonal colour gradient, 100 px grid, an off-centre disc
(the "subject", centred at 65% / 40%) and a 24 px transparent fade at the
edges to exercise alpha. No real photograph or customer material is used.

Usage: python3 -I scripts/gen_fixture_photo.py <out.png>
"""
import struct
import sys
import zlib

W, H = 1600, 1000
CX, CY, R = int(W * 0.65), int(H * 0.40), 180
FADE = 24


def pixel(x, y):
    r = 40 + (180 * x) // (W - 1)
    g = 90 + (120 * y) // (H - 1)
    b = 200 - (120 * (x + y)) // (W + H - 2)
    if x % 100 == 0 or y % 100 == 0:
        r, g, b = r // 2, g // 2, b // 2
    dx, dy = x - CX, y - CY
    if dx * dx + dy * dy <= R * R:
        r, g, b = 245, 200, 60
    edge = min(x, y, W - 1 - x, H - 1 - y)
    a = 255 if edge >= FADE else (255 * edge) // FADE
    return bytes((r, g, b, a))


def chunk(tag, data):
    return struct.pack(">I", len(data)) + tag + data + struct.pack(">I", zlib.crc32(tag + data) & 0xFFFFFFFF)


def main(out):
    raw = bytearray()
    for y in range(H):
        raw.append(0)  # filter: none
        for x in range(W):
            raw += pixel(x, y)
    png = b"\x89PNG\r\n\x1a\n"
    png += chunk(b"IHDR", struct.pack(">IIBBBBB", W, H, 8, 6, 0, 0, 0))
    png += chunk(b"IDAT", zlib.compress(bytes(raw), 9))
    png += chunk(b"IEND", b"")
    with open(out, "wb") as f:
        f.write(png)


if __name__ == "__main__":
    main(sys.argv[1])
