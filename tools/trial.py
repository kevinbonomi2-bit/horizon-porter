#!/usr/bin/env python3
"""Trials mods the way a player would: port, translate, build, play. For each jar
(from a survey's downloads) it runs the whole flow into one Horizon instance kept for
trials, one mod at a time, and writes what happened at each step:

  1. `horizon-porter port` into a fresh folder;
  2. `horizon-porter translate` with the default model (Claude Haiku, the cheapest),
     skipped when its estimate is over --cap dollars;
  3. a scan of the translated code for what a block's or item's code never needs
     (files, processes, the network, unsafe): the build is skipped if found;
  4. the instance's mods cleared, this one installed and built
     (`install --build --trust-native-code`);
  5. `horizon-porter check`: every block placed, those that act right-clicked.

    python tools/trial.py <slug>... [--instance "porter trials"] [--cap 0.5] [--name trials]

Jars come from ai-test/survey/<slug>/jar/. The report goes to ai-test/trials/<name>.md.
Translation spends from the API key saved in the app: the estimate is printed first.
"""

import argparse
import json
import os
import re
import shutil
import subprocess
import sys
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
EXE = ROOT / "target-dev" / "debug" / ("horizon-porter.exe" if os.name == "nt" else "horizon-porter")
SURVEY = ROOT / "ai-test" / "survey"
OUT = ROOT / "ai-test" / "trials"
INSTANCES = Path(os.environ.get("APPDATA", Path.home() / ".local" / "share")) / "horizon" / "instances"
# What a block's or item's code has no business doing.
DANGER = re.compile(r"std::(process|fs|net|env)|\bunsafe\b|Command::new|File::|TcpStream|include_bytes")


def run(args, timeout=3600):
    try:
        r = subprocess.run([str(a) for a in args], capture_output=True, text=True, encoding="utf-8",
                           errors="replace", timeout=timeout)
        return r.returncode, (r.stdout or "") + (r.stderr or "")
    except subprocess.TimeoutExpired:
        return 124, "timed out"


def last(text, n=3):
    lines = [l for l in text.splitlines() if l.strip()]
    return " / ".join(lines[-n:])


def trial(slug: str, instance: str, cap: float) -> dict:
    r = {"slug": slug}
    jars = sorted((SURVEY / slug / "jar").glob("*.jar"))
    if not jars:
        r["stage"] = "no jar"
        return r
    port = OUT / "ports" / f"{slug}-port"
    if port.exists():
        shutil.rmtree(port)
    port.parent.mkdir(parents=True, exist_ok=True)
    print(f"  port", flush=True)
    code, out = run([EXE, "port", jars[0], "--out", port], 1800)
    if code != 0:
        r.update(stage="port failed", error=last(out))
        return r
    m = re.search(r"Content: (\d+) blocks, (\d+) items", out)
    r["blocks"], r["items"] = (int(m[1]), int(m[2])) if m else (0, 0)
    r["native"] = (port / "horizon-native.json").exists()
    if r["native"]:
        code, out = run([EXE, "translate", port, "--dry-run"], 600)
        m = re.search(r"(\d+) functions? to translate", out)
        r["functions"] = int(m[1]) if m else 0
        m = re.search(r"about \$([\d.]+)", out)
        r["estimate"] = float(m[1]) if m else 0.0
        if r["functions"] and r["estimate"] <= cap:
            print(f"  translate {r['functions']} functions (about ${r['estimate']:.2f})", flush=True)
            code, out = run([EXE, "translate", port, "--yes"], 7200)
            m = re.search(r"Ported and tested: (\d+)\. Ported, checked when you build: (\d+)\. Left as they were: (\d+)", out)
            if m:
                r["translated"], r["unchecked"], r["left"] = int(m[1]), int(m[2]), int(m[3])
            m = re.search(r"Tokens: ([\d,]+) in, ([\d,]+) out", out)
            if m:
                tin, tout = (int(x.replace(",", "")) for x in m.groups())
                r["cost"] = round(tin / 1e6 * 0.10 + tout / 1e6 * 0.50, 3)
            if code != 0:
                r["translate_error"] = last(out)
        elif r["functions"]:
            r["skipped"] = f"translation estimate ${r['estimate']:.2f} over the cap"
        hits = [f"{p.name}:{i + 1}" for p in port.glob("mixins/*/*.rs")
                for i, line in enumerate(p.read_text(encoding="utf-8").splitlines()) if DANGER.search(line)]
        if hits:
            r.update(stage="not built: the code reaches past the game", error=", ".join(hits[:5]))
            return r
    # One mod at a time: the instance's other mods out.
    home = INSTANCES / instance.replace(" ", "-")
    for d in ("horizon-mods", "horizon-native-mods"):
        for f in (home / d).glob("*"):
            if f.is_file():
                f.unlink()
    (home / "horizon-mods.txt").unlink(missing_ok=True)
    print(f"  install and build", flush=True)
    args = [EXE, "install", port, "--instance", instance]
    if r["native"]:
        args += ["--build", "--trust-native-code"]
    code, out = run(args, 3600)
    if code != 0:
        err = [l for l in out.splitlines() if l.startswith("error")]
        r.update(stage="build failed", error=" / ".join(err[:3]) or last(out))
        return r
    print(f"  check in the game", flush=True)
    code, out = run([EXE, "check", port, "--instance", instance], 3600)
    m = re.search(r"(\d+) of (\d+) blocks place(?:, (\d+) of (\d+) respond to a click)?", out)
    if m:
        r["placed"], r["checked_blocks"] = int(m[1]), int(m[2])
        r["responded"], r["acting"] = int(m[3] or 0), int(m[4] or 0)
    r["faults"] = [l[6:] for l in out.splitlines() if l.startswith("FAULT ")]
    r["notes"] = [l[6:] for l in out.splitlines() if l.startswith("note  ")]
    r["stage"] = "plays" if not r["faults"] and m else ("faults in the game" if m else "check failed")
    if not m:
        r["error"] = last(out)
    return r


def report(rows: list[dict], name: str) -> str:
    lines = [
        f"# Trial: {len(rows)} mods ported, translated, built and played",
        "",
        f"{time.strftime('%Y-%m-%d %H:%M')}, `tools/trial.py`, translation with Claude Haiku.",
        "",
        "| Mod | Blocks | Items | Functions | Translated | Cost | Placed | Clicks | Result |",
        "| --- | --- | --- | --- | --- | --- | --- | --- | --- |",
    ]
    for r in rows:
        clicks = f"{r.get('responded', '')}/{r.get('acting', '')}" if r.get("acting") else ""
        placed = f"{r.get('placed', '')}/{r.get('checked_blocks', '')}" if "placed" in r else ""
        cost = f"${r['cost']:.2f}" if "cost" in r else ""
        lines.append(
            f"| {r['slug']} | {r.get('blocks', '')} | {r.get('items', '')} | {r.get('functions', '')} "
            f"| {r.get('translated', '')} | {cost} | {placed} | {clicks} | {r.get('stage', '?')} |"
        )
    lines.append("")
    for r in rows:
        if r.get("error") or r.get("faults") or r.get("skipped") or r.get("translate_error"):
            lines.append(f"## {r['slug']}: {r.get('stage')}")
            for k in ("error", "skipped", "translate_error"):
                if r.get(k):
                    lines.append(f"- {k}: {r[k]}")
            for f in r.get("faults", [])[:10]:
                lines.append(f"- fault: {f}")
            lines.append("")
    total = sum(r.get("cost", 0) for r in rows)
    lines.append(f"Translation cost, all mods: about ${total:.2f}.")
    return "\n".join(lines) + "\n"


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("slugs", nargs="+")
    ap.add_argument("--instance", default="porter trials")
    ap.add_argument("--cap", type=float, default=0.5, help="skip translating a mod estimated above this many dollars")
    ap.add_argument("--name", default="trials")
    args = ap.parse_args()
    OUT.mkdir(parents=True, exist_ok=True)
    rows = []
    for i, slug in enumerate(args.slugs, 1):
        print(f"[{i}/{len(args.slugs)}] {slug}", flush=True)
        try:
            rows.append(trial(slug, args.instance, args.cap))
        except Exception as e:  # noqa: BLE001
            rows.append({"slug": slug, "stage": "trial crashed", "error": str(e)})
        print(f"    {rows[-1].get('stage')}", flush=True)
        (OUT / f"{args.name}.json").write_text(json.dumps(rows, indent=1), encoding="utf-8")
        (OUT / f"{args.name}.md").write_text(report(rows, args.name), encoding="utf-8")
    print(f"Wrote {OUT / (args.name + '.md')}")


if __name__ == "__main__":
    main()
