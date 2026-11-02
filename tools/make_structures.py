#!/usr/bin/env python3
"""Writes the stone each village biome builds, and the pool files that add it.

    python tools/make_structures.py

A template is the stone on a small floor of the biome's stone, with a jigsaw
at its front that a village street joins, as a house's does. Standard library only.
"""
import gzip
import json
import struct
from pathlib import Path

DATA = Path(__file__).resolve().parents[1] / "content" / "data"
# DataVersion of Minecraft 26.3: a template this new is read as it is.
DATA_VERSION = 5023
# The village, and the block of the floor its stone stands on.
VILLAGES = {
    "plains": "minecraft:cobblestone",
    "desert": "minecraft:cut_sandstone",
    "savanna": "minecraft:smooth_red_sandstone",
    "snowy": "minecraft:polished_diorite",
    "taiga": "minecraft:mossy_cobblestone",
}
BYTE, SHORT, INT, STRING, LIST, COMPOUND = 1, 2, 3, 8, 9, 10


def named(tag, name, payload):
    raw = name.encode()
    return bytes([tag]) + struct.pack(">H", len(raw)) + raw + payload


def string(value):
    raw = value.encode()
    return struct.pack(">H", len(raw)) + raw


def integer(value):
    return struct.pack(">i", value)


def compound(fields):
    """`fields` is {name: int | str | list of int | dict | list of dict}."""
    out = b""
    for name, value in fields.items():
        if isinstance(value, bool):
            raise TypeError(name)
        if isinstance(value, int):
            out += named(INT, name, integer(value))
        elif isinstance(value, str):
            out += named(STRING, name, string(value))
        elif isinstance(value, dict):
            out += named(COMPOUND, name, compound(value))
        else:
            out += named(LIST, name, list_of(value))
    return out + b"\0"


def list_of(items):
    if not items:
        return bytes([0]) + integer(0)
    if isinstance(items[0], int):
        return bytes([INT]) + integer(len(items)) + b"".join(integer(i) for i in items)
    return bytes([COMPOUND]) + integer(len(items)) + b"".join(compound(i) for i in items)


def template(floor):
    """3 wide, 3 deep, 4 tall: the floor, the stone in its middle, air around it."""
    palette = [
        {"Name": floor},
        {"Name": "minecraft:air"},
        {"Name": "wayfarer_stones:wayfarer_stone", "Properties": {"waterlogged": "false"}},
        {"Name": "wayfarer_stones:wayfarer_stone_top", "Properties": {"waterlogged": "false"}},
        {"Name": "minecraft:jigsaw", "Properties": {"orientation": "south_up"}},
    ]
    floor_, air, lower, top, jigsaw = range(5)
    blocks = []
    for y in range(4):
        for z in range(3):
            for x in range(3):
                state = floor_ if y == 0 else air
                if (x, z) == (1, 1) and y in (1, 2):
                    state = lower if y == 1 else top
                block = {"pos": [x, y, z], "state": state}
                if (x, y, z) == (1, 1, 2):
                    # What a street's jigsaw (target `building_entrance`) joins;
                    # it becomes air once it has.
                    block = {
                        "pos": [x, y, z],
                        "state": jigsaw,
                        "nbt": {
                            "id": "minecraft:jigsaw",
                            "final_state": "minecraft:air",
                            "joint": "aligned",
                            "name": "minecraft:building_entrance",
                            "pool": "minecraft:empty",
                            "target": "minecraft:empty",
                            "placement_priority": 0,
                            "selection_priority": 0,
                        },
                    }
                blocks.append(block)
    root = {
        "DataVersion": DATA_VERSION,
        "size": [3, 4, 3],
        "palette": palette,
        "blocks": blocks,
        "entities": [],
    }
    # No clock in the header, so a rebuild is the same bytes.
    return gzip.compress(named(COMPOUND, "", compound(root)), mtime=0)


def pool(village):
    # `guaranteed`: tried before the other houses until one stands, so a village has one.
    return {
        "horizon:add_to": f"minecraft:village/{village}/houses",
        "elements": [
            {
                "weight": 1,
                "element": {
                    "element_type": "lithostitched:guaranteed",
                    "count": 1,
                    "delegate": {
                        "element_type": "minecraft:single_pool_element",
                        "location": f"wayfarer_stones:village/{village}",
                        "projection": "rigid",
                        "processors": "minecraft:empty",
                    },
                },
            }
        ],
    }


def main():
    for village, floor in VILLAGES.items():
        path = DATA / "wayfarer_stones" / "structure" / "village" / f"{village}.nbt"
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(template(floor))
        print(path.relative_to(DATA.parent), path.stat().st_size, "bytes")
        path = DATA / "wayfarer_stones" / "worldgen" / "template_pool" / "village" / f"{village}.json"
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(json.dumps(pool(village), indent=2) + "\n", encoding="utf-8")
    # A pickaxe mines it, and neither boss eats it.
    for name, values in {
        "mineable/pickaxe": ["wayfarer_stones:wayfarer_stone", "wayfarer_stones:wayfarer_stone_top"],
        "wither_immune": ["wayfarer_stones:wayfarer_stone", "wayfarer_stones:wayfarer_stone_top"],
        "dragon_immune": ["wayfarer_stones:wayfarer_stone", "wayfarer_stones:wayfarer_stone_top"],
    }.items():
        path = DATA / "minecraft" / "tags" / "block" / f"{name}.json"
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(json.dumps({"values": values}) + "\n", encoding="utf-8")


if __name__ == "__main__":
    main()
