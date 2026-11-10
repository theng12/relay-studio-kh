#!/usr/bin/env python3
"""Relay's flat three-to-one connection mark; stdlib PNG/ICNS, no native build.

Authored geometry uses the incumbent #235cc2 accent and white. Generated assets
stay ignored; this source is the versioned artwork and regeneration recipe.
"""
import hashlib
import math
from pathlib import Path
import struct
import sys
import zlib

BLUE = (35, 92, 194)
# Three register rows join one spine and the outgoing middle connection.
SEGMENTS = ((.26, .30, .66, .30), (.26, .50, .78, .50),
            (.26, .70, .66, .70), (.66, .30, .66, .70))
SIZES = ((16, 'icp4', '16x16.png'), (32, 'icp5', '32x32.png'),
         (64, 'icp6', '64x64.png'), (128, 'ic07', '128x128.png'),
         (256, 'ic08', '128x128@2x.png'), (512, 'ic09', '512x512.png'),
         (1024, 'ic10', 'icon.png'))


def chunk(tag, data):
    return (struct.pack('>I', len(data)) + tag + data
            + struct.pack('>I', zlib.crc32(tag + data) & 0xffffffff))


def pixel(x, y, size):
    # Signed distances give one-pixel antialiasing and true transparent corners.
    qx, qy = abs(x - .5) - .23, abs(y - .5) - .23
    face = math.hypot(max(qx, 0), max(qy, 0)) + min(max(qx, qy), 0) - .19
    alpha = round(255 * max(0, min(1, .5 - face * size)))
    ink_distance = min(math.hypot(x - min(max(x, x1), x2),
                                 y - min(max(y, y1), y2))
                       for x1, y1, x2, y2 in SEGMENTS) - .0375
    ink = max(0, min(1, .5 - ink_distance * size))
    return bytes((*[round(c + (255 - c) * ink) for c in BLUE], alpha))


def render(size, provenance):
    raw = b''.join(b'\0' + b''.join(pixel((x + .5) / size, (y + .5) / size, size)
                                  for x in range(size)) for y in range(size))
    return (b'\x89PNG\r\n\x1a\n'
            + chunk(b'IHDR', struct.pack('>IIBBBBB', size, size, 8, 6, 0, 0, 0))
            + chunk(b'tEXt', b'Source\0' + provenance.encode('ascii'))
            + chunk(b'IDAT', zlib.compress(raw, 9)) + chunk(b'IEND', b''))


def main():
    if len(sys.argv) != 2:
        raise SystemExit('usage: make-icon.py <generated-icons-directory>')
    output = Path(sys.argv[1])
    output.mkdir(parents=True, exist_ok=True)
    source_hash = hashlib.sha256(Path(__file__).read_bytes()).hexdigest()
    provenance = f'Authored Relay connection mark; scripts/make-icon.py SHA256 {source_hash}'
    blocks = []
    for size, tag, name in SIZES:
        png = render(size, provenance)
        assert struct.unpack('>II', png[16:24]) == (size, size)
        assert png[24:26] == bytes((8, 6)), 'RGBA PNG required'
        (output / name).write_bytes(png)
        assert (output / name).read_bytes() == png
        blocks.append(tag.encode('ascii') + struct.pack('>I', len(png) + 8) + png)
    body = b''.join(blocks)
    icns = b'icns' + struct.pack('>I', len(body) + 8) + body
    (output / 'icon.icns').write_bytes(icns)
    # Scope checks: true alpha, blue face, white mark, all ICNS representations.
    assert pixel(0, 0, 1024)[3] == 0
    assert pixel(.12, .5, 1024) == bytes((*BLUE, 255))
    for x, y in ((.3, .3), (.3, .5), (.3, .7), (.66, .4), (.75, .5)):
        assert pixel(x, y, 1024) == bytes((255, 255, 255, 255))
    data = (output / 'icon.icns').read_bytes()
    assert data[:4] == b'icns' and struct.unpack('>I', data[4:8])[0] == len(data)
    offset = 8
    for size, tag, name in SIZES:
        length = struct.unpack('>I', data[offset + 4:offset + 8])[0]
        assert data[offset:offset + 4] == tag.encode('ascii')
        assert data[offset + 8:offset + length] == (output / name).read_bytes()
        offset += length
    assert offset == len(data)
    print('Prepared seven RGBA PNG sizes and ICNS; geometry/alpha/container checks passed.')


if __name__ == '__main__':
    main()
