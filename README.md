# Promatica

**Litematica-style schematic tools for Horizon (Minecraft rewritten in Rust).**

Promatica is an independent continuation/fork based on the source of [Horizonmatica 0.3.0](https://github.com/keksi2k/horizonmatica), with compatibility work for current Horizon Nightly versions.

> **Compatibility status:** targeted at Horizon Nightly 2026-11-08 (`fdc87aecdbd116bf1e08adcd062fcbaea8c2e2ca`). Compatibility is not considered confirmed until the GitHub Actions client-native build succeeds and the mod is tested in-game.

## Features

- Load `.litematic` (versions 1–7), Sponge `.schem` v1–v3, legacy `.schematic`, and vanilla structure `.nbt`.
- Render schematic ghost blocks using Horizon's block models and textures.
- Verify missing, incorrect, and extra blocks.
- Show layers, multiple placements, rotation, mirroring, and placement transforms.
- Track materials and progress.
- Select an area and save it as a `.litematic`.
- Easy Place and printer features.
- In-game menu, configurable key bindings, and commands.

## Building

The project uses Horizon's native mixin system. Native mods run with the permissions of the player, so inspect the source and only build code you trust.

The local package command is:

```sh
python tools/package.py
```

It writes `dist/promatica-0.1.0.hnmod`. The package records the exact Horizon revision and horizon-mixins revision.

The compatibility build is intended to run on GitHub Actions so contributors do not need to compile Horizon on a low-spec PC. See [CI workflow](.github/workflows/ci.yml) once available.

## Installing a successful build

Use the Promatica `.hnmod` artifact from the matching GitHub Actions run or GitHub Release. In Horizon Launcher, open **Mods**, choose the native mod install option, select the package, review its source and trust warning, then build it for the instance.

Do not install a package from a failed or incomplete workflow run.

## Upstream and credits

Promatica is based on [Horizonmatica](https://github.com/keksi2k/horizonmatica) by **keksi2k**. The original implementation's MIT license and attribution are retained. The bundled menu library [horizon-menus](https://github.com/keksi2k/horizon-menus) is also MIT licensed.

The schematic formats and user-facing behaviour are inspired by:
- [Litematica](https://github.com/maruohon/litematica) by masa, maintained by Sakura-Ryoko.
- [Litematica Printer](https://github.com/aleksilassila/litematica-printer) by aleksilassila.

Promatica is not affiliated with or endorsed by the Horizon, Litematica, Mojang, or Microsoft teams.

## Safety notes

- Printer and Easy Place send ordinary block-placement interactions to the server. Automatic placement may violate server rules or trigger anti-cheat. Use them only where permitted.
- Schematic files are parsed as data. Do not put untrusted files into the game if you do not trust their contents.
- Promatica has no telemetry or independent network service.

## Project status

The initial port is in progress. See [PLAN.md](PLAN.md) for the compatibility plan and acceptance criteria.
