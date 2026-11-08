# Printer tab

[Back to the overview](Home.md)

The printer places missing blocks of the shown placements within reach, from the bottom up and only in the layers
shown. It needs a loaded schematic, and it is off each time you start the game.

> **On other people's servers, automatic placing may break the rules or trip anti-cheat.**

| Control | Default | What it does | Command |
|---|---|---|---|
| **Printer: ON/OFF** | Off | Starts or stops building | `/hm printer [on\|off]`, key **K** |
| **One block every** | 3 ticks | Speed, from 1 to 20 ticks per block. Lower is faster | `/hm printer speed <ticks>` |
| **Reach** | 4.5 blocks | How far it places, from 1 to 6 blocks | `/hm printer reach <blocks>` |
| **Air place** | Off | When no neighbouring block can be clicked, it clicks the empty spot itself. Anti-cheat plugins notice this. Blocks with a real neighbour still come first | `/hm printer air [on\|off]` |

The tab also shows **Placed this session**, and a note on what the printer is doing or why it is waiting.

## Good to know

- **Survival:** the blocks must be in your hotbar. If an item is only in your inventory, the HUD says so.
- **Creative:** only the hotbar is used. The rest of the inventory stays as it is.
- **Order:** plain blocks first, then what carries or uses a signal, then what makes one, and observers last.
  Within that it goes from the bottom up, in each layer what you already hold first, nearest first.
- **Falling blocks** are only placed onto something. If they float in the schematic, the printer leaves them out.
- **Without air place,** only blocks next to a real block whose face you can see are tried. The rest are passed over
  at once until that changes, and the note in this tab counts them.
- **Logs, froglights and other pillars** get their direction from the clicked face alone. Without air place, a pillar
  with `axis=y` needs a real block above or below it, and `axis=x` or `axis=z` one on that side, with that face in view.
  So a pillar that should stand upright can't go on a block below it while your eyes are lower than that block's top.
  The note in this tab names such a block.
- **Plants** are placed by the item that grows them: seeds for wheat, bamboo for shoots and stalks. They count as correct
  at any stage of growth, so a bamboo shoot is fine where the schematic has a tall stalk with leaves.
- **Blocks it can't place as wanted from where you stand,** such as stairs that would face the wrong way, are not tried
  again until you move or a block next to them changes.
- **Fluids** are not placed. Blocks that can't be placed correctly from any reachable side are skipped, and the HUD
  says why.
- The printer pauses while [move mode](Keys.md#move-mode) is on. Hidden placements aren't printed.
