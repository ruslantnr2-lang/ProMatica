# Save tab

[Back to the overview](Home.md)

Selects a box in the world and saves it as a `.litematic` in the schematics folder. The selection shows as a white
frame, with corner 1 in red and corner 2 in blue. You can also set the corners with the [selection tool](Tool.md).

## Corners

There is one row for **Corner 1** and one for **Corner 2**. Below them, the tab shows the box's size and block count.

| Control | What it does | Command (`pos2` for corner 2) |
|---|---|---|
| x, y, z and **Set** | Puts the corner at the typed position | `/hm pos1 <x> <y> <z>` |
| **Look** | Puts it on the block you look at, up to 64 blocks away. If you look at nothing in range, it uses where you stand | `/hm pos1` |
| **Here** | Puts it on the block you stand in | `/hm pos1 here` |

## Saving

| Control | What it does | Command |
|---|---|---|
| **Name** | The file name, without its extension | — |
| **Save** | Starts saving. It is greyed out until both corners and a valid name are set | `/hm save <name>` |
| **Overwrite** (replaces Save) | When the file exists, the first click on Save only warns you. The second click overwrites it | `/hm save <name> overwrite` |
| **Cancel** (replaces Save while saving) | Stops the save. Nothing is written | `/hm save cancel` |
| **Clear selection** | Removes both corners | `/hm area clear` |
| **Open folder** | Opens the schematics folder | `/hm folder` only prints the path in chat |
| **Selection tool: ... · settings** | Opens the [Tool tab](Tool.md) | — |

`/hm area` prints the corners and size in chat, and `/hm save status` prints how far a save has got.

## How a save runs

The area is read chunk by chunk, a few per tick, so the game doesn't stall. A chunk can only be read while it is
loaded:

- Chunks that aren't loaded yet show as **orange frames**.
- The progress bar here and the HUD tell you how many are left and where the nearest one is.
- Fly there and the save goes on. The file is written when the last chunk is in.

## What is saved

| Saved | Not saved |
|---|---|
| Every block with its state | Chest and other container contents |
| | Sign texts and banner patterns |
| | Entities |

The file holds one region. The area selection itself isn't remembered when you leave the world.
