# Keys

[Back to the overview](Home.md)

You can bind every action to a key on the [Keys tab](#keys-tab) of the menu. Keys you changed in Options › Mods before
0.3.0 are kept as the starting point.

The defaults:

| Key | Action | Command |
|---|---|---|
| **M** | Opens the menu | — |
| **V** | Rendering on or off | `/hm render` |
| **Page Up** / **Page Down** | Layer up / down | `/hm layer up`, `/hm layer down` |
| **Home** | Next layer mode: all, this layer, above, below | — |
| **K** | Printer on or off | `/hm printer` |
| **J** | Material list in the HUD on or off | `/hm materials` |
| **N** | Move mode on or off | — |
| **Middle mouse button** | Takes the block the schematic wants where you look, see below | — |

No default key: selection tool, easy place, reload, rotate, mirror, move to me, show or hide, select next or previous
placement, unload, area corners and clear, copy material list, open schematics folder. Bind them on the Keys page.

## In the menu

| Key | Action |
|---|---|
| **1** to **8** | Switch tabs (while no edit box is being typed in) |
| **M**, **Esc** | Close the menu |

## Keys tab

Click the key next to an action, then press the new key. **Backspace** or **Delete** clears it, and a second click on
the button cancels. **Esc** closes the menu, which cancels too. A key that was already in use moves to the new action.
**Reset all keys** brings back the defaults.

Only keyboard keys can be bound. Things that need a file or a name, such as loading a schematic or `/hm save <name>`,
have no key. In the menu itself **M** and **1** to **8** keep their meaning whatever is bound.

## Move mode

Only in this mode do the mouse and keys move the selected placement, so nothing gets moved by accident. Turn it on with
**N** or the **Move mode** button in the [Placement tab](Placement.md). A locked placement can't enter it.

| Input | Effect |
|---|---|
| Mouse wheel | Along your view (the closest axis, up and down too) |
| Arrow keys | Forward, back, left, right, relative to your view |
| Page Up / Page Down | Up / down, instead of changing layers |
| R | Rotate clockwise |
| Home | Move it to your position |
| Shift held | 5 blocks instead of 1 |
| N or Enter | Done |

The printer pauses while move mode is on.

## Picking a block from the schematic

Middle-click a ghost block to take that block, as the game's own pick block does: from the hotbar, from your inventory
into the hotbar, or in creative out of nothing. This makes building by hand easier. It works within your normal reach,
on any spot that is still missing or wrong, in the layers shown.

Everywhere else the middle button stays the game's own pick block, also with rendering off (**V**). With the selection
tool in hand, the middle button belongs to the tool.

## Selection tool

The mouse controls are listed in the [Tool tab](Tool.md).
