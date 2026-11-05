#!/usr/bin/env python3
"""Extracts Horizon's native hook points into crates/porter-map/data/horizon-hooks.json.

For each hook in Horizon's native-hooks.json: its name, version, target, contract,
the Rust signature of the hooked function (receiver and parameters, as written) and
the first sentence of its doc comment. horizon-porter wires any of them from this.

    python tools/horizon_hooks.py <horizon source checkout>

The launcher keeps Horizon's source in <launcher data>/native/source/<commit>/.
Horizon is MIT licensed (Copyright (c) 2026 Ludovic Marie and the Horizon
contributors); the table is derived from it.
"""

import json
import re
import subprocess
import sys
from pathlib import Path

OUT = Path(__file__).resolve().parent.parent / "crates/porter-map/data/horizon-hooks.json"


def module_files(root: Path, target: str) -> list[Path]:
    """The source files that may hold `target`'s function."""
    parts = target.split("::")
    crate = parts[0].replace("_", "-")
    mods = [p for p in parts[1:-1] if not p.startswith("Game<")]
    base = root / "crates" / crate / "src"
    candidates = []
    if mods:
        candidates += [base.joinpath(*mods).with_suffix(".rs"), base.joinpath(*mods, "mod.rs")]
    candidates += [base / "lib.rs", base / "main.rs"]
    found = [c for c in candidates if c.is_file()]
    # Fall back to every file of the crate.
    return found or sorted(base.rglob("*.rs"))


def split_params(text: str) -> list[str]:
    """Splits a parameter list at top-level commas."""
    out, depth, cur = [], 0, ""
    for ch in text:
        if ch in "(<[{":
            depth += 1
        elif ch in ")>]}":
            depth -= 1
        if ch == "," and depth == 0:
            out.append(cur.strip())
            cur = ""
        else:
            cur += ch
    if cur.strip():
        out.append(cur.strip())
    return out


def find_fn(source: str, name: str):
    """The doc comment, generics, parameters and return type of `fn name`."""
    m = re.search(r"\bfn\s+" + re.escape(name) + r"\b\s*(<[^(]*>)?\s*\(", source)
    if not m:
        return None
    i, depth = m.end(), 1
    while depth and i < len(source):
        depth += {"(": 1, ")": -1}.get(source[i], 0)
        i += 1
    params = " ".join(source[m.end() : i - 1].split())
    rest = source[i : source.find("{", i)]
    ret = rest.split("->", 1)[1].strip() if "->" in rest else ""
    ret = " ".join(ret.split("where")[0].split())
    # Doc comment lines just above (attributes and the visibility line skipped).
    lines = source[: m.start()].splitlines()
    doc = []
    for line in reversed(lines[:-1] if lines and not lines[-1].strip().startswith("//") else lines):
        s = line.strip()
        if s.startswith("///"):
            doc.append(s[3:].strip())
        elif s.startswith("#[") or s.startswith("//") or not s:
            if doc and not s:
                break
            continue
        else:
            break
    doc.reverse()
    return (m.group(1) or "").strip(), split_params(params), ret, " ".join(doc)


def first_sentence(doc: str) -> str:
    m = re.match(r"(.+?[.:])(\s|$)", doc)
    return (m.group(1) if m else doc)[:300]


def main() -> None:
    root = Path(sys.argv[1])
    spec = json.loads((root / "native-hooks.json").read_text(encoding="utf-8"))
    try:
        commit = subprocess.run(
            ["git", "rev-parse", "HEAD"], cwd=root, capture_output=True, text=True, check=True
        ).stdout.strip()
    except Exception:
        commit = root.name
    hooks, missing = [], []
    cache: dict[Path, str] = {}
    for h in spec["hooks"]:
        fn_name = h["target"].rsplit("::", 1)[1]
        found = None
        # The target's module first, then every file (a hook point may sit in a
        # module its target path does not name).
        files = module_files(root, h["target"]) + sorted((root / "crates").rglob("*.rs"))
        for f in files:
            text = cache.setdefault(f, f.read_text(encoding="utf-8"))
            if f"fn {fn_name}" not in text:
                continue
            found = find_fn(text, fn_name)
            if found:
                break
        if not found:
            missing.append(h["name"])
            continue
        generics, params, ret, doc = found
        # "API 15" in the doc, or a bare version closing it ("... jumped. 359.").
        api = re.search(r"\bAPI (\d+)\b", doc) or re.search(r"\s(\d{1,4})\.\s*$", doc)
        hooks.append(
            {
                "name": h["name"],
                "version": h["version"],
                "target": h["target"],
                "contract": h.get("mod_api", {}).get("contract"),
                "out": h.get("mod_api", {}).get("out"),
                "api": int(api.group(1)) if api else None,
                "generics": generics,
                "params": params,
                "returns": ret,
                "doc": first_sentence(doc),
                # The vanilla methods and loader events its doc names:
                # `Screen.render`, `LivingEvent.LivingJumpEvent`, `Level#setBlock`.
                "cites": sorted(
                    {
                        c.replace("#", ".").replace("::", ".")
                        for c in re.findall(r"`([A-Z][A-Za-z0-9]*(?:(?:\.|#|::)[A-Za-z_][A-Za-z0-9_]*)+)(?:\([^`]*\))?`", doc)
                    }
                ),
            }
        )
    # The helpers a hook's code calls on the game (`game.native_set_block(...)`).
    helpers = []
    for h in spec.get("helpers", []):
        fn_name = h["target"].rsplit("::", 1)[1]
        files = module_files(root, h["target"]) + sorted((root / "crates").rglob("*.rs"))
        found = None
        for f in files:
            text = cache.setdefault(f, f.read_text(encoding="utf-8"))
            if f"fn {fn_name}" in text:
                found = find_fn(text, fn_name)
                if found:
                    break
        if not found:
            missing.append(h["name"])
            continue
        generics, params, ret, doc = found
        helpers.append(
            {
                "name": fn_name,
                "target": h["target"],
                "generics": generics,
                "params": params,
                "returns": ret,
                "doc": first_sentence(doc),
            }
        )
    # Other `native_*` functions of `Game` mods call that the helper list leaves
    # out (`native_player`, `native_chat`).
    known = {"native_" + h["name"] for h in hooks} | {h["name"] for h in helpers}
    game_file = root / "crates/horizon-client/src/game/native_hooks.rs"
    if game_file.is_file():
        text = cache.setdefault(game_file, game_file.read_text(encoding="utf-8"))
        for name in sorted(set(re.findall(r"\bpub(?:\([a-z]+\))? fn (native_[a-z0-9_]+)", text)) - known):
            found = find_fn(text, name)
            if not found:
                continue
            generics, params, ret, doc = found
            if not params or "self" not in params[0]:
                continue
            helpers.append(
                {
                    "name": name,
                    "target": f"horizon_client::game::native_hooks::Game<'_>::{name}",
                    "generics": generics,
                    "params": params,
                    "returns": ret,
                    "doc": first_sentence(doc),
                }
            )
    OUT.parent.mkdir(parents=True, exist_ok=True)
    with open(OUT, "w", encoding="utf-8", newline="\n") as f:
        json.dump({"horizon": commit, "hooks": hooks, "helpers": helpers}, f, indent=1, ensure_ascii=False)
        f.write("\n")
    print(f"{len(hooks)} hooks and {len(helpers)} helpers from {commit} to {OUT}")
    if missing:
        print("not found:", ", ".join(missing))


if __name__ == "__main__":
    main()
