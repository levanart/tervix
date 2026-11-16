"""Render the original SVG's deliberately small geometry subset with Python 3.10+.

No packages, fonts, platform image tools, timestamps, or external resources.
Run from any directory; --check verifies committed outputs without writing.
"""

import argparse
from pathlib import Path
import struct
import xml.etree.ElementTree as ET
import zlib


ROOT = Path(__file__).resolve().parent
SIZES = (16, 24, 32, 48, 64, 128, 256)
SAMPLES = 4
NS = "{http://www.w3.org/2000/svg}"


def rgb(value):
    return tuple(int(value[i:i + 2], 16) for i in (1, 3, 5))


def load_shapes():
    """Read SVG as the sole source of geometry and palette, failing on new shapes."""
    root = ET.parse(ROOT / "tervix.svg").getroot()
    gradients = {}
    for gradient in root.findall(f"{NS}defs/{NS}linearGradient"):
        stops = gradient.findall(f"{NS}stop")
        if len(stops) != 2 or [s.attrib["offset"] for s in stops] != ["0", "1"]:
            raise ValueError("Renderer requires two endpoint gradient stops")
        gradients[gradient.attrib["id"]] = (
            *(float(gradient.attrib[k]) for k in ("x1", "y1", "x2", "y2")),
            *(rgb(s.attrib["stop-color"]) for s in stops),
        )
    shapes = []
    for node in root:
        tag = node.tag.removeprefix(NS)
        if tag in ("title", "desc", "defs"):
            continue
        fill = node.attrib["fill"]
        paint = gradients[fill[5:-1]] if fill.startswith("url(#") else rgb(fill)
        if tag == "rect":
            geometry = tuple(float(node.attrib[k]) for k in ("x", "y", "width", "height", "rx"))
        elif tag == "polygon":
            geometry = tuple(tuple(map(float, p.split(","))) for p in node.attrib["points"].split())
        else:
            raise ValueError(f"Unsupported SVG shape: {tag}")
        shapes.append((tag, geometry, paint))
    return shapes


def contains(tag, geometry, x, y):
    if tag == "rect":
        left, top, width, height, radius = geometry
        if not (left <= x <= left + width and top <= y <= top + height):
            return False
        dx = max(left + radius - x, 0, x - (left + width - radius))
        dy = max(top + radius - y, 0, y - (top + height - radius))
        return dx * dx + dy * dy <= radius * radius
    inside = False
    previous = geometry[-1]
    for current in geometry:
        ax, ay = previous
        bx, by = current
        if (ay > y) != (by > y) and x < (bx - ax) * (y - ay) / (by - ay) + ax:
            inside = not inside
        previous = current
    return inside


def color_at(paint, x, y):
    if len(paint) == 3:
        return paint
    x1, y1, x2, y2, first, last = paint
    t = max(0, min(1, ((x - x1) * (x2 - x1) + (y - y1) * (y2 - y1))
                   / ((x2 - x1) ** 2 + (y2 - y1) ** 2)))
    return tuple(round(a + (b - a) * t) for a, b in zip(first, last))


def render(size, shapes):
    """4x4 supersampling, averaging premultiplied samples to avoid dark halos."""
    pixels = bytearray()
    scale = 256 / (size * SAMPLES)
    for y in range(size):
        for x in range(size):
            total = [0, 0, 0]
            covered = 0
            for sy in range(SAMPLES):
                py = (y * SAMPLES + sy + 0.5) * scale
                for sx in range(SAMPLES):
                    px = (x * SAMPLES + sx + 0.5) * scale
                    for tag, geometry, paint in reversed(shapes):
                        if contains(tag, geometry, px, py):
                            color = color_at(paint, px, py)
                            for channel in range(3):
                                total[channel] += color[channel]
                            covered += 1
                            break
            if covered:
                pixels.extend(round(c / covered) for c in total)
                pixels.append(round(255 * covered / (SAMPLES * SAMPLES)))
            else:
                pixels.extend((0, 0, 0, 0))
    return bytes(pixels)


def chunk(kind, payload):
    return (struct.pack(">I", len(payload)) + kind + payload
            + struct.pack(">I", zlib.crc32(kind + payload)))


def png(size, rgba):
    rows = b"".join(b"\0" + rgba[y * size * 4:(y + 1) * size * 4] for y in range(size))
    return (b"\x89PNG\r\n\x1a\n"
            + chunk(b"IHDR", struct.pack(">IIBBBBB", size, size, 8, 6, 0, 0, 0))
            + chunk(b"IDAT", zlib.compress(rows, 9)) + chunk(b"IEND", b""))


def verify_png(blob, size):
    """Decode every chunk, checksum, scanline and alpha channel of generated PNGs."""
    assert blob[:8] == b"\x89PNG\r\n\x1a\n"
    offset, compressed = 8, bytearray()
    while offset < len(blob):
        length = struct.unpack_from(">I", blob, offset)[0]
        kind = blob[offset + 4:offset + 8]
        payload = blob[offset + 8:offset + 8 + length]
        assert zlib.crc32(kind + payload) == struct.unpack_from(">I", blob, offset + 8 + length)[0]
        if kind == b"IHDR":
            assert struct.unpack(">IIBBBBB", payload) == (size, size, 8, 6, 0, 0, 0)
        if kind == b"IDAT":
            compressed.extend(payload)
        offset += 12 + length
    assert offset == len(blob) and kind == b"IEND"
    rows = zlib.decompress(compressed)
    stride = size * 4 + 1
    assert len(rows) == size * stride
    assert all(rows[y * stride] == 0 for y in range(size))
    alpha = [rows[y * stride + 4 + x * 4] for y in range(size) for x in range(size)]
    assert min(alpha) == 0 and max(alpha) == 255
    assert any(0 < a < 255 for a in alpha)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--check", action="store_true", help="compare outputs without writing")
    args = parser.parse_args()
    shapes = load_shapes()
    images = {}
    for size in SIZES:
        images[size] = png(size, render(size, shapes))
        verify_png(images[size], size)
    directory = bytearray(struct.pack("<HHH", 0, 1, len(SIZES)))
    offset = 6 + 16 * len(SIZES)
    for size, image in images.items():
        directory.extend(struct.pack("<BBBBHHII", size % 256, size % 256, 0, 0, 1, 32, len(image), offset))
        offset += len(image)
    ico = bytes(directory) + b"".join(images.values())
    for index, size in enumerate(SIZES):
        width, height, colors, reserved, planes, bits, length, start = struct.unpack_from("<BBBBHHII", ico, 6 + index * 16)
        assert (width or 256, height or 256, colors, reserved, planes, bits) == (size, size, 0, 0, 1, 32)
        verify_png(ico[start:start + length], size)
    assert start + length == len(ico)
    outputs = {f"tervix-{size}.png": image for size, image in images.items()}
    outputs["tervix-256.rgba"] = render(256, shapes)
    outputs["tervix.ico"] = ico
    for name, content in outputs.items():
        path = ROOT / name
        if args.check:
            if not path.exists() or path.read_bytes() != content:
                raise SystemExit(f"Outdated asset: {path}")
        else:
            path.write_bytes(content)
        print(f"{'Verified' if args.check else 'Generated'} {name}: {len(content)} bytes")


if __name__ == "__main__":
    main()
