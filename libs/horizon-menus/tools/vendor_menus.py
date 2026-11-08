#!/usr/bin/env python3
"""Copies horizon-menus into a native mod's bundle.

    python <horizon-menus>/tools/vendor_menus.py <bundle folder> <mod id>
    python libs/horizon-menus/tools/vendor_menus.py mixins/horizonmatica horizonmatica

Writes <bundle>/menus/ (the crate, renamed `<mod>-menus`) and <bundle>/menus_game.rs (the game-side
module, using `<mod>_menus`). Each mod has its own copy: Horizon has no dependencies between
native mods, so a mod never needs another one installed. Add to the bundle's horizon-mixin.toml:

    [[crate]]
    id = "menus"
    path = "menus"

    [[module]]
    id = "menus"
    file = "crates/horizon-client/src/game.rs"
    name = "<mod>_menus"
    source = "menus_game.rs"

`--check` only says whether the copy is up to date (exit 1 when not).
"""
import argparse
import pathlib
import sys

# This repository: the library the copy is made from (a clone of its own, or a submodule).
LIB = pathlib.Path(__file__).resolve().parent.parent


def wanted(mod: str) -> dict[str, str]:
    files = {}
    cargo = (LIB / "core" / "Cargo.toml").read_text(encoding="utf-8")
    cargo = cargo.replace('name = "horizon-menus"', f'name = "{mod}-menus"', 1)
    files["menus/Cargo.toml"] = cargo
    for src in sorted((LIB / "core" / "src").rglob("*.rs")):
        rel = src.relative_to(LIB / "core").as_posix()
        text = src.read_text(encoding="utf-8").replace("horizon_menus::", f"{mod}_menus::")
        files[f"menus/{rel}"] = text
    game = (LIB / "game.rs").read_text(encoding="utf-8").replace("horizon_menus", f"{mod}_menus")
    files["menus_game.rs"] = game
    # The menu screen's hook helpers (their [[injection]]s: hooks/injections.toml, --toml prints it).
    for src in sorted((LIB / "hooks").glob("*.rs")):
        files[f"menus_{src.name}"] = src.read_text(encoding="utf-8").replace("MODID", mod)
    return files


def injections(mod: str) -> str:
    return (LIB / "hooks" / "injections.toml").read_text(encoding="utf-8").replace("MODID", mod)


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("bundle")
    ap.add_argument("mod")
    ap.add_argument("--check", action="store_true")
    ap.add_argument("--toml", action="store_true", help="print the screen's [[injection]]s for horizon-mixin.toml")
    a = ap.parse_args()
    if a.toml:
        print(injections(a.mod))
        return
    bundle = pathlib.Path(a.bundle).resolve()
    stale = []
    for rel, text in wanted(a.mod).items():
        path = bundle / rel
        if not path.exists() or path.read_text(encoding="utf-8") != text:
            stale.append(rel)
            if not a.check:
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_text(text, encoding="utf-8", newline="\n")
    if a.check:
        if stale:
            print(f"horizon-menus copy in {a.bundle} is out of date: {', '.join(stale)}"
                  f" (run tools/vendor_menus.py {a.bundle} {a.mod})")
            sys.exit(1)
        print("horizon-menus copy is up to date")
    else:
        print(f"horizon-menus -> {a.bundle}: {len(stale)} file(s) written")


if __name__ == "__main__":
    main()
