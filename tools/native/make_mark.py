#!/usr/bin/env python3
"""Native Client: the launch screen's mark from the window icon (see boot_screen.py).

Usage: make_mark.py <icon.png> <out.bin> [<out.bin> ...]

Reads a 64x64 8-bit PNG (the Native icon, horizon-64.png) and writes 64*64 bytes: each pixel's grey
as it looks over pure black (luma x alpha). The launch screens draw it light-on-black from this.
No third-party modules: the build machine's plain Python is enough.
"""
import struct
import sys
import zlib

SIDE = 64


def read_png(data):
    if data[:8] != b'\x89PNG\r\n\x1a\n':
        raise SystemExit('make_mark: not a PNG')
    pos, ihdr, idat = 8, None, []
    while pos < len(data):
        size, kind = struct.unpack('>I4s', data[pos:pos + 8])
        body = data[pos + 8:pos + 8 + size]
        pos += 12 + size
        if kind == b'IHDR':
            ihdr = struct.unpack('>IIBBBBB', body)
        elif kind == b'IDAT':
            idat.append(body)
        elif kind == b'IEND':
            break
    if ihdr is None:
        raise SystemExit('make_mark: no IHDR')
    width, height, depth, ctype, _, _, interlace = ihdr
    channels = {0: 1, 2: 3, 4: 2, 6: 4}.get(ctype)
    if depth != 8 or channels is None or interlace != 0:
        raise SystemExit(f'make_mark: unsupported PNG (depth {depth}, type {ctype}, interlace {interlace})')
    raw = zlib.decompress(b''.join(idat))
    stride = width * channels
    rows, prev = [], bytearray(stride)
    for y in range(height):
        start = y * (stride + 1)
        kind, line = raw[start], bytearray(raw[start + 1:start + 1 + stride])
        for i in range(stride):
            a = line[i - channels] if i >= channels else 0
            b = prev[i]
            c = prev[i - channels] if i >= channels else 0
            if kind == 1:
                line[i] = (line[i] + a) & 255
            elif kind == 2:
                line[i] = (line[i] + b) & 255
            elif kind == 3:
                line[i] = (line[i] + (a + b) // 2) & 255
            elif kind == 4:
                p = a + b - c
                pa, pb, pc = abs(p - a), abs(p - b), abs(p - c)
                pred = a if pa <= pb and pa <= pc else (b if pb <= pc else c)
                line[i] = (line[i] + pred) & 255
            elif kind != 0:
                raise SystemExit(f'make_mark: bad PNG filter {kind}')
        rows.append(line)
        prev = line
    return width, height, channels, rows


def main():
    if len(sys.argv) < 3:
        raise SystemExit(__doc__)
    with open(sys.argv[1], 'rb') as f:
        width, height, channels, rows = read_png(f.read())
    if (width, height) != (SIDE, SIDE):
        raise SystemExit(f'make_mark: the icon is {width}x{height}, expected {SIDE}x{SIDE}')
    out = bytearray()
    for line in rows:
        for x in range(width):
            px = line[x * channels:(x + 1) * channels]
            if channels == 1:
                luma, alpha = px[0], 255
            elif channels == 2:
                luma, alpha = px[0], px[1]
            else:
                luma = (px[0] * 299 + px[1] * 587 + px[2] * 114) // 1000
                alpha = px[3] if channels == 4 else 255
            out.append((luma * alpha + 127) // 255)
    for path in sys.argv[2:]:
        with open(path, 'wb') as f:
            f.write(out)
    print(f'make_mark: {len(out)} bytes -> {len(sys.argv) - 2} file(s)')


main()
