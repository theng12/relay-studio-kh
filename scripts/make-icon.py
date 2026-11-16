#!/usr/bin/env python3
"""Relay's flat three-to-one connection mark; stdlib PNG/ICNS, no native build.

Authored geometry uses the incumbent #235cc2 accent and white. Generated assets
stay ignored; this source is the versioned artwork and regeneration recipe.
"""
import hashlib
import math
from pathlib import Path
import struct
import subprocess
import sys
import tempfile
import zlib

BLUE = (35, 92, 194)
# Three register rows join one spine and the outgoing middle connection.
SEGMENTS = ((.26, .30, .66, .30), (.26, .50, .78, .50),
            (.26, .70, .66, .70), (.66, .30, .66, .70))
SIZES = ((16, '16x16.png'), (32, '32x32.png'), (64, '64x64.png'),
         (128, '128x128.png'), (256, '128x128@2x.png'),
         (512, '512x512.png'), (1024, 'icon.png'))
# iconutil expects the standard iconset names, not the historical icp4/icp5 tags.
ICONSET = (('16x16.png', 'icon_16x16.png'),
           ('32x32.png', 'icon_16x16@2x.png'),
           ('32x32.png', 'icon_32x32.png'),
           ('64x64.png', 'icon_32x32@2x.png'),
           ('128x128.png', 'icon_128x128.png'),
           ('128x128@2x.png', 'icon_128x128@2x.png'),
           ('128x128@2x.png', 'icon_256x256.png'),
           ('512x512.png', 'icon_256x256@2x.png'),
           ('512x512.png', 'icon_512x512.png'),
           ('icon.png', 'icon_512x512@2x.png'))


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


def render(size):
    raw = b''.join(b'\0' + b''.join(pixel((x + .5) / size, (y + .5) / size, size)
                                  for x in range(size)) for y in range(size))
    return (b'\x89PNG\r\n\x1a\n'
            + chunk(b'IHDR', struct.pack('>IIBBBBB', size, size, 8, 6, 0, 0, 0))
            + chunk(b'IDAT', zlib.compress(raw, 9)) + chunk(b'IEND', b''))


def average_rgba(png):
    """Decode one 8-bit RGBA PNG and return the average visible colour."""
    pos, width, height, idat = 8, 0, 0, b''
    while pos < len(png):
        length = struct.unpack('>I', png[pos:pos + 4])[0]
        kind = png[pos + 4:pos + 8]
        data = png[pos + 8:pos + 8 + length]
        if kind == b'IHDR':
            width, height, depth, color, _, _, interlace = struct.unpack('>IIBBBBB', data)
            assert (depth, color, interlace) == (8, 6, 0), 'RGBA8 PNG required'
        elif kind == b'IDAT':
            idat += data
        pos += 12 + length
    raw = zlib.decompress(idat)
    stride, prev, offset, totals = width * 4, bytearray(width * 4), 0, [0, 0, 0, 0]
    for _ in range(height):
        filt = raw[offset]
        offset += 1
        cur = bytearray(raw[offset:offset + stride])
        offset += stride
        for i in range(stride):
            left = cur[i - 4] if i >= 4 else 0
            up = prev[i]
            corner = prev[i - 4] if i >= 4 else 0
            if filt == 1:
                cur[i] = (cur[i] + left) & 255
            elif filt == 2:
                cur[i] = (cur[i] + up) & 255
            elif filt == 3:
                cur[i] = (cur[i] + (left + up) // 2) & 255
            elif filt == 4:
                guess = left + up - corner
                pa, pb, pc = abs(guess - left), abs(guess - up), abs(guess - corner)
                cur[i] = (cur[i] + (left if pa <= pb and pa <= pc else up if pb <= pc else corner)) & 255
        for i in range(0, stride, 4):
            if cur[i + 3]:
                totals[0] += cur[i]
                totals[1] += cur[i + 1]
                totals[2] += cur[i + 2]
                totals[3] += 1
        prev = cur
    assert totals[3], 'icon has no visible pixels'
    return tuple(round(total / totals[3]) for total in totals[:3])


def main():
    if len(sys.argv) != 2:
        raise SystemExit('usage: make-icon.py <generated-icons-directory>')
    output = Path(sys.argv[1])
    output.mkdir(parents=True, exist_ok=True)
    source_hash = hashlib.sha256(Path(__file__).read_bytes()).hexdigest()
    provenance = f'Authored Relay connection mark; scripts/make-icon.py SHA256 {source_hash}'
    for size, name in SIZES:
        png = render(size)
        assert struct.unpack('>II', png[16:24]) == (size, size)
        assert png[24:26] == bytes((8, 6)), 'RGBA PNG required'
        (output / name).write_bytes(png)
        assert (output / name).read_bytes() == png
    (output / 'icon.provenance.txt').write_text(provenance + '\n', encoding='ascii')
    # Scope checks: true alpha, blue face, white mark, and a correctly packed ICNS.
    assert pixel(0, 0, 1024)[3] == 0
    assert pixel(.12, .5, 1024) == bytes((*BLUE, 255))
    for x, y in ((.3, .3), (.3, .5), (.3, .7), (.66, .4), (.75, .5)):
        assert pixel(x, y, 1024) == bytes((255, 255, 255, 255))
    with tempfile.TemporaryDirectory() as tmp:
        iconset = Path(tmp) / 'Relay.iconset'
        iconset.mkdir()
        for source, name in ICONSET:
            (iconset / name).write_bytes((output / source).read_bytes())
        icns = output / 'icon.icns'
        subprocess.run(['iconutil', '-c', 'icns', '-o', str(icns), str(iconset)],
                       check=True, capture_output=True)
        roundtrip = Path(tmp) / 'roundtrip.iconset'
        subprocess.run(['iconutil', '-c', 'iconset', '-o', str(roundtrip), str(icns)],
                       check=True, capture_output=True)
        tiny = {}
        for name in ('icon_16x16.png', 'icon_32x32.png'):
            colour = average_rgba((roundtrip / name).read_bytes())
            tiny[name] = colour
            assert colour[2] > colour[0] + 40 and colour[2] > 120, \
                f'{name} is not blue after ICNS round-trip: {colour}'
    assert icns.read_bytes()[:4] == b'icns'
    print(f'Prepared seven RGBA PNG sizes and iconutil ICNS; 16x16 {tiny["icon_16x16.png"]}, 32x32 {tiny["icon_32x32.png"]}; geometry/alpha/blue-tiny checks passed.')


if __name__ == '__main__':
    main()
