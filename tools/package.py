#!/usr/bin/env python3
"""Build a deterministic Promatica .hnmod package.

Usage: python tools/package.py [--horizon <40-character commit>]
"""
import argparse
import hashlib
import json
import pathlib
import re
import subprocess
import sys
import zipfile

ROOT = pathlib.Path(__file__).resolve().parent.parent
MOD_ID = "promatica"
BUNDLE = ROOT / "mixins" / MOD_ID
MIXINS_REV = "5a64af938ec86b00d53d16a408aabc0038fb3e57"
ALLOWED_TEXT = ("README", "LICENSE", "COPYING", "NOTICE", ".gitignore")


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--horizon", help="override the pinned Horizon commit (40 hex chars)")
    args = ap.parse_args()

    vendor = ROOT / "libs" / "horizon-menus" / "tools" / "vendor_menus.py"
    if not vendor.exists():
        raise SystemExit("libs/horizon-menus is missing")
    subprocess.run([sys.executable, str(vendor), str(BUNDLE), MOD_ID, "--check"], check=True)

    toml_path = BUNDLE / "horizon-mixin.toml"
    toml = toml_path.read_text(encoding="utf-8")
    version = re.search(r'^version\\s*=\\s*"([^"]+)"', toml, re.M).group(1)
    pinned = re.search(r'^horizon_revision\\s*=\\s*"([0-9a-f]{40})"', toml, re.M).group(1)
    horizon = args.horizon or pinned
    if not re.fullmatch(r"[0-9a-f]{40}", horizon):
        raise SystemExit("--horizon must be a 40-character commit hash")
    if horizon != pinned:
        toml = toml.replace(pinned, horizon)

    manifest = {
        "schema": 1,
        "id": MOD_ID,
        "version": version,
        "game": "26.3",
        "horizon": horizon,
        "mixins": MIXINS_REV,
    }

    out = ROOT / "dist" / f"{MOD_ID}-{version}.hnmod"
    out.parent.mkdir(exist_ok=True)
    entries = {"horizon-native.json": (json.dumps(manifest, indent=2) + "\n").encode()}
    license_path = ROOT / "LICENSE"
    if not license_path.is_file():
        raise SystemExit("Root LICENSE file is missing")
    entries[f"mixins/{MOD_ID}/LICENSE"] = license_path.read_bytes()

    for path in sorted(BUNDLE.rglob("*")):
        if not path.is_file() or "target" in path.relative_to(BUNDLE).parts:
            continue
        rel = path.relative_to(BUNDLE)
        if path.suffix not in (".rs", ".toml") and not path.name.startswith(ALLOWED_TEXT) \\
                and "data" not in rel.parts:
            continue
        arc = f"mixins/{MOD_ID}/{rel.as_posix()}"
        entries[arc] = toml.encode() if rel.as_posix() == "horizon-mixin.toml" else path.read_bytes()

    with zipfile.ZipFile(out, "w", zipfile.ZIP_DEFLATED) as archive:
        for name, content in sorted(entries.items()):
            info = zipfile.ZipInfo(name, (2026, 1, 1, 0, 0, 0))
            info.compress_type = zipfile.ZIP_DEFLATED
            info.external_attr = 0o100644 << 16
            archive.writestr(info, content)

    digest = hashlib.sha256(out.read_bytes()).hexdigest()
    print(f"wrote {out} ({len(entries) - 1} bundle files; Horizon {horizon})")
    print(f"sha256 {digest}")


if __name__ == "__main__":
    main()
