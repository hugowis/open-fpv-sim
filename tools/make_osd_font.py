#!/usr/bin/env python3
"""Converts a Betaflight OSD font (.mcm, MAX7456 format) into a glyph atlas PNG for the Godot client.

An .mcm file is the text line `MAX7456` followed by 256 glyphs x 64 lines of 8 binary digits. Each glyph is 12 x 18
pixels, 2 bits per pixel, 4 pixels per byte starting at the most significant bits, row by row (54 of the 64 bytes
are used): 00 black, 01 transparent, 10 white, 11 transparent. The atlas is 16 x 16 glyphs (192 x 288 px, RGBA);
glyph `code` sits at column `code % 16`, row `code // 16`.

Usage: python tools/make_osd_font.py <default.mcm> godot/ui/osd_font.png
"""
import struct
import sys
import zlib

GLYPH_W, GLYPH_H, GLYPHS, BYTES_PER_GLYPH, COLS = 12, 18, 256, 64, 16
PIXELS = {0b00: (0, 0, 0, 255), 0b01: (0, 0, 0, 0), 0b10: (255, 255, 255, 255), 0b11: (0, 0, 0, 0)}


def read_mcm(path):
    with open(path, encoding="ascii") as f:
        lines = [line.strip() for line in f if line.strip()]
    if lines[0] != "MAX7456":
        raise SystemExit(f"{path}: not a MAX7456 font (first line {lines[0]!r})")
    bits = lines[1:]
    if len(bits) != GLYPHS * BYTES_PER_GLYPH:
        raise SystemExit(f"{path}: expected {GLYPHS * BYTES_PER_GLYPH} data lines, found {len(bits)}")
    return [bytes(int(b, 2) for b in bits[g * BYTES_PER_GLYPH:(g + 1) * BYTES_PER_GLYPH]) for g in range(GLYPHS)]


def glyph_pixels(data):
    out = []
    for i in range(GLYPH_W * GLYPH_H):
        byte = data[i // 4]
        out.append(PIXELS[(byte >> (6 - 2 * (i % 4))) & 3])
    return out


def write_png(path, width, height, rows):
    raw = b"".join(b"\x00" + bytes(channel for pixel in row for channel in pixel) for row in rows)

    def chunk(tag, data):
        body = tag + data
        return struct.pack(">I", len(data)) + body + struct.pack(">I", zlib.crc32(body) & 0xFFFFFFFF)

    png = b"\x89PNG\r\n\x1a\n"
    png += chunk(b"IHDR", struct.pack(">IIBBBBB", width, height, 8, 6, 0, 0, 0))
    png += chunk(b"IDAT", zlib.compress(raw, 9))
    png += chunk(b"IEND", b"")
    with open(path, "wb") as f:
        f.write(png)


def main():
    if len(sys.argv) != 3:
        raise SystemExit(__doc__)
    glyphs = [glyph_pixels(g) for g in read_mcm(sys.argv[1])]
    width, height = COLS * GLYPH_W, (GLYPHS // COLS) * GLYPH_H
    rows = []
    for y in range(height):
        row = []
        for x in range(width):
            glyph = glyphs[(y // GLYPH_H) * COLS + x // GLYPH_W]
            row.append(glyph[(y % GLYPH_H) * GLYPH_W + x % GLYPH_W])
        rows.append(row)
    write_png(sys.argv[2], width, height, rows)
    print(f"wrote {sys.argv[2]} ({width}x{height})")


if __name__ == "__main__":
    main()
