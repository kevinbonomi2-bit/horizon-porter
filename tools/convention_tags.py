#!/usr/bin/env python3
"""Writes crates/porter-content/data/convention-tags.json: the common `c:` item and
block tags (`c:crops/carrot`, `c:ingots/iron`) that a loader provides on Java, from
Fabric API's generated data (fabric-convention-tags-v2, Apache-2.0). Mods' recipes
name them; Horizon has none, so the porter adds the ones a mod uses and lacks.

    python tools/convention_tags.py [branch]   (needs `gh`, logged in)
"""

import json
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
PREFIX = "fabric-convention-tags-v2/src/generated/resources/data/c/tags/"


def gh(*args) -> str:
    return subprocess.run(["gh", "api", *args], capture_output=True, text=True, check=True).stdout


def main():
    branch = sys.argv[1] if len(sys.argv) > 1 else json.loads(gh("repos/FabricMC/fabric"))["default_branch"]
    tree = json.loads(gh(f"repos/FabricMC/fabric/git/trees/{branch}?recursive=1"))["tree"]
    files = [
        t["path"]
        for t in tree
        if t["path"].startswith(PREFIX)
        and t["path"].endswith(".json")
        and t["path"][len(PREFIX):].split("/")[0] in ("item", "block")
    ]
    out = {}
    for path in files:
        rel = path[len(PREFIX):-len(".json")]  # item/crops/carrot
        kind, tag = rel.split("/", 1)
        raw = gh("-H", "Accept: application/vnd.github.raw", f"repos/FabricMC/fabric/contents/{path}?ref={branch}")
        values = json.loads(raw).get("values", [])
        # `{"id": "...", "required": false}` entries kept as their id.
        out[f"{kind}:c:{tag}"] = [v if isinstance(v, str) else v.get("id") for v in values]
    dest = ROOT / "crates" / "porter-content" / "data" / "convention-tags.json"
    dest.write_text(json.dumps(out, indent=0, sort_keys=True) + "\n", encoding="utf-8")
    print(f"{len(out)} tags from FabricMC/fabric@{branch} to {dest}")


if __name__ == "__main__":
    main()
