#!/usr/bin/env python3
"""Compares horizon-porter's hook table (regular Horizon's native hook points) with the
`native_*` functions of a checkout of the arismoko/horizon fork.

    python tools/fork_hooks.py <fork checkout> [a port's mixins/<id>/horizon-mixin.toml]

For each hook of crates/porter-map/data/horizon-hooks.json: whether the fork's game crate
still has a function of that name, in which module, and whether its parameters are the
same. With a bundle's manifest, only the hooks that bundle wires, each on a line.

It answers how much of a horizon-mixins bundle could be applied to the fork as
`[[native.mixin]]` rows as it is: a hook with the same function and parameters takes the
bundle's helper unchanged; a changed one needs its bindings and helper redone; a missing
one became a `mod_api::Hooks` method. Run from the repository's root.
"""
import json
import re
import sys
from pathlib import Path

fork = Path(sys.argv[1])
cat = json.load(open("crates/porter-map/data/horizon-hooks.json", encoding="utf-8"))
game = fork / "crates/horizon-client/src/game"


def norm(p):
    return re.sub(r"\s+", " ", p).strip().rstrip(",")


def split_params(text):
    out, depth, cur = [], 0, ""
    for c in text:
        if c in "<([{":
            depth += 1
        elif c in ">)]}":
            depth -= 1
        if c == "," and depth == 0:
            if cur.strip():
                out.append(norm(cur))
            cur = ""
        else:
            cur += c
    if cur.strip():
        out.append(norm(cur))
    return out


found = {}
for f in game.rglob("*.rs"):
    if f.name.endswith("_tests.rs") or "tests" in f.parts:
        continue
    text = f.read_text(encoding="utf-8", errors="replace")
    for m in re.finditer(r"\bfn (native_[a-z0-9_]+)\s*(<[^>(]*>)?\s*\(", text):
        # The parameter list, to its closing parenthesis.
        i, depth = m.end(), 1
        while i < len(text) and depth:
            depth += {"(": 1, ")": -1}.get(text[i], 0)
            i += 1
        params = split_params(text[m.end():i - 1])
        rel = f.relative_to(fork / "crates/horizon-client/src").with_suffix("")
        found.setdefault(m.group(1), []).append((str(rel).replace("\\", "::").replace("/", "::"), params, bool(m.group(2))))

wired = None
if len(sys.argv) > 2:
    wired = set(t.split("::")[-1] for t in re.findall(r'target = "([^"]+)"', open(sys.argv[2], encoding="utf-8").read()))

same = changed = missing = 0
rows = []
for h in cat["hooks"]:
    name = h["target"].split("::")[-1]
    if wired is not None and name not in wired:
        continue
    if name not in found:
        missing += 1
        rows.append((name, "MISSING", ""))
        continue
    module, params, generic = found[name][0]
    want = [norm(p) for p in h["params"]]
    if params == want and not generic:
        same += 1
        rows.append((name, "same", module))
    else:
        changed += 1
        diff = [p for p in params if p not in want][:3]
        gone = [p for p in want if p not in params][:3]
        rows.append((name, "CHANGED" + (" generic" if generic else ""), "%s | new: %s | gone: %s" % (module, diff, gone)))
print("hooks compared: %d; same signature: %d; changed: %d; missing: %d" % (same + changed + missing, same, changed, missing))
for name, what, more in rows:
    if what != "same" or wired is not None:
        print("  %-44s %-8s %s" % (name, what, more[:170]))
