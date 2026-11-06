#!/usr/bin/env python3
"""Port popular Modrinth mods and see how far each gets.

For the most downloaded mods with a Minecraft 26.3 version, this downloads the
jar (NeoForge first, else Fabric) into ai-test/survey/<slug>/ (checked against
Modrinth's SHA-512), runs `horizon-porter port` on it, runs the generated port's
own tests, and compiles its Horizon helpers against the API mock. It writes
ai-test/survey/report.md and results.json: what carried over, what is left to
port, and what failed, so the porter's next fixes go where most mods need them.

    python tools/survey.py [--count 40] [--slugs a,b,c] [--skip-download]

The jars are other people's mods, for local testing only: ai-test/ is not
committed.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import re
import subprocess
import sys
import time
import urllib.parse
import urllib.request
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
OUT = ROOT / "ai-test" / "survey"
UA = "horizon-porter-survey (github.com/kevinbonomi2-bit/horizon-porter)"
API = "https://api.modrinth.com/v2"
EXE = ROOT / "target" / "debug" / ("horizon-porter.exe" if os.name == "nt" else "horizon-porter")
CARGO = Path.home() / ".cargo" / "bin" / ("cargo.exe" if os.name == "nt" else "cargo")
# Mods that are only an API or a library for other mods: nothing to play.
LIBRARY_ONLY = {"library"}


def get(url: str) -> bytes:
    req = urllib.request.Request(url, headers={"User-Agent": UA})
    for attempt in range(4):
        try:
            with urllib.request.urlopen(req, timeout=60) as r:
                return r.read()
        except Exception:
            if attempt == 3:
                raise
            time.sleep(2 * (attempt + 1))
    raise RuntimeError("unreachable")


def popular(count: int, categories: list[str], server: bool = False) -> list[dict]:
    groups = [["versions:26.3"], ["project_type:mod"], ["categories:neoforge", "categories:fabric"]]
    if categories:
        groups.append([f"categories:{c}" for c in categories])
    facets = json.dumps(groups)
    out: list[dict] = []
    offset = 0
    while len(out) < count and offset < 400:
        q = urllib.parse.urlencode(
            {"limit": 100, "offset": offset, "index": "downloads", "facets": facets}
        )
        hits = json.loads(get(f"{API}/search?{q}"))["hits"]
        if not hits:
            break
        for h in hits:
            kinds = set(h["categories"]) - {"fabric", "neoforge", "forge", "quilt", "babric"}
            if kinds and kinds <= LIBRARY_ONLY:
                continue
            # Gameplay and content: mods a server needs too.
            if server and h.get("server_side") != "required":
                continue
            out.append(h)
            if len(out) == count:
                break
        offset += 100
    return out


def pick_file(slug: str) -> dict | None:
    q = urllib.parse.urlencode({"game_versions": json.dumps(["26.3"])})
    versions = json.loads(get(f"{API}/project/{slug}/version?{q}"))
    for loader in ("neoforge", "fabric"):
        for v in versions:
            if loader in v["loaders"]:
                files = [f for f in v["files"] if f["primary"]] or v["files"]
                if files and files[0]["filename"].endswith(".jar"):
                    return {"loader": loader, "version": v["version_number"], **files[0]}
    return None


def download(slug: str, f: dict) -> Path:
    folder = OUT / slug / "jar"
    folder.mkdir(parents=True, exist_ok=True)
    jar = folder / f["filename"]
    if not jar.exists() or hashlib.sha512(jar.read_bytes()).hexdigest() != f["hashes"]["sha512"]:
        data = get(f["url"])
        if hashlib.sha512(data).hexdigest() != f["hashes"]["sha512"]:
            raise RuntimeError("the download does not match Modrinth's SHA-512")
        jar.write_bytes(data)
    return jar


def run(args: list[str], cwd: Path | None = None, env: dict | None = None, timeout: int = 900):
    try:
        p = subprocess.run(
            args,
            cwd=cwd,
            env={**os.environ, **(env or {})},
            capture_output=True,
            text=True,
            encoding="utf-8",
            errors="replace",
            timeout=timeout,
        )
        return p.returncode, p.stdout + p.stderr
    except subprocess.TimeoutExpired:
        return -1, f"timed out after {timeout} s"


CHECKER_MAIN = r"""//! Reads each content folder given as Horizon's mod loader does, and says what it refuses.
use horizon_mods::{content::Content, manifest::Package};
use std::path::Path;

fn main() {
    let mut bad = 0;
    for arg in std::env::args().skip(1) {
        let dir = Path::new(&arg);
        match Package::read(dir).and_then(|p| Content::read(dir, &p.manifest)) {
            Ok(_) => println!("ok {arg}"),
            Err(e) => {
                bad += 1;
                println!("REFUSED {arg}: {e}");
            }
        }
    }
    std::process::exit(i32::from(bad > 0));
}
"""


def content_checker() -> Path | None:
    """Horizon's own content loader (horizon-mods, from the launcher's Horizon
    source), built offline with the launcher's crates: what a player's game
    refuses to load. None without a Horizon source here."""
    appdata = Path(os.environ.get("APPDATA", Path.home() / ".local" / "share"))
    sources = sorted((appdata / "horizon-launcher" / "native" / "source").glob("*/crates/horizon-mods"),
                     key=lambda p: p.stat().st_mtime)
    if not sources:
        return None
    crate = OUT / "content-check"
    (crate / "src").mkdir(parents=True, exist_ok=True)
    manifest = "\n".join([
        "[package]",
        'name = "content-check"',
        'version = "0.0.0"',
        'edition = "2024"',
        "publish = false",
        "",
        "[dependencies]",
        f"horizon-mods = {{ path = {json.dumps(sources[-1].as_posix())} }}",
        "",
        "[workspace]",
        "",
    ])
    (crate / "Cargo.toml").write_text(manifest, encoding="utf-8")
    (crate / "src" / "main.rs").write_text(CHECKER_MAIN, encoding="utf-8")
    code, out = run([str(CARGO), "build", "-q", "--offline"], cwd=crate,
                    env={"CARGO_HOME": str(appdata / "horizon-launcher" / "cargo")})
    exe = crate / "target" / "debug" / ("content-check.exe" if os.name == "nt" else "content-check")
    return exe if code == 0 and exe.exists() else None


CHECKER: Path | None = None
VANILLA = json.loads((ROOT / "crates" / "porter-content" / "data" / "vanilla-blocks.json").read_text(encoding="utf-8"))


def registry_problems(content: Path) -> list[str]:
    out = []
    # Horizon reads JSON strictly: no byte order mark, nothing malformed.
    for f in sorted(content.rglob("*.json")):
        raw = f.read_bytes()
        if raw.startswith(bytes([0xEF, 0xBB, 0xBF])):
            out.append(f"{f.relative_to(content).as_posix()}: starts with a byte order mark")
            continue
        try:
            json.loads(raw.decode("utf-8"))
        except Exception as e:  # noqa: BLE001
            out.append(f"{f.relative_to(content).as_posix()}: {e}")
    for f in sorted(content.glob("data/*/horizon/block/*.json")):
        b = json.loads(f.read_text(encoding="utf-8"))
        t = b.get("template", "")
        v = VANILLA.get(t)
        if v is None:
            out.append(f"{f.name}: template {t} is not a block")
            continue
        if b.get("item", True) and not v["item"]:
            out.append(f"{f.name}: template {t} has no item")
        for p in b.get("state_properties", {}):
            if p in v["properties"]:
                out.append(f"{f.name}: state property {p} is already on {t}")
    return out


def first_errors(text: str, n: int = 3) -> list[str]:
    lines = [l.strip() for l in text.splitlines()]
    errs = [l for l in lines if l.startswith("error") or l.startswith("Error")]
    return errs[:n] or [l for l in lines if l][-n:]


def survey_one(slug: str, hit: dict | None, skip_download: bool) -> dict:
    r: dict = {"slug": slug, "downloads": hit["downloads"] if hit else None}
    try:
        jars = sorted((OUT / slug / "jar").glob("*.jar"))
        if skip_download and jars:
            jar = jars[-1]
            r["loader"] = "?"
        else:
            f = pick_file(slug)
            if not f:
                r["stage"] = "no 26.3 jar"
                return r
            r["loader"], r["version"] = f["loader"], f["version"]
            jar = download(slug, f)
        r["jar"] = jar.name
    except Exception as e:  # noqa: BLE001
        r["stage"] = f"download failed: {e}"
        return r

    # What the scan says, before porting: the verdict and the size.
    code, out = run([str(EXE), "scan", str(jar)])
    m = re.search(r"Code\s+([\d,]+) classes", out)
    r["classes"] = int(m.group(1).replace(",", "")) if m else None
    m = re.search(r"Mixins\s+([\d,]+) classes", out)
    r["mixin_classes"] = int(m.group(1).replace(",", "")) if m else 0
    m = re.search(r"Portable\?\s+(.+)", out)
    r["verdict"] = m.group(1).split(".")[0] if m else "no Mixins to judge"

    port = OUT / slug / "port"
    code, out = run([str(EXE), "port", str(jar), "--out", str(port), "--force"], timeout=1800)
    if code != 0:
        r["stage"] = "port failed"
        r["errors"] = first_errors(out)
        return r
    m = re.search(r"Content: (\d+) blocks, (\d+) items, (\d+) resource", out)
    r["blocks"], r["items"], r["resources"] = (int(x) for x in m.groups()) if m else (0, 0, 0)
    m = re.search(r"Block entities: (.+)", out)
    r["block_entities"] = len(re.findall(r"\(\d+ saved", m.group(1))) if m else 0
    m = re.search(r"Wired hooks: (.+)", out)
    r["wired"] = len(m.group(1).split(",")) if m else 0
    m = re.search(r"Hooks to port into \(helpers ready\): (.+)", out)
    r["stubbed"] = len(m.group(1).split(",")) if m else 0
    r["mobs"] = len(list(port.glob("content/data/*/horizon/entity_type/*.json")))
    # The content as Horizon's loader reads it.
    if CHECKER and (port / "content" / "horizon-mod.toml").exists():
        code, out = run([str(CHECKER), str(port / "content")])
        if code != 0:
            r["stage"] = "content refused"
            r["errors"] = [l.split(": ", 1)[-1] for l in out.splitlines() if l.startswith("REFUSED")][:3]
            return r
    # What Horizon checks as it builds the registries (mod_content.rs): a template
    # is a block, a block item copies a block that has an item, no declared state
    # property is already the template's. One refused block keeps every content
    # mod from loading.
    problems = registry_problems(port / "content")
    if problems:
        r["stage"] = "content refused"
        r["errors"] = problems[:3]
        return r
    mixins = list((port / "mixins").glob("*"))
    r["native"] = bool(mixins)
    r["todo"] = sum(
        p.read_text(encoding="utf-8", errors="replace").count("TODO: port")
        for d in mixins
        for p in d.glob("*.rs")
    )
    if not mixins:
        r["stage"] = "ported (content only)"
        return r

    # The port's own tests, then its helpers against the Horizon API mock.
    target = OUT / "target"
    code, out = run([str(CARGO), "test", "--quiet"], cwd=port, env={"CARGO_TARGET_DIR": str(target)})
    if code != 0:
        r["stage"] = "its tests fail"
        r["errors"] = first_errors(out)
        return r
    code, out = run(
        [str(CARGO), "test", "-q", "-p", "porter-translate", "--test", "horizon_mock", "--",
         "--ignored", "port_folder"],
        cwd=ROOT,
        env={"PORTER_PORT": str(port)},
    )
    if code != 0:
        r["stage"] = "helpers don't compile"
        errs = [
            e for e in re.findall(r"(?:error(?:\[E\d+\])?|warning): .+", out)
            if "test failed" not in e and "generated" not in e
        ]
        locs = re.findall(r"--> .*?([\w.]+\.rs:\d+)", out)
        r["errors"] = [f"{e} ({l})" for e, l in zip(errs, locs + [""] * len(errs))][:3] or first_errors(out)
        return r
    r["stage"] = "ported, compiles"
    return r


def report(results: list[dict]) -> str:
    stages: dict[str, int] = {}
    for r in results:
        stages[r.get("stage", "?")] = stages.get(r.get("stage", "?"), 0) + 1
    lines = [
        "# Survey: popular 26.3 mods through horizon-porter",
        "",
        f"{len(results)} mods, {time.strftime('%Y-%m-%d %H:%M')}. "
        + ", ".join(f"{n} {s}" for s, n in sorted(stages.items(), key=lambda x: -x[1])),
        "",
        "| Mod | Loader | Classes | Mixins | Verdict | Blocks | Items | BEs | Mobs | Hooks | Left to port | Result |",
        "| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |",
    ]
    for r in results:
        hooks = f"{r.get('wired', 0)}+{r.get('stubbed', 0)}" if "wired" in r else ""
        lines.append(
            f"| {r['slug']} | {r.get('loader', '')} | {r.get('classes') or ''} | {r.get('mixin_classes') or ''} "
            f"| {r.get('verdict', '')} | {r.get('blocks', '')} | {r.get('items', '')} | {r.get('block_entities', '')} | {r.get('mobs', '')} "
            f"| {hooks} | {r.get('todo', '')} | {r.get('stage', '?')} |"
        )
    errors = [r for r in results if r.get("errors")]
    if errors:
        lines += ["", "## Errors", ""]
        for r in errors:
            lines.append(f"- **{r['slug']}** ({r['stage']}):")
            lines += [f"  - `{e[:300]}`" for e in r["errors"]]
    return "\n".join(lines) + "\n"


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--count", type=int, default=40)
    ap.add_argument("--slugs", default="")
    ap.add_argument("--skip-download", action="store_true", help="reuse the jars already here")
    ap.add_argument("--categories", default="", help="only mods in these Modrinth categories (any of them)")
    ap.add_argument("--name", default="report", help="the report's name: <name>.md and <name>.json")
    ap.add_argument("--server", action="store_true", help="only mods a server needs too: gameplay and content")
    args = ap.parse_args()
    OUT.mkdir(parents=True, exist_ok=True)
    global CHECKER
    CHECKER = content_checker()
    print("Horizon's content loader:", "found" if CHECKER else "not found (content not checked)", flush=True)
    if args.slugs:
        mods = [(s, None) for s in args.slugs.split(",") if s]
    else:
        cats = [c for c in args.categories.split(",") if c]
        mods = [(h["slug"], h) for h in popular(args.count, cats, args.server)]
    results = []
    for i, (slug, hit) in enumerate(mods, 1):
        print(f"[{i}/{len(mods)}] {slug}", flush=True)
        r = survey_one(slug, hit, args.skip_download)
        print(f"    {r.get('stage')}", flush=True)
        results.append(r)
        (OUT / f"{args.name}.json").write_text(json.dumps(results, indent=1), encoding="utf-8")
        (OUT / f"{args.name}.md").write_text(report(results), encoding="utf-8")
    print(f"Wrote {OUT / (args.name + '.md')}")


if __name__ == "__main__":
    sys.exit(main())
