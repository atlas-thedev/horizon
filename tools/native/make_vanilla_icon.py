#!/usr/bin/env python3
"""Native Client: the Minecraft Java Edition icon for the game window and executable.

Usage: make_vanilla_icon.py <out.ico> <out-64.png>

Downloads the Java Edition window icons that Mojang publishes with Minecraft 26.3 (asset index entries
icons/icon_*.png), checks each against the SHA-1 that is part of its address, and writes a multi-size
Windows .ico plus a 64 x 64 PNG. Needs Pillow (pip install pillow) and network access, like the rest of
the build. Fails when a download does not match its hash, instead of shipping an unknown icon.
"""
import hashlib
import io
import struct
import sys
import urllib.request

from PIL import Image

# Minecraft 26.3 (release) asset index: name -> SHA-1 of the file.
ICONS = {
    16: '5ff04807c356f1beed0b86ccf659b44b9983e3fa',
    32: 'af96f55a90eaf11b327f1b5f8834a051027dc506',
    48: 'b80b6e9ff01c78c624df5429e1d3dcd3d5130834',
    128: 'b62ca8ec10d07e6bf5ac8dae0c8c1d2e6a1e3356',
    256: '8030dd9dc315c0381d52c4782ea36c6baf6e8135',
}


def fetch(sha1):
    url = f'https://resources.download.minecraft.net/{sha1[:2]}/{sha1}'
    data = urllib.request.urlopen(url, timeout=60).read()
    if hashlib.sha1(data).hexdigest() != sha1:
        raise SystemExit(f'make_vanilla_icon: {url} does not match its SHA-1')
    return data


def main():
    if len(sys.argv) != 3:
        raise SystemExit(__doc__)
    images = {}
    for side, sha1 in ICONS.items():
        data = fetch(sha1)
        size = Image.open(io.BytesIO(data)).size
        if size != (side, side):
            raise SystemExit(f'make_vanilla_icon: icon_{side}x{side}.png is {size[0]}x{size[1]}')
        images[side] = data
    out = bytearray(struct.pack('<HHH', 0, 1, len(images)))
    offset = 6 + 16 * len(images)
    for side, data in images.items():
        out += struct.pack('<BBBBHHII', side % 256, side % 256, 0, 0, 1, 32, len(data), offset)
        offset += len(data)
    for data in images.values():
        out += data
    with open(sys.argv[1], 'wb') as f:
        f.write(out)
    Image.open(io.BytesIO(images[128])).convert('RGBA').resize((64, 64), Image.LANCZOS).save(sys.argv[2], optimize=True)
    print(f'make_vanilla_icon: {sys.argv[1]} ({len(images)} sizes) and {sys.argv[2]} (64x64)')


main()
