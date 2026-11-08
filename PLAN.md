# Promatica — Compatibility Plan

## Goal

Maintain an independent, Litematica-style native mod for Horizon and target the newest Horizon Nightly without requiring a local build on the maintainer's low-spec PC.

## Research baseline (8 November 2026)

- Upstream project: https://github.com/keksi2k/horizonmatica
- Upstream source release: Horizonmatica 0.3.0 (8 November 2026).
- Upstream 0.3.0 is pinned to Horizon Nightly 2026-11-04, commit 67dad0d1a5adca139b426c8114e74a33b376ba64, and hook API 359. It is not already compatible with the current target.
- Target Horizon: Nightly 2026-11-08, commit fdc87aecdbd116bf1e08adcd062fcbaea8c2e2ca.
- Target Horizon Launcher: 0.2.55.
- Launcher 0.2.55 uses horizon-mixins revision 5a64af938ec86b00d53d16a408aabc0038fb3e57. Upstream Horizonmatica's packaging script currently names an older revision; this must be reconciled with the target Launcher.
- Horizon's native hook API increased from 359 to 411 between the upstream mod's pinned revision and the target Nightly.

## Compatibility findings

Promatica cannot safely rely on the hook API number alone. The mod uses both named hooks and private/raw source targets:

- Named hooks: tick, frame, HUD, key, wheel, command, commands, mod list, settings, settings click, world-drawn and render-needs.
- Raw targets: Game::play_mouse, chat::Game::chat_tree_for, multiple debug_options::Game methods, and Game::pauses_game.
- It also adds modules directly to Horizon's game and renderer source files.

Because raw targets and internal game structures are used, the target build must keep an exact Horizon revision pin. The target is the full commit above, not merely the fdc87ae abbreviation.

## Work plan

### Phase 1 — Establish the fork

- Copy the upstream source and its horizon-menus submodule into this repository.
- Preserve upstream MIT license notices, credits and the statement that Promatica is an independent project.
- Keep the upstream project as a clearly documented source/credit reference; do not push changes to the upstream repository.

### Phase 2 — Port the compatibility layer

- Update the Horizon revision pin to the target Nightly.
- Align the packaging tool's horizon-mixins revision with the target Launcher.
- Audit every injection target, module insertion point, helper binding and renderer type against the target Horizon source.
- Pay particular attention to debug_options.rs, which changed materially, and to the native hook catalog/API.
- Fix only demonstrated API/compile errors. Do not remove functionality to make the build pass.
- Preserve schematic formats, placements, transforms, overlay, verifier, layers, materials, selection/save, Easy Place, printer, menu and key bindings.

### Phase 3 — Automated tests

- Run the upstream workspace tests: cargo test --workspace.
- Add/retain format and lint checks where they are supported by the repository.
- Validate the generated package manifest: mod ID, version, game version, exact Horizon commit and horizon-mixins revision.
- Test the actual client-native build on a GitHub-hosted Linux x86-64 runner using the official Horizon Launcher CLI.
- The Launcher requires a prepared base Horizon version for native builds. The workflow must install and prepare the target Nightly on the runner; a packaging-only job is not proof of compatibility.
- Do not treat a headless/server build as a substitute for the client build, because Promatica includes client rendering and UI integrations.

### Phase 4 — Package and publish

- Generate a .hnmod package only after the source checks and client-native build pass.
- Publish the package and build logs as GitHub Actions artifacts / a GitHub Release.
- Signing is optional and should only be enabled with a private signing key stored as a GitHub Actions secret. Never commit a private key.

### Phase 5 — In-game acceptance

Test on the target Nightly:
- Load .litematic, .schem, .schematic and .nbt examples.
- Check ghost rendering, visibility, layers, rotations and mirroring.
- Check verifier colours, progress HUD and material list.
- Test selection and saving.
- Test manual/Easy Place and the printer in a private test world.
- Confirm there are no crashes when entering/leaving worlds or opening/closing the menu.

Automatic placement may violate server rules or trigger anti-cheat; test printer features only where permitted.

## Definition of done

Promatica is called compatible with the target Nightly only when:
1. workspace tests pass;
2. the package manifest carries the exact target pins;
3. the official Launcher client-native build succeeds on GitHub Actions;
4. the resulting package installs and the in-game acceptance checklist passes.

A successful package step alone is not a compatibility claim.
