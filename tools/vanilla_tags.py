#!/usr/bin/env python3
"""Writes crates/porter-translate/data/vanilla-block-tags.json: vanilla's block tags with
the blocks in each (tags inside tags followed), names without `minecraft:`, from the game
data Horizon's launcher unpacks for a version. A port's code that asks whether a block is
in a tag where Horizon gives no tag query (a plant's `canSurvive`, on the server level)
reads the tags its Java names from this table.

    python tools/vanilla_tags.py [<folder holding data/minecraft/tags/block>]
"""

import json
import os
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent


def tags_folder() -> Path:
    if len(sys.argv) > 1:
        return Path(sys.argv[1]) / "data" / "minecraft" / "tags" / "block"
    appdata = Path(os.environ.get("APPDATA", Path.home() / ".local" / "share"))
    found = sorted(
        (appdata / "horizon-launcher" / "versions").glob("*/.local/horizon-client/26.3/data/minecraft/tags/block"),
        key=lambda p: p.stat().st_mtime,
    )
    if not found:
        sys.exit("no game data found: pass the folder holding data/minecraft/tags/block")
    return found[-1]


def main() -> None:
    folder = tags_folder()
    raw = {}
    for path in folder.rglob("*.json"):
        name = path.relative_to(folder).with_suffix("").as_posix()
        values = json.loads(path.read_text(encoding="utf-8")).get("values", [])
        raw[name] = [v["id"] if isinstance(v, dict) else v for v in values]

    def blocks(name, seen=()):
        out = []
        for v in raw.get(name, []):
            if v.startswith("#"):
                inner = v[1:].removeprefix("minecraft:")
                if inner not in seen:
                    out.extend(blocks(inner, seen + (name,)))
            else:
                out.append(v.removeprefix("minecraft:"))
        return out

    table = {name: sorted(set(blocks(name))) for name in sorted(raw)}
    out = ROOT / "crates" / "porter-translate" / "data" / "vanilla-block-tags.json"
    out.write_text(json.dumps(table, separators=(",", ":")) + "\n", encoding="utf-8")
    print(f"{len(table)} tags, {sum(len(v) for v in table.values())} entries -> {out}")


if __name__ == "__main__":
    main()
