#!/usr/bin/env python3
"""Packs the bundle into promatica-<version>.hnmod (a zip: horizon-native.json + mixins/promatica/...).

    python tools/package.py [--horizon <commit>] [--sign]

--horizon overrides the pinned commit (default: [compatibility] horizon_revision of horizon-mixin.toml).
--sign signs the package with minisign (default secret key ~/.minisign/minisign.key, asks for its password)
and verifies the signature against minisign.pub, writing <package>.minisig next to it.
"""
import argparse
import hashlib
import json
import pathlib
import re
import zipfile

ROOT = pathlib.Path(__file__).resolve().parent.parent
BUNDLE = ROOT / "mixins" / "promatica"
MIXINS_REV = "5a64af938ec86b00d53d16a408aabc0038fb3e57"  # launcher's native_mods::MIXINS_REV
ALLOWED_TEXT = ("README", "LICENSE", "COPYING", "NOTICE", ".gitignore")


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--horizon", help="Horizon commit to pin (40 hex chars)")
    ap.add_argument("--sign", action="store_true", help="sign the package with minisign")
    args = ap.parse_args()
    # The bundle's copy of horizon-menus must match the library (the libs/horizon-menus submodule).
    import subprocess
    import sys
    vendor = ROOT / "libs" / "horizon-menus" / "tools" / "vendor_menus.py"
    if vendor.exists():
        subprocess.run([sys.executable, str(vendor), str(BUNDLE), "promatica", "--check"], check=True)
    else:
        print("libs/horizon-menus is missing (git submodule update --init): the menu copy is not checked")
    toml = (BUNDLE / "horizon-mixin.toml").read_text(encoding="utf-8")
    version = re.search(r'^version\s*=\s*"([^"]+)"', toml, re.M).group(1)
    pinned = re.search(r'^horizon_revision\s*=\s*"([0-9a-f]{40})"', toml, re.M).group(1)
    horizon = args.horizon or pinned
    if not re.fullmatch(r"[0-9a-f]{40}", horizon):
        raise SystemExit("--horizon must be a 40-character commit hash")
    if horizon != pinned:
        toml = toml.replace(pinned, horizon)

    manifest = {"schema": 1, "id": "promatica", "version": version, "game": "26.3",
                "horizon": horizon, "mixins": MIXINS_REV}
    out = ROOT / "dist" / f"promatica-{version}.hnmod"
    out.parent.mkdir(exist_ok=True)
    # Deterministic, like horizon-mods' native.py: sorted names, fixed time and mode, the bytes as
    # committed, so anyone can rebuild the file from the tag and get the same SHA-256.
    entries = {"horizon-native.json": (json.dumps(manifest, indent=2) + "\n").encode()}
    for path in sorted(BUNDLE.rglob("*")):
        if not path.is_file() or "target" in path.relative_to(BUNDLE).parts:
            continue
        rel = path.relative_to(BUNDLE)
        if path.suffix not in (".rs", ".toml") and not path.name.startswith(ALLOWED_TEXT) \
                and "data" not in rel.parts:
            continue
        arc = f"mixins/promatica/{rel.as_posix()}"
        entries[arc] = toml.encode() if rel.as_posix() == "horizon-mixin.toml" else path.read_bytes()
    with zipfile.ZipFile(out, "w", zipfile.ZIP_DEFLATED) as z:
        for name, content in sorted(entries.items()):
            info = zipfile.ZipInfo(name, (2026, 1, 1, 0, 0, 0))
            info.compress_type = zipfile.ZIP_DEFLATED
            info.external_attr = 0o100644 << 16
            z.writestr(info, content)
    digest = hashlib.sha256(out.read_bytes()).hexdigest()
    print(f"wrote {out} ({len(entries) - 1} files, Horizon {horizon[:10]})\nsha256 {digest}")
    if args.sign:
        sig = out.with_name(out.name + ".minisig")
        sig.unlink(missing_ok=True)  # never leave a stale signature next to a new package
        subprocess.run(["minisign", "-Sm", str(out)], check=True)
        subprocess.run(["minisign", "-Vm", str(out), "-p", str(ROOT / "minisign.pub")], check=True)
        print(f"wrote {sig}")


if __name__ == "__main__":
    main()
