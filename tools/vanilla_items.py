#!/usr/bin/env python3
"""Writes crates/porter-content/data/vanilla-items.json: the ids of vanilla's items (names
without `minecraft:`), from the game data Horizon's launcher unpacks for a version. A
registration that names `Items.X` copies a vanilla item only when `x` is one: the same
words name a tag in a loader's `Tags.Items` (`Items.TOOLS_WRENCH`), and an item file whose
template is `minecraft:tools_wrench` shows as nothing.

    python tools/vanilla_items.py [<folder holding assets/minecraft/items>]
"""

import json
import os
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent


def items_folder() -> Path:
    if len(sys.argv) > 1:
        return Path(sys.argv[1]) / "assets" / "minecraft" / "items"
    appdata = Path(os.environ.get("APPDATA", Path.home() / ".local" / "share"))
    found = sorted(
        (appdata / "horizon-launcher" / "versions").glob("*/.local/horizon-client/26.3/assets/minecraft/items"),
        key=lambda p: p.stat().st_mtime,
    )
    if not found:
        sys.exit("no game data found: pass the folder holding assets/minecraft/items")
    return found[-1]


def main() -> None:
    folder = items_folder()
    names = sorted(p.stem for p in folder.glob("*.json"))
    out = ROOT / "crates" / "porter-content" / "data" / "vanilla-items.json"
    out.write_text(json.dumps(names, separators=(",", ":")) + "\n", encoding="utf-8", newline="\n")
    print(f"{len(names)} items from {folder} -> {out.relative_to(ROOT)}")


if __name__ == "__main__":
    main()
