# Tool tab

[Back to the overview](Home.md)

The selection tool is an item (a stick by default). While it is in your main hand, its clicks select things instead of
breaking or using blocks. The top of the tab shows the item and whether the tool is ready.

## Which item

| Control | What it does | Command |
|---|---|---|
| Item icons | Makes that item the tool: stick, blaze rod, breeze rod, bone, feather, wooden axe, golden hoe or arrow | `/hm tool <item>` |
| **item id** and **Set** | Makes the typed item the tool, such as `minecraft:stick` | `/hm tool <item>` |
| **Held item** | Makes the item in your main hand the tool | — |

Choosing an item also turns the tool on.

## Settings

| Control | Default | What it does | Command |
|---|---|---|---|
| **Selection tool** | On | Off: the item breaks and uses blocks as usual | `/hm tool on\|off` |
| **Tool works on** | The area | Whether it works on the area selection or on placements | `/hm tool mode area\|placement` |
| **Reach** | 128 blocks | How far a click reaches, from 16 to 256 blocks | — |
| **Corner on** | Block hit | **Block in front** marks the air in front of the face, handy for selecting above a floor | — |
| **Stops at water/lava** | Off | On: water and lava stop the aim. Off: it goes through them to the ground | — |
| **Controls in the HUD** | On | Shows the controls below in the HUD while the tool is in hand | — |

## Mouse controls in area mode

| Input | Effect |
|---|---|
| Left click | Corner 1 on the block you look at |
| Right click | Corner 2 on the block you look at |
| Middle click | Picks the corner you look at. Again, or elsewhere: picks the whole box |
| Alt + wheel | Moves the picked corner or the box along the direction you look |
| Ctrl + wheel | Grows (up) or shrinks (down) the box on the side you look at |
| Shift | 5 blocks per wheel step instead of 1 |
| Ctrl + middle click | Switches to placement mode and back |

## Mouse controls in placement mode

| Input | Effect |
|---|---|
| Left or middle click | Selects the placement you look at (its box) |
| Right click | Moves the selected placement's origin to the block you look at |
| Alt + wheel | Moves it along the direction you look (Shift: 5 blocks) |
| Ctrl + wheel | Turns it (up: clockwise) |
| Ctrl + middle click | Switches to area mode and back |

A [locked](Placement.md#show-lock-unload) placement can't be moved or turned with the tool.
