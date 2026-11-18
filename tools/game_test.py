#!/usr/bin/env python3
"""Checks Wayfarer Stones in the game: a stone a player places registers and grows its upper block; a right
click activates it and opens the screen; a command renames a stone and travels to it; a piston doesn't move a
stone; breaking it takes both blocks and the registry entry; the scroll opens the screen from anywhere.

    python tools/game_test.py GAME_FOLDER CTL

GAME_FOLDER is a test instance's folder with the mod built in; CTL its version's horizon-ctl.exe.
"""
import json
import os
import shutil
import subprocess
import sys
import time

GAME, CTL = sys.argv[1:3]
os.environ["HORIZON_HOME"] = GAME
os.environ["MSYS_NO_PATHCONV"] = "1"
sys.stdout.reconfigure(encoding="utf-8")
WORLD = "wayfarer-port"
DIR = os.path.join(GAME, "saves", WORLD, "horizon-wayfarer-stones")
STONE, TOP, SCROLL = "wayfarer_stones:wayfarer_stone", "wayfarer_stones:wayfarer_stone_top", "wayfarer_stones:wayfarers_scroll"
failed = []


def ctl(*args, timeout=300):
    out = subprocess.run([CTL, *map(str, args)], capture_output=True, text=True, encoding="utf-8", timeout=timeout)
    try:
        return json.loads(out.stdout)
    except ValueError:
        return {"raw": (out.stdout + out.stderr)[:400]}


def chat(line):
    return " ".join(map(str, ctl("command", line).get("chat", [])))


def check(name, ok, detail=""):
    print(("ok  " if ok else "FAIL"), name, flush=True)
    if not ok:
        failed.append(f"{name}: {detail}")


def block(x, y, z):
    return json.dumps(ctl("block", x, y, z))


def stones():
    try:
        with open(os.path.join(DIR, "stones.json"), encoding="utf-8") as f:
            return json.load(f).get("stones", [])
    except (OSError, ValueError):
        return []


def messages(n=12):
    return json.dumps(ctl("messages", n), ensure_ascii=False)


def position():
    player = ctl("state").get("player") or {}
    return player.get("pos") or player.get("position")


shutil.rmtree(os.path.join(GAME, "saves", WORLD), ignore_errors=True)
print(" launch:", str(ctl("launch", "--world", WORLD, "--seed", "12345", "--creative", "--wait", "--offscreen", timeout=280))[:80])
ctl("wait-ready", timeout=280)
pos = None
for _ in range(60):
    pos = position()
    if pos:
        break
    ctl("wait", 10)
cx, cz = int(pos[0] // 1), int(pos[2] // 1)
y = 200
chat("gamerule spawn_mobs false")
chat(f"fill {cx - 4} {y - 1} {cz - 4} {cx + 44} {y - 1} {cz + 4} minecraft:stone")
chat(f"tp @s {cx + 0.5} {y} {cz + 0.5}")
chat(f"give @s {STONE} 4")
chat(f"give @s {SCROLL} 2")
chat("xp add @s 20 levels")
ctl("wait", 20)

# A stone placed by the player registers itself and grows its upper block at the tick's end.
a = (cx + 2, y, cz)
ctl("select", STONE)
print(" place:", str(ctl("place", *a))[:120])
ctl("wait", 30)
check("a placed stone stands", STONE in block(*a), block(*a))
check("its upper block grows", TOP in block(a[0], y + 1, a[2]), block(a[0], y + 1, a[2]))
said = messages()
check("the owner hears it was placed", "placed" in said or "установлен" in said, said[-300:])
ctl("wait", 30)
check("the registry has it", any(s.get("pos") == list(a) for s in stones()), json.dumps(stones())[:300])

# A right click on it activates it and opens the mod's screen.
ctl("look-at", "block", *a)
print(" interact:", str(ctl("interact", *a))[:120])
ctl("wait", 20)
screen = json.dumps(ctl("screen"))
check("a right click opens the screen", '"mod"' in screen or "wayfarer" in screen, screen[:300])
ctl("key", "esc")
ctl("wait", 10)

# A second stone, renamed by command, and a travel to it.
b = (cx + 40, y, cz)
chat(f"tp @s {b[0] - 2 + 0.5} {y} {cz + 0.5}")
ctl("wait", 20)
ctl("select", STONE)
ctl("place", *b)
ctl("wait", 30)
said = chat("wayfarer name Far")
ctl("wait", 30)
check("a stone is renamed by command", any(s.get("name") == "Far" for s in stones()), said + json.dumps(stones())[:300])
chat(f"tp @s {a[0] - 2 + 0.5} {y} {cz + 0.5}")
ctl("wait", 20)
said = chat("wayfarer to Far")
ctl("wait", 60)
p = position() or [0, 0, 0]
check("a travel takes the player to the stone", abs(p[0] - b[0]) < 4 and abs(p[2] - b[2]) < 4, f"{said} at {p}")

# A piston doesn't move a stone.
chat(f"setblock {b[0] - 1} {y} {b[2]} minecraft:piston[facing=east]")
chat(f"setblock {b[0] - 2} {y} {b[2]} minecraft:redstone_block")
ctl("wait", 20)
check("a piston doesn't move a stone", STONE in block(*b) and TOP in block(b[0], y + 1, b[2]), block(*b))
chat(f"fill {b[0] - 2} {y} {b[2]} {b[0] - 1} {y} {b[2]} minecraft:air")

# Breaking the stone takes both blocks and its entry.
chat(f"tp @s {a[0] - 2 + 0.5} {y} {cz + 0.5}")
ctl("wait", 20)
ctl("look-at", "block", *a)
print(" break:", str(ctl("break", *a))[:120])
ctl("wait", 40)
check("a broken stone is gone", STONE not in block(*a), block(*a))
check("its upper block goes with it", TOP not in block(a[0], y + 1, a[2]), block(a[0], y + 1, a[2]))
check("its entry goes", not any(s.get("pos") == list(a) for s in stones()), json.dumps(stones())[:300])

# The scroll opens the screen from anywhere.
ctl("select", SCROLL)
ctl("look", 0, -90)
ctl("use-item")
ctl("wait", 20)
screen = json.dumps(ctl("screen"))
check("the scroll opens the screen", '"mod"' in screen or "wayfarer" in screen, screen[:300])
ctl("key", "esc")

ctl("quit")
time.sleep(6)
print(f"== failed: {failed}" if failed else "== all passed", flush=True)
