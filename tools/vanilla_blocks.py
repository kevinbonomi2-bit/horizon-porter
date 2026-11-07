#!/usr/bin/env python3
"""Writes crates/porter-content/data/vanilla-blocks.json: each vanilla block's state
properties (names and values) and whether it has an item, from the block facts Horizon's launcher
extracts for a version (`facts.json` and `gameplay-facts.json`). The porter checks
a template against it: that it is a block, which state properties a block copying
it already has, and whether its item can be the mod block's (a potted plant, a
vine's body, a portal has none, and Horizon refuses a block item copying one).

    python tools/vanilla_blocks.py [<folder holding facts.json>]

It also writes vanilla-models.json: each block model's parent and boxes.

Without a folder it takes the newest `.local/horizon-client/26.3` the launcher has.
"""

import json
import os
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
OUT = ROOT / "crates" / "porter-content" / "data" / "vanilla-blocks.json"
MODELS = ROOT / "crates" / "porter-content" / "data" / "vanilla-models.json"


def facts_folder() -> Path:
    if len(sys.argv) > 1:
        return Path(sys.argv[1])
    appdata = Path(os.environ.get("APPDATA", Path.home() / ".local" / "share"))
    found = sorted(
        (appdata / "horizon-launcher" / "versions").glob("*/.local/horizon-client/26.3/facts.json"),
        key=lambda p: p.stat().st_mtime,
    )
    if not found:
        sys.exit("no facts.json: open the Horizon launcher and build an instance once")
    return found[-1].parent


def main() -> None:
    folder = facts_folder()
    states = json.loads((folder / "facts.json").read_text(encoding="utf-8"))["states"]
    rows = json.loads((folder / "gameplay-facts.json").read_text(encoding="utf-8"))["states"]
    blocks: dict[str, dict] = {}
    for i, state in enumerate(states):
        name = state[0]
        info = state[4] if len(state) > 4 and isinstance(state[4], dict) else {}
        props = info.get("properties") or {}
        if name not in blocks:
            item = rows[i].get("item") not in (None, "minecraft:air")
            blocks[name] = {"item": item, "properties": {}, "full_cube": True}
        if rows[i].get("outline") != [[0.0, 0.0, 0.0, 1.0, 1.0, 1.0]]:
            blocks[name]["full_cube"] = False
        for k, v in props.items():
            blocks[name]["properties"].setdefault(k, set()).add(str(v))
    for b in blocks.values():
        b["properties"] = {k: sorted(v) for k, v in sorted(b["properties"].items())}
    # What each block's item has beyond a plain block item's (stone's): a carved
    # pumpkin's is worn on the head and hides the wearer from waypoints. A mod block
    # that copies one only for its shape or facing leaves these out.
    items = json.loads((folder / "items.json").read_text(encoding="utf-8"))
    plain = set(items.get("minecraft:stone", {}))
    item_of = {state[0]: rows[i].get("item") for i, state in enumerate(states)}
    stone = items.get("minecraft:stone", {})
    own = {"minecraft:item_model", "minecraft:item_name", "minecraft:lore"}
    for name, b in blocks.items():
        item = items.get(item_of.get(name) or "", {})
        extras = sorted(set(item) - plain)
        if b["item"] and extras:
            b["item_extras"] = extras
        # Components every block item has, with a value of its own here (the
        # pumpkin's attribute_modifiers): a plain item's value puts it back.
        differ = {k: stone[k] for k in item if k in stone and k not in own and item[k] != stone[k]}
        if b["item"] and differ:
            b["item_plain"] = differ
    OUT.parent.mkdir(parents=True, exist_ok=True)
    OUT.write_text(json.dumps(blocks, sort_keys=True, separators=(",", ":")) + "\n", encoding="utf-8")
    print(f"{len(blocks)} blocks, {sum(not b['item'] for b in blocks.values())} without an item, to {OUT}")
    # Each vanilla block model's parent and boxes, which a mod's models inherit
    # (`block/cube_all`, `block/cross`): the porter takes a block's shape from them.
    models = {}
    base = folder / "assets" / "minecraft" / "models" / "block"
    for f in sorted(base.glob("**/*.json")):
        m = json.loads(f.read_text(encoding="utf-8"))
        entry = {}
        if "parent" in m:
            entry["parent"] = m["parent"]
        if "elements" in m:
            entry["elements"] = [e["from"] + e["to"] for e in m["elements"]]
        models["minecraft:block/" + f.relative_to(base).with_suffix("").as_posix()] = entry
    MODELS.write_text(json.dumps(models, sort_keys=True, separators=(",", ":")) + "\n", encoding="utf-8")
    print(f"{len(models)} block models to {MODELS}")


if __name__ == "__main__":
    main()
