#!/usr/bin/env python3
"""Checks content mods with Horizon's own code (tools/horizon-check): the content
build, the world's rules and every block's models, as the game loads them.

    python tools/horizon_check.py <mod>...        a .hmod or a port's content/ folder

It builds tools/horizon-check against the newest Horizon source the launcher has
(%APPDATA%/horizon-launcher/native/source/<commit>), with the launcher's crate
cache, and the newest prepared game files of that commit as the base. The first
build takes a few minutes. `check(mods)` is what tools/survey.py calls.
"""

import os
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
CRATE = ROOT / "tools" / "horizon-check"
TARGET = ROOT / "target-horizon-check"
APPDATA = Path(os.environ.get("APPDATA", Path.home() / ".local" / "share"))
LAUNCHER = APPDATA / "horizon-launcher"
EXE = TARGET / "debug" / ("horizon-check.exe" if os.name == "nt" else "horizon-check")


def source() -> Path:
    found = sorted((LAUNCHER / "native" / "source").glob("*/crates/horizon-client"), key=lambda p: p.stat().st_mtime)
    if not found:
        sys.exit("no Horizon source: build a native mod with the launcher once")
    return found[-1].parent.parent


def facts(commit: str) -> Path:
    found = sorted(
        (LAUNCHER / "versions").glob("*/.local/horizon-client/26.3/facts.json"),
        key=lambda p: (commit[:7] in str(p), p.stat().st_mtime),
    )
    if not found:
        sys.exit("no prepared game files: start an instance with the launcher once")
    return found[-1].parent


def build() -> Path:
    src = source()
    s = src.as_posix()
    toml = f"""# Written by tools/horizon_check.py for the Horizon source at {s}.
[package]
name = "horizon-check"
version = "0.0.0"
edition = "2024"
publish = false

[dependencies]
horizon-client = {{ path = "{s}/crates/horizon-client" }}
horizon-client-render = {{ path = "{s}/crates/horizon-client-render" }}
horizon-core = {{ path = "{s}/crates/horizon-core" }}
horizon-mods = {{ path = "{s}/crates/horizon-mods" }}
horizon-world = {{ path = "{s}/crates/horizon-world" }}
serde_json = "1"

[workspace]

[patch.crates-io]
wgpu-hal = {{ path = "{s}/third_party/wgpu-hal" }}
naga = {{ path = "{s}/third_party/naga" }}
libmimalloc-sys = {{ path = "{s}/third_party/libmimalloc-sys" }}
"""
    path = CRATE / "Cargo.toml"
    if not path.exists() or path.read_text(encoding="utf-8") != toml:
        path.write_text(toml, encoding="utf-8")
        shutil.copy(src / "Cargo.lock", CRATE / "Cargo.lock")
    env = dict(os.environ, CARGO_TARGET_DIR=str(TARGET))
    r = subprocess.run(["cargo", "build", "--quiet"], cwd=CRATE, env=env, capture_output=True, text=True)
    if r.returncode != 0:
        sys.exit("horizon-check didn't build:\n" + r.stderr[-3000:])
    return src


def check(mods: list[Path], src: Path | None = None) -> tuple[bool, list[str]]:
    """(passed, its report lines) for these mods loaded together."""
    src = src or build()
    with tempfile.TemporaryDirectory(prefix="horizon-check-") as out:
        r = subprocess.run(
            [str(EXE), str(facts(src.name)), out, *map(str, mods)],
            capture_output=True,
            text=True,
            encoding="utf-8",
            errors="replace",
        )
    lines = (r.stdout + r.stderr).splitlines()
    return r.returncode == 0, lines


def main() -> None:
    if len(sys.argv) < 2:
        sys.exit(__doc__)
    ok, lines = check([Path(a) for a in sys.argv[1:]])
    print("\n".join(lines))
    sys.exit(0 if ok else 1)


if __name__ == "__main__":
    main()
