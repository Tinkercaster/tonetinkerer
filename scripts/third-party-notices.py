#!/usr/bin/env python3
"""Generate THIRD_PARTY_NOTICES.md and THIRD_PARTY_LICENSES.txt from the
crates compiled into the plugin (`cargo metadata`).

    scripts/third-party-notices.py [--target TRIPLE ...] [--out DIR]

With no --target, the union of the release platforms (macOS, Windows, Linux)
is used: that is the copy kept in the repository. The release workflow runs
it per platform so every download lists exactly what it contains.
"""

import argparse
import json
import pathlib
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
PLATFORMS = [
    "aarch64-apple-darwin",
    "x86_64-apple-darwin",
    "x86_64-pc-windows-msvc",
    "x86_64-unknown-linux-gnu",
]
MIT_TEXT = """Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE."""
LICENSE_HINTS = ("license", "licence", "copying", "notice", "unlicense", "ofl", "ufl")
EXTRA = [
    ("Inter (font, bundled)", "OFL-1.1", ROOT / "assets/fonts/Inter-LICENSE.txt"),
    ("Jester Dyne Productions cabinet IRs (bundled)", "CC0-1.0", ROOT / "assets/irs/JESTER-IRS-LICENSE.txt"),
]


def closure(target):
    meta = json.loads(
        subprocess.check_output(
            ["cargo", "metadata", "--format-version", "1", "--filter-platform", target],
            cwd=ROOT,
        )
    )
    pkgs = {p["id"]: p for p in meta["packages"]}
    nodes = {n["id"]: n for n in meta["resolve"]["nodes"]}
    start = next(p["id"] for p in meta["packages"] if p["name"] == "tonetinkerer-plugin")
    seen, todo = set(), [start]
    while todo:
        pid = todo.pop()
        if pid in seen:
            continue
        seen.add(pid)
        for dep in nodes[pid]["deps"]:
            # Normal and build dependencies end up in (or produce) the binary;
            # dev-dependencies (tests only) do not.
            if any(k["kind"] in (None, "build") for k in dep["dep_kinds"]):
                todo.append(dep["pkg"])
    members = set(meta["workspace_members"])
    return [pkgs[i] for i in seen if i not in members]


def licence_files(pkg):
    root = pathlib.Path(pkg["manifest_path"]).parent
    found = []
    for path in sorted(root.rglob("*")):
        rel = path.relative_to(root)
        if len(rel.parts) > 2 or not path.is_file() or "target" in rel.parts:
            continue
        name = path.name.lower()
        if any(h in name for h in LICENSE_HINTS) and path.suffix.lower() not in (".rs", ".toml", ".json"):
            found.append(path)
    return found


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--target", action="append")
    ap.add_argument("--out", default=str(ROOT))
    args = ap.parse_args()
    targets = args.target or PLATFORMS
    crates = {}
    for t in targets:
        for p in closure(t):
            crates[(p["name"], p["version"])] = p
    rows = sorted(crates.values(), key=lambda p: (p["name"].lower(), p["version"]))
    copyleft_only = [
        p for p in rows
        if (p.get("license") or "") and "GPL" in p["license"] and " OR " not in p["license"]
    ]
    if copyleft_only:
        sys.exit("copyleft-only dependency: " + ", ".join(p["name"] for p in copyleft_only))

    out = pathlib.Path(args.out)
    platforms = ", ".join(targets)
    notices = [
        "# Third-party notices",
        "",
        "ToneTinkerer is built on open-source software. This list covers every crate",
        f"compiled into the plugin for: {platforms} (generated from `cargo metadata`",
        "by `scripts/third-party-notices.py`). None is under a copyleft-only licence.",
        "The full licence texts are in THIRD_PARTY_LICENSES.txt, shipped with every",
        "download.",
        "",
        "Notable components: [nice-plug](https://codeberg.org/RustAudio/nice-plug)",
        "(ISC), [nam-rs](https://crates.io/crates/nam-rs) (Neural Amp Modeler",
        "inference), [egui](https://github.com/emilk/egui) (MIT OR Apache-2.0, includes",
        "fonts under OFL-1.1 / Ubuntu Font Licence), the `vst3` bindings, and",
        "[fft-convolver](https://crates.io/crates/fft-convolver).",
        "",
        "| Crate | Version | Licence |",
        "|---|---|---|",
    ]
    for p in rows:
        notices.append(f"| {p['name']} | {p['version']} | {p.get('license') or 'see licence file'} |")
    notices += ["", "## Fonts", "", "| Font | Licence |", "|---|---|"]
    notices += [f"| {name} | {lic} |" for name, lic, _ in EXTRA]
    (out / "THIRD_PARTY_NOTICES.md").write_text("\n".join(notices) + "\n", encoding="utf-8")

    bar = "=" * 78
    texts = [
        "ToneTinkerer - third-party licence texts",
        f"Generated from the crate sources compiled into the plugin ({platforms}).",
        "",
    ]
    for p in rows:
        texts += [bar, f"{p['name']} {p['version']}  ({p.get('license') or 'see below'})", bar]
        files = licence_files(p)
        if not files:
            texts.append(f"Licence: {p.get('license')} (the crate ships no licence file; "
                         "see the standard texts at the end).")
        for f in files:
            texts += [f"--- {f.name}", f.read_text(encoding="utf-8", errors="replace").rstrip(), ""]
        texts.append("")
    for name, lic, path in EXTRA:
        texts += [bar, f"{name}  ({lic})", bar, path.read_text(encoding="utf-8").rstrip(), ""]
    # Standard texts for crates that name a licence without shipping its file.
    apache = next(
        (f.read_text(encoding="utf-8", errors="replace") for p in rows for f in licence_files(p)
         if "apache" in f.name.lower() and "Version 2.0" in f.read_text(encoding="utf-8", errors="replace")),
        None,
    )
    texts += [bar, "Standard licence texts (for crates above that ship no licence file)", bar,
              "--- MIT", MIT_TEXT, ""]
    if apache:
        texts += ["--- Apache-2.0", apache.rstrip(), ""]
    (out / "THIRD_PARTY_LICENSES.txt").write_text("\n".join(texts) + "\n", encoding="utf-8")
    print(f"{len(rows)} crates -> {out}/THIRD_PARTY_NOTICES.md, THIRD_PARTY_LICENSES.txt")


if __name__ == "__main__":
    main()
