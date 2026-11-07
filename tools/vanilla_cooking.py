#!/usr/bin/env python3
"""Writes crates/porter-translate/data/vanilla-cooking.json: vanilla's cooking recipes
(campfire, smoker, furnace, blast furnace) as [type, ingredient, result, count, ticks],
from the game data Horizon's launcher unpacks for a version. A port's code that looks
up a vanilla cooking recipe (Farmer's Delight's skillet cooks what a campfire does)
reads them from this table: Horizon's recipe helper serves only the mods' own types.

    python tools/vanilla_cooking.py [<folder holding data/minecraft/recipe>]
"""

import json
import os
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
TYPES = {
    "minecraft:campfire_cooking",
    "minecraft:smoking",
    "minecraft:smelting",
    "minecraft:blasting",
}


def recipes_folder() -> Path:
    if len(sys.argv) > 1:
        return Path(sys.argv[1]) / "data" / "minecraft" / "recipe"
    appdata = Path(os.environ.get("APPDATA", Path.home() / ".local" / "share"))
    found = sorted(
        (appdata / "horizon-launcher" / "versions").glob("*/.local/horizon-client/26.3/data/minecraft/recipe"),
        key=lambda p: p.stat().st_mtime,
    )
    if not found:
        sys.exit("no recipes: open the Horizon launcher and build an instance once")
    return found[-1]


def ingredient(value) -> str | None:
    if isinstance(value, str):
        return value
    if isinstance(value, list) and value:
        return ingredient(value[0])
    if isinstance(value, dict):
        if "item" in value:
            return value["item"]
        if "tag" in value:
            return "#" + value["tag"]
    return None


def main():
    rows = []
    for f in sorted(recipes_folder().glob("*.json")):
        r = json.loads(f.read_text(encoding="utf-8"))
        if r.get("type") not in TYPES:
            continue
        what = ingredient(r.get("ingredient"))
        result = r.get("result")
        if isinstance(result, str):
            result = {"id": result}
        if not what or not isinstance(result, dict) or "id" not in result:
            continue
        rows.append([r["type"], what, result["id"], result.get("count", 1), r.get("cookingtime", 200)])
    out = ROOT / "crates" / "porter-translate" / "data" / "vanilla-cooking.json"
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text(json.dumps(rows, separators=(",", ":")) + "\n", encoding="utf-8")
    print(f"{len(rows)} recipes to {out}")


if __name__ == "__main__":
    main()
