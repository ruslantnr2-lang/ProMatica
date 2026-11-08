# Placement tab

[Back to the overview](Home.md)

With nothing loaded, this tab only offers **Load one**, which opens the [Load tab](Load.md).

## Left side: all placements

| Control | What it does | Command |
|---|---|---|
| List row | Selects that placement. A row shows the number, name and origin, and whether it is hidden or locked | `/hm select <n>` or `<name>`, `next`, `prev`; `/hm placements` lists them in chat |
| **Load more** | Opens the [Load tab](Load.md) to add another placement | — |
| **Unload all** | Removes every placement and forgets them for this world and dimension | `/hm unload all` |

## Right side: the selected placement

The top lines show its size, rotation, mirror and progress: percent done, and how many blocks are missing, wrong or
extra. `/hm info` prints the same in chat.

### Show, lock, unload

| Button | What it does | Command |
|---|---|---|
| **Hide** / **Show** | A hidden placement stays loaded and remembered, but isn't drawn or printed | `/hm show [on\|off]`, `/hm hide` |
| **Lock** / **Unlock** | A locked placement can't be moved, turned or mirrored, by any means. The lock is remembered | `/hm lock [on\|off]`, `/hm unlock` |
| **Unload** | Removes this placement. The blocks in the world stay as they are | `/hm unload [<n>]` |

### Position

The buttons in this section and the next are greyed out while the placement is locked.

| Control | What it does | Command |
|---|---|---|
| **Origin** x, y, z and **Set** | Moves the origin to the typed position. Enter works too while typing | `/hm origin <x> <y> <z>` |
| **-X +X -Y +Y -Z +Z** | Moves it by 1 block, or by 10 with Shift held | `/hm move <dx> <dy> <dz>` |
| **Move to me** | Moves the origin to where you stand | `/hm origin here` |
| **Move mode: ON/OFF** | Closes the menu and lets the wheel and keys move the placement in the world. See [Keys](Keys.md#move-mode) | Key **N** |

### Rotation and mirror

| Button | What it does | Command |
|---|---|---|
| **Rotate left** | 90° counter-clockwise | `/hm rotate ccw` |
| **Rotate right** | 90° clockwise | `/hm rotate cw` |
| **Mirror: ...** | Steps through none, x (front-back) and z (left-right) | `/hm mirror none\|x\|z` |

`/hm rotate 180` and `/hm rotate none` set a rotation directly.

### Layers

Layers limit what is drawn and printed. They apply to all placements, and locking doesn't affect them.

| Button | What it does | Command |
|---|---|---|
| **Layers: ...** | Steps through all, this layer, above, and below | Key **Home** |
| **Layer down** | Moves the shown layer down by one | `/hm layer down`, key **Page Down** |
| **Layer up** | Moves the shown layer up by one | `/hm layer up`, key **Page Up** |
| **All layers** | Shows everything again | `/hm layer all` |

Only with a command: a certain height (`/hm layer <y>`), the height you stand at (`/hm layer here`), everything above
or below a height (`/hm layer above <y>`, `/hm layer below <y>`), and a range (`/hm layer range <a> <b>`).

### Selection tool

| Button | What it does | Command |
|---|---|---|
| **Selection tool: area/placement mode** | Switches what the [selection tool](Tool.md) works on | `/hm tool mode area\|placement` |

## Good to know

- Where two placements overlap, the later one's blocks win. Its air doesn't erase the other's blocks.
- Placements are remembered per world and dimension, and come back when you join again.
- Every shown placement has a frame: faint normally, bright when selected. A selected frame is cyan, or orange when
  locked. **Boxes around placements** in the [Display tab](Display.md) turns the frames off.

## Easy place

Easy place is building by hand with the schematic as a guard. Turn it on with **Easy place** at the bottom left of this
tab, `/hm easyplace`, or a key of your choice on the [Keys tab](Keys.md#keys-tab).

Look at a ghost block: the block it needs is taken in hand by itself, from the hotbar, from your inventory, or in
creative out of nothing. Hold the **right mouse button** and the mod places it, facing the right way, and goes on with
the next ghost block you look at. It always takes the first ghost block along your view, so nothing lands behind it.

While it is on, **only the schematic's blocks are placed**, also while you hold the button and look around. Anywhere else
a block in hand does nothing. The HUD shows **Easy place ON**, and why a block can't go in right now, such as a stair that
would face the wrong way from where you stand, or a ghost block with no real block next to it.

| Control | Default | What it does | Command |
|---|---|---|---|
| **Easy place** | Off | On or off | `/hm easyplace [on\|off]` |
| **Easy place every** | 2 ticks | Time between blocks while you hold the button, 1 to 10 ticks | `/hm easyplace every <ticks>` |
| **Easy air place** | Off | Ghost blocks with no real neighbour can be placed too: the mod clicks the empty spot itself. Anti-cheat plugins notice this. Separate from the printer's air place | `/hm easyplace air [on\|off]` |

- Everything that doesn't place a block stays as it is: opening a chest, using a lever. Hold **Shift** to click against
  such a block instead. To eat, look away from the ghost blocks: while you look at one, the hand holds what it needs.
- With no placement shown, or in move mode, the game places blocks as usual. While the printer is on, the hand is the
  printer's.
- A ghost block needs a real neighbour to click against, unless **Easy air place** is on. It works on missing blocks
  only, in the placements and layers shown.
- With the selection tool in hand, the right button stays the tool's.
