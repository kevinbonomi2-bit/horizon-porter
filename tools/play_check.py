#!/usr/bin/env python3
"""Plays a ported mod in the real game and says what doesn't work there: the
checks only a running Horizon can make. Through horizon-ctl (Horizon's control
socket) it opens a creative test world in the instance the mod is installed in,
and for each of the mod's blocks:

  1. places it (`setblock`) on a stone pad and reads the state back: a block the
     game refuses, or that comes back as another, is reported;
  2. for a block whose Java acts on a right click (runtime.rs's `*_USE` lists),
     walks to it, right-clicks it and reads it again: a click that changes
     nothing is listed (some need an item in hand, so it is a note, not a fault);
  3. checks the game is still running after each step, and reads its log for
     panics at the end.

    python tools/play_check.py <port folder> [--instance demo-port] [--limit N] [--keep]

The mod must be installed (and built) in the instance first:
`horizon-porter install <port> --instance <name> --build --trust-native-code`.
It plays in a world named porter-check, which it creates or reopens; the
player's own worlds aren't touched. --keep leaves the game running.

A note is not a fault: "a right click changed nothing" can be the block (it
needs an item, or its code waits for a neighbour) or this script (the pad is far
from where the player stands and the click met another block). Look at the
block in the game before calling it broken.
"""

import argparse
import json
import os
import re
import subprocess
import sys
import time
from pathlib import Path

APPDATA = Path(os.environ.get("APPDATA", Path.home() / ".local" / "share"))
LAUNCHER = APPDATA / "horizon-launcher"
INSTANCES = APPDATA / "horizon" / "instances"


def ctl_path() -> Path:
    """The newest native build's horizon-ctl (the one `install --build` just made)."""
    found = sorted(LAUNCHER.glob("versions/*/horizon-ctl*"), key=lambda p: p.parent.stat().st_mtime)
    found = [p for p in found if p.suffix in ("", ".exe")]
    if not found:
        sys.exit("no horizon-ctl: build an instance with the launcher first")
    return found[-1]


class Game:
    def __init__(self, ctl: Path, home: Path):
        self.ctl = ctl
        self.env = dict(os.environ, HORIZON_HOME=str(home))

    def run(self, *args: str, timeout: float = 60) -> dict:
        r = subprocess.run(
            [str(self.ctl), "--timeout", str(int(timeout)), *args],
            capture_output=True,
            text=True,
            env=self.env,
            timeout=timeout + 30,
        )
        out = (r.stdout or "").strip().splitlines()
        try:
            return json.loads(out[-1]) if out else {"ok": False, "error": r.stderr.strip()}
        except json.JSONDecodeError:
            return {"ok": False, "error": (r.stdout + r.stderr).strip()[-300:]}

    def state(self, pos) -> str:
        return self.run("block", *map(str, pos)).get("state", "")

    def alive(self) -> bool:
        return "protocol" in self.run("hello", timeout=15)


def mod_blocks(port: Path) -> tuple[list[str], set[str]]:
    """The content mod's blocks, and those whose Java acts on a right click."""
    blocks = sorted(
        f"{f.parent.parent.parent.name}:{f.stem}"
        for f in port.glob("content/data/*/horizon/block/*.json")
    )
    used: set[str] = set()
    for runtime in port.glob("mixins/*/runtime.rs"):
        text = runtime.read_text(encoding="utf-8")
        for body in re.findall(r"const [A-Z0-9_]+_USE: &\[&str\] = &\[(.*?)\];", text, re.S):
            used.update(re.findall(r'"([^"]+)"', body))
    return blocks, used


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("port")
    ap.add_argument("--instance", default="demo-port")
    ap.add_argument("--limit", type=int, default=0, help="at most this many blocks (0: all)")
    ap.add_argument("--keep", action="store_true", help="leave the game running")
    args = ap.parse_args()
    port = Path(args.port)
    home = INSTANCES / args.instance
    if not home.is_dir():
        sys.exit(f"no instance {args.instance} in {INSTANCES}")
    blocks, used = mod_blocks(port)
    if args.limit:
        blocks = blocks[: args.limit]
    game = Game(ctl_path(), home)
    print(f"{len(blocks)} blocks, {len(used & set(blocks))} that act on a click; starting the game", flush=True)
    # A game already answering (another check's, still closing) would take the
    # commands meant for this one: wait until it is gone.
    if game.alive():
        game.run("quit", timeout=60)
    for _ in range(60):
        if not game.alive():
            break
        time.sleep(1)
    launched = game.run("launch", "--world", "porter-check", "--creative", "--wait", timeout=300)
    if "player" not in launched or not game.alive():
        sys.exit(f"the game didn't start: {launched.get('error') or launched}")
    px, py, pz = (int(v) for v in launched["player"]["position"])
    # The world's surface at the player: pads go on it, not in a cave or the sky.
    for dy in range(0, 40):
        if game.state((px, py - 1 - dy, pz)) not in ("", "minecraft:air"):
            py -= dy
            break
    faults: list[str] = []
    notes: list[str] = []
    # A row of pads two blocks apart, beside the player, in rows of 16.
    spots = []
    for i, block in enumerate(blocks):
        x, z = px + 3 + (i % 16) * 2, pz + 3 + (i // 16) * 3
        pos = (x, py, z)
        spots.append((block, pos))
        game.run("setblock", str(x), str(py - 1), str(z), "stone")
        for dy in (0, 1, 2):
            game.run("setblock", str(x), str(py + dy), str(z), "air")
        placed = game.run("setblock", *map(str, pos), block)
        if placed.get("ok") is False:
            faults.append(f"{block}: not placed: {placed.get('error')}")
            continue
        got = game.state(pos)
        if got.split("[")[0] != block:
            time.sleep(0.5)
            got = game.state(pos)
        if got.split("[")[0] != block:
            faults.append(f"{block}: placed, reads back as {got or 'nothing'}")
        elif "half=lower" in got:
            # A door is two blocks: its code reads the other half, as placing one does.
            upper = got.replace("half=lower", "half=upper")
            game.run("setblock", str(x), str(py + 1), str(z), upper)
        if (i + 1) % 25 == 0:
            print(f"  placed {i + 1}", flush=True)
            if not game.alive():
                faults.append(f"the game stopped answering after placing {block}")
                break
    for block, pos in spots:
        if block not in used:
            continue
        x, y, z = pos
        game.run("goto", str(x + 0.5), str(y), str(z + 2.5), "--direct", "--timeout", "15")
        before = game.state(pos)
        clicked = game.run("interact", *map(str, pos), "south")
        if clicked.get("ok") is False:
            clicked = game.run("interact", *map(str, pos), "up")
        time.sleep(1.0)
        if not game.alive():
            faults.append(f"{block}: the game stopped answering after a right click")
            break
        after = game.state(pos)
        if before == after and clicked.get("ok") is not False:
            # The click may have met another block first: once more, from above.
            game.run("interact", *map(str, pos), "up")
            time.sleep(1.0)
            after = game.state(pos)
        if clicked.get("ok") is False:
            notes.append(f"{block}: couldn't be clicked: {clicked.get('error')}")
        elif before == after:
            notes.append(f"{block}: a right click changed nothing ({before})")
    log = game.run("logs", timeout=30)
    text = json.dumps(log)
    for line in re.findall(r"[^\\\"]*(?:panicked|ERROR)[^\\\"]*", text)[:10]:
        faults.append(f"log: {line.strip()[:200]}")
    if not args.keep:
        game.run("quit", timeout=60)
    print(f"\n{len(faults)} faults, {len(notes)} notes")
    for f in faults:
        print(f"FAULT {f}")
    for n in notes:
        print(f"note  {n}")
    sys.exit(1 if faults else 0)


if __name__ == "__main__":
    main()
