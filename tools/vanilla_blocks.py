#!/usr/bin/env python3
"""Writes crates/porter-content/data/vanilla-blocks.json: each vanilla block's state
property names and whether it has an item, from the block facts Horizon's launcher
extracts for a version (`facts.json` and `gameplay-facts.json`). The porter checks
a template against it: that it is a block, which state properties a block copying
it already has, and whether its item can be the mod block's (a potted plant, a
vine's body, a portal has none, and Horizon refuses a block item copying one).

    python tools/vanilla_blocks.py [<folder holding facts.json>]

Without a folder it takes the newest `.local/horizon-client/26.3` the launcher has.
"""

import json
import os
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
OUT = ROOT / "crates" / "porter-content" / "data" / "vanilla-blocks.json"


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
        if name in blocks:
            continue
        info = state[4] if len(state) > 4 and isinstance(state[4], dict) else {}
        props = sorted((info.get("properties") or {}).keys())
        item = rows[i].get("item") not in (None, "minecraft:air")
        blocks[name] = {"item": item, "properties": props}
    OUT.parent.mkdir(parents=True, exist_ok=True)
    OUT.write_text(json.dumps(blocks, sort_keys=True, separators=(",", ":")) + "\n", encoding="utf-8")
    print(f"{len(blocks)} blocks, {sum(not b['item'] for b in blocks.values())} without an item, to {OUT}")


if __name__ == "__main__":
    main()
