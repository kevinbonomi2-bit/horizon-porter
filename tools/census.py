#!/usr/bin/env python3
"""A census of what the surveyed mods' Java does that the porter doesn't carry over.

For each mod `tools/survey.py` downloaded (ai-test/survey/<slug>/, ported by the
current porter), this reads the mod's decompiled Java and its port, and counts in
how many mods each pattern appears:

- the vanilla methods their classes override (`useItemOn`, `animateTick`), and
  whether the port reaches each one (a ported stub, a wired hook, a note) or
  drops it silently;
- the game calls their code makes (`level.addParticle`, `player.getInventory`)
  that the porter's runtime helpers don't name;
- the `Properties` settings their blocks and items use that the content mod
  doesn't read;
- block entity fields left out for want of a Rust type;
- Horizon hooks left to wire by hand;
- what translating each mod would cost (`translate --dry-run`, no model call);
- the gaps the translations noted in the trial ports ("Missing:", "not reachable").

Nothing is translated or built. Writes docs/CENSUS.md and ai-test/census.json.

    python tools/census.py [--no-dry-run]
"""

from __future__ import annotations

import argparse
import json
import os
import re
import subprocess
from collections import Counter, defaultdict
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
SURVEY = ROOT / "ai-test" / "survey"
TRIALS = ROOT / "ai-test" / "trials" / "ports"
EXE = ROOT / "target-dev" / "debug" / ("horizon-porter.exe" if os.name == "nt" else "horizon-porter")
# Packages that are bundled libraries, not the mod's own code.
LIBRARIES = (
    "com.google.", "org.apache.", "kotlin.", "org.jetbrains.", "it.unimi.", "javax.",
    "org.slf4j.", "com.mojang.", "net.minecraft.", "org.objectweb.", "io.netty.",
    "com.electronwill.", "org.spongepowered.", "com.llamalad7.", "dev.isxander.yacl3",
    "me.shedaniel.", "com.terraformersmc.modmenu", "org.joml.", "org.lwjgl.",
)
CLASS_RE = re.compile(
    r"\b(?:class|record|enum|interface)\s+(\w+)(?:<[^{]*?>)?"
    r"(?:\s+extends\s+([\w.]+))?(?:\s+implements\s+([\w.,<>\s?]+?))?\s*\{"
)
OVERRIDE_RE = re.compile(r"@Override\s+(?:@\w+(?:\([^)]*\))?\s+)*(?:public|protected)?\s*(?:static\s+)?(?:final\s+)?[\w.<>\[\], ?]+\s+(\w+)\s*\(")
INSTANCE_METHOD_RE = re.compile(
    r"^\s*(?:@\w+(?:\([^)]*\))?\s+)*(?:public|protected)\s+(?!static)(?:final\s+|synchronized\s+)*"
    r"(?:<[^>]+>\s+)?[\w.<>\[\], ?]+\s+(\w+)\s*\([^;{]*\)\s*(?:throws [\w., ]+)?\{",
    re.M,
)
# Java's own supers (streams, exceptions, collections): not the game's.
JAVA_SUPERS = {
    "Exception", "RuntimeException", "Error", "Throwable", "InputStream", "OutputStream",
    "Reader", "Writer", "FilterInputStream", "FilterOutputStream", "AbstractMap",
    "AbstractSet", "AbstractList", "AbstractCollection", "AbstractIterator", "Number",
    "Thread", "Random", "Object", "Enum", "Record", "SimpleFileVisitor", "SimplePathVisitor",
    "TimerTask", "ClassLoader", "URLClassLoader", "PrintStream", "IOException",
    "IllegalStateException", "IllegalArgumentException", "ZipEntry", "TypeAdapter",
}
GAME_SUPER = re.compile(r"Block|Item|Entity|Menu|Goal|Slot|Animal|Mob|Monster|Projectile|Effect|Crop|Plant|Bush|Sapling|Vine|Door|Chest|Furnace|Container|Villager|Fish|Golem")
CLIENT_SUPER = re.compile(r"Renderer|Model|Screen|Particle|Layer|Widget")
METHOD_DECL_RE = re.compile(r"(?:public|protected|private|static)[\w\s<>\[\],.?]*?\s(\w+)\s*\([^;{]*\)\s*(?:throws [\w., ]+)?\{")
CALL_RE = re.compile(r"\.([a-z]\w{3,})\(")
PROPS_RE = re.compile(r"(?:Properties\(\)|Properties\.of\(\)|Properties\.ofFullCopy\([^)]*\)|Properties\.ofLegacyCopy\([^)]*\))")
# Calls too generic to say anything (collections, strings, streams, Optional).
GENERIC = set("""
get set add put remove contains equals hashCode toString size isEmpty stream map filter
collect forEach orElse orElseGet orElseThrow ifPresent isPresent append length charAt
substring startsWith endsWith replace split trim format valueOf parse parseInt keySet
values entrySet getKey getValue iterator hasNext next clear addAll putAll containsKey
computeIfAbsent getOrDefault apply accept test compareTo toList findFirst anyMatch
allMatch noneMatch sorted limit flatMap mapToInt sum count max min reduce of empty
then build register info warn error debug getName getClass getId getType copy
""".split())


def own_class(name: str) -> bool:
    return not name.startswith(LIBRARIES)


def java_files(src: Path):
    base = src / "src" / "main" / "java"
    root = base if base.exists() else src
    for p in root.rglob("*.java"):
        rel = p.relative_to(root).as_posix()[:-5].replace("/", ".")
        if own_class(rel):
            yield rel, p


def balanced_end(text: str, open_at: int) -> int:
    """The index just past the `}` that closes the `{` at `open_at`."""
    depth = 0
    for i in range(open_at, len(text)):
        c = text[i]
        if c == "{":
            depth += 1
        elif c == "}":
            depth -= 1
            if depth == 0:
                return i + 1
    return len(text)


def read_mod(src: Path) -> dict:
    """The mod's classes: name → (super simple name, overridden methods), the method
    names it declares, the external calls it makes, its Properties settings."""
    classes: dict[str, tuple[str | None, set[str]]] = {}
    declared: set[str] = set()
    class_calls: dict[str, set[str]] = {}
    props: set[str] = set()
    for fq, path in java_files(src):
        try:
            text = path.read_text(encoding="utf-8", errors="replace")
        except OSError:
            continue
        # Strip comments and strings, roughly.
        text = re.sub(r"//[^\n]*", "", text)
        text = re.sub(r"/\*.*?\*/", "", text, flags=re.S)
        text = re.sub(r'"(?:\\.|[^"\\])*"', '""', text)
        spans = []
        for m in CLASS_RE.finditer(text):
            name, sup = m.group(1), m.group(2)
            spans.append((m.end() - 1, balanced_end(text, m.end() - 1), name, sup))
        for start, end, name, sup in spans:
            classes.setdefault(name, ((sup or "").rsplit(".", 1)[-1] or None, set()))
        # The decompiler drops `@Override`: every instance method a class declares
        # counts, and those that recur across mods are the vanilla ones.
        for m in INSTANCE_METHOD_RE.finditer(text):
            at = m.start()
            inner = [s for s in spans if s[0] <= at < s[1]]
            if inner:
                name = min(inner, key=lambda s: s[1] - s[0])[2]
                if m.group(1) != name:
                    classes[name][1].add(m.group(1))
        declared.update(m.group(1) for m in METHOD_DECL_RE.finditer(text))
        # Calls made in each class's own body (not its nested classes').
        for m in CALL_RE.finditer(text):
            inner = [s for s in spans if s[0] <= m.start() < s[1]]
            if inner:
                name = min(inner, key=lambda s: s[1] - s[0])[2]
                class_calls.setdefault(name, set()).add(m.group(1))
        for m in PROPS_RE.finditer(text):
            chain = text[m.end(): m.end() + 600]
            # The chained settings, until the expression ends.
            depth = 0
            cut = len(chain)
            for i, c in enumerate(chain):
                if c == "(":
                    depth += 1
                elif c == ")":
                    depth -= 1
                    if depth < 0:
                        cut = i
                        break
                elif c in ";" and depth == 0:
                    cut = i
                    break
            # `.strength(3.5F).sound(SoundType.WOOD).noOcclusion()`: each `.x(` that
            # starts the chain or follows a closed call.
            props.update(re.findall(r"(?:^|\))\s*\.(\w+)\(", chain[:cut]))
    external = {
        name: {c for c in cs if c not in declared and c not in GENERIC}
        for name, cs in class_calls.items()
    }
    return {"classes": classes, "external": external, "props": props}


def vanilla_super(name: str, classes: dict) -> str | None:
    """The first superclass up `name`'s chain that the mod doesn't define."""
    seen = set()
    sup = classes.get(name, (None, set()))[0]
    while sup and sup in classes and sup not in seen:
        seen.add(sup)
        sup = classes[sup][0]
    return sup


def porter_text(*crates: str) -> str:
    out = []
    for c in crates:
        for p in (ROOT / "crates" / c / "src").rglob("*.rs"):
            out.append(p.read_text(encoding="utf-8", errors="replace"))
    return "\n".join(out)


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--no-dry-run", action="store_true", help="skip the translation cost estimate")
    args = ap.parse_args()

    translate_src = porter_text("porter-translate")
    content_src = porter_text("porter-content", "porter-java")
    # Java calls the runtime helpers name in their docs: `level.playSound(..)`.
    helper_calls = set(re.findall(r"`[^`\n]*?\.([a-z]\w+)\(", translate_src))
    content_calls = set(re.findall(r"\.([a-z]\w+)\(", content_src))
    # Methods the porter reads statically (a block's `getShape` into its shapes,
    # `createBlockStateDefinition` into its states): named as strings in the Java
    # reader and the content mod's code. (porter-translate names the methods it
    # writes stubs for, which a port shows by naming them.)
    read_methods = set(re.findall(r'"([a-z]\w+)"', content_src))
    # Every word the porter's code says: a method it knows of (reads, maps to a
    # hook, or writes a stub for), whether or not this port named it.
    known_words = set(re.findall(r"\b([a-z][A-Za-z0-9]+)\b", porter_text(
        "porter-translate", "porter-content", "porter-java", "porter-map", "porter-model")))

    mods = []
    overrides = defaultdict(lambda: {"mods": set(), "reached": set(), "families": Counter(), "examples": set()})
    calls = defaultdict(set)
    props = defaultdict(set)
    fields = defaultdict(set)
    hooks = defaultdict(set)
    costs = []
    for d in sorted(SURVEY.iterdir()):
        port = d / "port"
        meta = port / ".porter" / "port.json"
        if not meta.exists():
            continue
        slug = d.name
        src = Path(json.loads(meta.read_text(encoding="utf-8"))["input"])
        if not src.exists():
            continue
        mod = read_mod(src)
        runtime = "\n".join(p.read_text(encoding="utf-8", errors="replace") for p in port.glob("mixins/*/runtime.rs"))
        porting = (port / "PORTING.md").read_text(encoding="utf-8", errors="replace") if (port / "PORTING.md").exists() else ""
        said = (runtime + "\n" + porting).splitlines()
        for name, (_, methods) in mod["classes"].items():
            sup = vanilla_super(name, mod["classes"])
            if not sup or not methods or sup in JAVA_SUPERS:
                continue
            named = [l for l in said if name in l]
            for m in methods:
                key = m
                overrides[key]["mods"].add(slug)
                overrides[key]["families"][sup] += 1
                if m in read_methods or any(m in l for l in named):
                    overrides[key]["reached"].add(slug)
                else:
                    overrides[key]["examples"].add(f"{slug}: {name}")
        # Calls from the mod's game classes: what a translation of their code needs.
        for name, cs in mod["external"].items():
            sup = vanilla_super(name, mod["classes"])
            if sup and GAME_SUPER.search(sup) and not CLIENT_SUPER.search(sup):
                for c in cs:
                    calls[c].add(slug)
        for p in mod["props"]:
            props[p].add(slug)
        for m in re.finditer(r"^// - `([\w.]+)(?:<[^`]*>)?(?:\[\])? \w+`: no Rust type", runtime, re.M):
            fields[m.group(1)].add(slug)
        section = porting.split("## Hooks to wire by hand", 1)
        if len(section) == 2:
            for row in re.findall(r"^\| `?(\w+)`? \|", section[1], re.M):
                if row not in ("Horizon", "---"):
                    hooks[row].add(slug)
        cost = None
        if not args.no_dry_run and EXE.exists():
            r = subprocess.run([str(EXE), "translate", str(port), "--dry-run"], capture_output=True, text=True)
            out = r.stdout + r.stderr
            m = re.search(r"(\d+) functions? to translate, about ([\d,]+) input tokens", out)
            dollars = re.search(r"about \$([\d.]+)", out)
            if m:
                cost = {
                    "functions": int(m.group(1)),
                    "tokens": int(m.group(2).replace(",", "")),
                    "dollars": float(dollars.group(1)) if dollars else None,
                }
            elif "Nothing to translate" in out or "nothing to translate" in out:
                cost = {"functions": 0, "tokens": 0, "dollars": 0.0}
        costs.append((slug, cost))
        mods.append(slug)

    # The translations' own notes in the trial ports.
    notes = Counter()
    note_mods = defaultdict(set)
    for rt in TRIALS.glob("*-port/mixins/*/runtime.rs"):
        slug = rt.parts[-4][: -len("-port")]
        for line in rt.read_text(encoding="utf-8", errors="replace").splitlines():
            l = line.strip().lstrip("/").strip()
            if re.search(r"not reachable|Missing:|cannot (reach|run|hold|test|answer|be)", l, re.I):
                notes[l[:140]] += 1
                note_mods[l[:140]].add(slug)

    n = len(mods)
    data = {
        "mods": mods,
        "overrides": {
            k: {
                "mods": len(v["mods"]),
                "reached": len(v["reached"]),
                "families": v["families"].most_common(4),
            }
            for k, v in overrides.items()
        },
        "calls": {k: len(v) for k, v in calls.items()},
        "props": {k: len(v) for k, v in props.items()},
        "fields": {k: sorted(v) for k, v in fields.items()},
        "hooks": {k: sorted(v) for k, v in hooks.items()},
        "costs": dict(costs),
    }
    (ROOT / "ai-test" / "census.json").write_text(json.dumps(data, indent=1), encoding="utf-8")

    r = []
    r.append(f"# Census: what {n} mods' Java does that the porter doesn't carry over\n")
    r.append(
        "Written by `tools/census.py` from the mods `tools/survey.py` downloaded, each ported "
        "by the current porter (nothing translated or built). A count is how many mods "
        "do it, so a fix's reach is its count. The jars stay local (`ai-test/`).\n"
    )

    def area(v) -> str:
        fams = [f for f, _ in v["families"].most_common(3)]
        if all(re.search(r"Provider|Generator|DataGen|Datagen", f) for f in fams):
            return "datagen"
        if any(re.search(r"Screen|Widget|Button|EditBox|Slider|EventHandler|Entry|Toast|Overlay|Renderer|Model|Particle", f) for f in fams):
            return "client"
        return "game"

    r.append("\n## Vanilla methods mods override that the porter doesn't know\n")
    r.append(
        "A method the mods' classes declare over a vanilla superclass (a block's "
        "`useItemOn`, an item's `inventoryTick`; the decompiler drops `@Override`, so a "
        "method that recurs across mods is taken as one). *Not named* counts the mods whose "
        "port names it nowhere (no stub, hook or note); *unknown* is when the porter's code "
        "doesn't name it at all, so it is dropped without a word. Data generators (run "
        "only when the mod is built) are left out. Ranked by unknown, then not named.\n"
    )
    for title, which in (("Game logic", "game"), ("Client: screens, widgets, renderers, particles", "client")):
        r.append(f"\n### {title}\n\n| Method | Mods | Not named | Unknown | Mostly on | e.g. |\n| --- | --- | --- | --- | --- | --- |\n")
        rows = []
        for k, v in overrides.items():
            if area(v) != which:
                continue
            total = len(v["mods"])
            missed = total - len(v["reached"])
            unknown = missed if k not in known_words else 0
            if missed < 3:
                continue
            rows.append((unknown, missed, total, k, v))
        rows.sort(key=lambda x: (-x[0], -x[1]))
        for unknown, missed, total, k, v in rows[:45]:
            fams = ", ".join(f"`{f}`" for f, _ in v["families"].most_common(3))
            eg = sorted(v["examples"])[0] if v["examples"] else ""
            r.append(f"| `{k}` | {total} | {missed} | {unknown or ''} | {fams} | {eg} |\n")

    r.append("\n## Game calls no runtime helper names\n")
    r.append(
        "Calls the mods' game classes (blocks, items, block entities, mobs, menus) make "
        "that the porter's helpers (`World`, `Machine`, `Blocks`, `ScreenAt`) don't name "
        "in their docs, so a translation of that code may have nothing to call. Some are "
        "covered without being named (a position's `getX` is `pos[0]`); generic names "
        "(`get`, `stream`) are left out.\n\n| Call | Mods |\n| --- | --- |\n"
    )
    missing_calls = sorted(
        ((k, len(v)) for k, v in calls.items() if k not in helper_calls),
        key=lambda kv: -kv[1],
    )
    for k, c in missing_calls[:80]:
        if c < 5:
            break
        r.append(f"| `.{k}(` | {c} |\n")

    r.append("\n## Block and item settings the content mod doesn't read\n")
    r.append(
        "`Properties` settings (`.strength(..)`, `.craftRemainder(..)`) that appear in the "
        "mods and nowhere in porter-content or porter-java.\n\n| Setting | Mods |\n| --- | --- |\n"
    )
    for k, v in sorted(props.items(), key=lambda kv: -len(kv[1])):
        if k in content_calls or len(v) < 2:
            continue
        r.append(f"| `.{k}(` | {len(v)} |\n")

    r.append("\n## Block entity fields with no Rust type\n\n| Java type | Mods |\n| --- | --- |\n")
    for k, v in sorted(fields.items(), key=lambda kv: -len(kv[1]))[:25]:
        r.append(f"| `{k}` | {len(v)} |\n")

    r.append("\n## Horizon hooks left to wire by hand\n\n")
    if hooks:
        r.append("| Hook | Mods |\n| --- | --- |\n")
        for k, v in sorted(hooks.items(), key=lambda kv: -len(kv[1]))[:25]:
            r.append(f"| `{k}` | {len(v)} |\n")
    else:
        r.append("None: every Horizon hook the mods' code maps to is wired by the porter.\n")

    known = [c for _, c in costs if c]
    if known:
        funcs = sum(c["functions"] for c in known)
        dollars = sum(c["dollars"] or 0 for c in known)
        r.append("\n## What translating them would cost\n\n")
        r.append(
            f"`translate --dry-run` (no model called) for {len(known)} mods: {funcs} functions, "
            f"about ${dollars:.2f} with Haiku for one try each (fixes add more).\n\n"
            "| Mod | Functions | About |\n| --- | --- | --- |\n"
        )
        for slug, c in sorted(costs, key=lambda sc: -(sc[1] or {}).get("functions", 0))[:20]:
            if c:
                r.append(f"| {slug} | {c['functions']} | ${c['dollars'] or 0:.2f} |\n")

    if notes:
        r.append("\n## Gaps the translations noted (trial ports)\n\n")
        r.append(
            "What Haiku wrote it couldn't do in the 8 translated trial mods, as it said it: "
            "each names a helper or hook the porter lacks.\n\n| Note | Times | Mods |\n| --- | --- | --- |\n"
        )
        for k, c in notes.most_common(40):
            r.append(f"| {k.replace('|', '/')} | {c} | {len(note_mods[k])} |\n")

    (ROOT / "docs" / "CENSUS.md").write_text("".join(r), encoding="utf-8")
    print(f"{n} mods; wrote docs/CENSUS.md and ai-test/census.json")


if __name__ == "__main__":
    main()
