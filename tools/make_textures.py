#!/usr/bin/env python3
"""Draws the mod's textures from the maps below: original art, standard library only.

    python tools/make_textures.py [PREVIEW.png]

Writes content/assets/wayfarer_stones/textures/{block,item,gui}/*.png, and with an argument a magnified preview.
"""
import math
import struct
import sys
import zlib
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1] / "content" / "assets" / "wayfarer_stones" / "textures"

STONE = {
    "h": (150, 156, 172, 255),  # a lit edge
    "l": (116, 122, 139, 255),  # light stone
    "s": (84, 89, 105, 255),  # stone
    "m": (66, 70, 85, 255),  # mortar shade
    "d": (43, 46, 59, 255),  # dark joint
}
GLOW = {
    "t": (24, 122, 134, 255),  # the rune's faint edge
    "c": (44, 196, 196, 255),  # the rune
    "b": (150, 246, 232, 255),  # its bright core
}
PAPER = {
    ".": (0, 0, 0, 0),
    "e": (86, 62, 40, 255),  # outline
    "p": (226, 205, 158, 255),  # parchment
    "q": (176, 146, 98, 255),  # the roll's shade
    **GLOW,
}

# The sides of a pillar two blocks tall, with a rune running up the two of them.
# The models take a texture's rows from the top: the lower block's shaft is its
# first 13 rows and its plinth the last 3; the upper's cap its first 4, its shaft the rest.
SHAFT_LEFT = ["ssd", "msd", "ssd", "smd", "ssd", "ssd", "msd", "ssd", "smd", "ssd", "ssd", "msd", "ssd"]
SHAFT_RIGHT = ["dss", "dsm", "dss", "dms", "dss", "dss", "dsm", "dss", "dms", "dss", "dss", "dsm", "dss"]
RUNE_LOWER = [
    "sssbccbsss",
    "sstbssbtss",
    "sssbssbsss",
    "ssssbbssss",
    "sstbbbbtss",
    "ssssbbssss",
    "sssbccbsss",
    "sstbssbtss",
    "sssbbbbsss",
    "ssssssssss",
    "sssstsssss",
    "sssssstsss",
    "ssssssssss",
]
LOWER = [a + b + c for a, b, c in zip(SHAFT_LEFT, RUNE_LOWER, SHAFT_RIGHT)] + [
    "hhlhhhlhhhhlhhhl",
    "llsllllsllllslls",
    "mmdmmmdmmmmdmmmd",
]
RUNE_UPPER = [
    "ssssssssss",
    "ssssbbssss",
    "sssbccbsss",
    "sstbssbtss",
    "sssbssbsss",
    "ssssbbssss",
    "sssstsssss",
    "ssssssssss",
    "sssssstsss",
    "ssssssssss",
    "ssssbbssss",
    "ssssccssss",
]
UPPER = [
    "hhlhhhlhhhhlhhhl",
    "llsllllsllllslls",
    "ssdsssdssssdsssd",
    "mmdmmmdmmmmdmmmd",
] + [a + b + c for a, b, c in zip(SHAFT_LEFT, RUNE_UPPER, SHAFT_RIGHT)]

SCROLL = [
    "................",
    "...eeeeeeeeee...",
    "..eppppppppppe..",
    "..eqqqqqqqqqqe..",
    "...eppppppppe...",
    "...eppcbbcppe...",
    "...epcppppcpe...",
    "...eppcbbcppe...",
    "...epppccpppe...",
    "...epcppppcpe...",
    "...eppcbbcppe...",
    "...eppppppppe...",
    "..eppppppppppe..",
    "..eqqqqqqqqqqe..",
    "...eeeeeeeeee...",
    "................",
]

# The item: the stone as it stands, on a ground of nothing.
ICON = {".": (0, 0, 0, 0), "o": (24, 26, 34, 255), **STONE, **GLOW}
STONE_ITEM = [
    "....oooooooo....",
    "...ohlhhlhhlo...",
    "...olsslsslso...",
    "...oooooooooo...",
    "....osbbssdo....",
    "....osbccbdo....",
    "....osbssbdo....",
    "....osbbssdo....",
    "....ostbbsdo....",
    "....osbbssdo....",
    "....osbccbdo....",
    "....osbssbdo....",
    "...oooooooooo...",
    "..ohhlhhlhhlho..",
    "..olsslsslsslo..",
    "..oooooooooooo..",
]

PENCIL = {
    ".": (0, 0, 0, 0),
    "o": (24, 26, 34, 255),  # outline
    "w": (230, 196, 120, 255),  # the shaft
    "s": (176, 138, 74, 255),  # its shade
    "m": (150, 156, 172, 255),  # the ferrule
    "r": (214, 96, 96, 255),  # the eraser
    "g": (58, 62, 74, 255),  # the point
}
RENAME = [
    "................",
    "...........oo...",
    "..........orro..",
    ".........orrrro.",
    "........ommrro..",
    ".......owwmmo...",
    "......owwsmo....",
    ".....owwso.o....",
    "....owwso.......",
    "...owwso........",
    "..owwso.........",
    "..owso..........",
    ".ogoo...........",
    ".ogo............",
    ".oo.............",
    "................",
]


def jitter(x, y, salt):
    """A small, repeatable change of brightness, so stone isn't flat."""
    h = (x * 73856093) ^ (y * 19349663) ^ (salt * 83492791)
    return ((h >> 3) % 9) - 4


def shade(color, amount):
    r, g, b, a = color
    return (*(max(0, min(255, c + amount)) for c in (r, g, b)), a)


def from_map(rows, palette, salt):
    assert len(rows) == 16 and all(len(r) == 16 for r in rows), [len(r) for r in rows]
    out = []
    for y, row in enumerate(rows):
        line = []
        for x, ch in enumerate(row):
            color = palette[ch]
            line.append(shade(color, jitter(x, y, salt)) if ch in STONE else color)
        out.append(line)
    return out


def top():
    """A ring of light round a bright centre, in a stone rim."""
    out = []
    for y in range(16):
        line = []
        for x in range(16):
            r = math.hypot(x - 7.5, y - 7.5)
            if x in (0, 15) or y in (0, 15):
                c = STONE["l"]
            elif x in (1, 14) or y in (1, 14):
                c = STONE["d"]
            elif r < 1.6:
                c = GLOW["b"]
            elif r < 2.6:
                c = GLOW["t"]
            elif 4.2 <= r < 5.3:
                # The ring is broken at the diagonals, as carved runes are.
                on = abs(abs(x - 7.5) - abs(y - 7.5)) > 1.0
                c = GLOW["c"] if on else STONE["m"]
            else:
                c = STONE["s"]
            line.append(shade(c, jitter(x, y, 2)) if c in STONE.values() else c)
        out.append(line)
    return out


def bottom():
    out = []
    for y in range(16):
        line = []
        for x in range(16):
            edge = x in (0, 15) or y in (0, 15)
            joint = y in (5, 10) or (x in (5, 10) and y % 10 < 5)
            c = STONE["m"] if edge else STONE["d"] if joint else STONE["s"]
            line.append(shade(c, jitter(x, y, 3) - 8))
        out.append(line)
    return out


def panel():
    """The window's carved edge, 32 square: corners with a rune, edges that stretch."""
    size = 32
    out = []
    for y in range(size):
        line = []
        for x in range(size):
            d = min(x, y, size - 1 - x, size - 1 - y)
            if d == 0:
                c = STONE["d"]
            elif d == 1:
                c = STONE["h"] if x == 1 or y == 1 else STONE["l"]
            elif d < 6:
                c = STONE["s"]
            elif d == 6:
                c = GLOW["t"]
            elif d == 7:
                c = STONE["d"]
            else:
                c = (34, 37, 49, 255)
            # A rune in each corner, the only part that isn't stretched.
            cx = x if x < 8 else size - 1 - x
            cy = y if y < 8 else size - 1 - y
            if (cx, cy) in ((3, 3), (4, 4)) and x % (size - 1) and y % (size - 1):
                c = GLOW["b"]
            elif (cx, cy) in ((3, 4), (4, 3)):
                c = GLOW["c"]
            line.append(c)
        out.append(line)
    return out


def png(pixels):
    height, width = len(pixels), len(pixels[0])
    raw = b"".join(b"\0" + b"".join(bytes(p) for p in row) for row in pixels)

    def chunk(kind, data):
        body = kind + data
        return struct.pack(">I", len(data)) + body + struct.pack(">I", zlib.crc32(body))

    header = struct.pack(">IIBBBBB", width, height, 8, 6, 0, 0, 0)
    return (
        b"\x89PNG\r\n\x1a\n"
        + chunk(b"IHDR", header)
        + chunk(b"IDAT", zlib.compress(raw, 9))
        + chunk(b"IEND", b"")
    )


def preview(images, scale=8, gap=8):
    width = sum(len(i[0]) * scale + gap for i in images) + gap
    height = max(len(i) for i in images) * scale + 2 * gap
    back = (58, 92, 58, 255)
    canvas = [[back for _ in range(width)] for _ in range(height)]
    ox = gap
    for image in images:
        for y in range(len(image) * scale):
            for x in range(len(image[0]) * scale):
                p = image[y // scale][x // scale]
                if p[3]:
                    canvas[gap + y][ox + x] = p
        ox += len(image[0]) * scale + gap
    return canvas


def main():
    textures = {
        "block/wayfarer_stone_lower.png": from_map(LOWER, {**STONE, **GLOW}, 1),
        "block/wayfarer_stone_upper.png": from_map(UPPER, {**STONE, **GLOW}, 6),
        "block/wayfarer_stone_top.png": top(),
        "block/wayfarer_stone_bottom.png": bottom(),
        "item/wayfarers_scroll.png": from_map(SCROLL, PAPER, 4),
        "item/wayfarer_stone.png": from_map(STONE_ITEM, ICON, 7),
        "gui/panel.png": panel(),
        "gui/rename.png": from_map(RENAME, PENCIL, 5),
    }
    for name, pixels in textures.items():
        path = ROOT / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(png(pixels))
        print(path.relative_to(ROOT.parents[2]), path.stat().st_size, "bytes")
    if len(sys.argv) > 1:
        Path(sys.argv[1]).write_bytes(png(preview(list(textures.values()))))


if __name__ == "__main__":
    main()
