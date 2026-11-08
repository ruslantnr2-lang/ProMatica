# Materials tab

[Back to the overview](Home.md)

Lists the items the **selected** placement needs. Select another placement in the [Placement tab](Placement.md) to see
its list.

| Control | What it does | Command |
|---|---|---|
| **Copy** | Copies the list to the clipboard as a table | `/hm materials copy` |
| **HUD list: ON/OFF** | The list in the corner while playing | `/hm materials [on\|off]`, key **J** |

## Highlighting blocks

The row below the buttons marks blocks so that you can see them through everything, however far away they are.

| Control | What it does | Command |
|---|---|---|
| **Missing: ON/OFF** | Marks missing blocks in blue | `/hm highlight missing [on\|off]` |
| **Wrong: ON/OFF** | Marks wrong blocks in red and wrong states in orange | `/hm highlight wrong [on\|off]` |
| A row of the list | Marks only that item. Click it again to mark all items | `/hm highlight item <item>` |
| The button with the item's name | Marks all items again | `/hm highlight item all` |

- `/hm highlight off` switches everything off.
- The filter goes by the item that belongs at that spot. With **Stone** picked, **Wrong** marks spots where stone should
  be but another block is.
- Clicking a row switches **Missing** on if neither is on.
- All shown placements are marked, in the layers shown. Blocks in chunks that aren't loaded can't be checked.
- Extra blocks aren't marked. Rendering off (**V**) hides the highlights too.
- At most 50,000 blocks are marked, the nearest ones. The HUD then says "the nearest only"; picking an item helps.
- The switches are remembered. The picked item lasts until you close the game.

## Copying to a spreadsheet

After **Copy**, paste into Excel or another spreadsheet with **Ctrl+V**, and the list lands in separate columns. The
copy has a header row and these columns: Item, Missing, Total, Carried, To fetch and Item ID.

## The columns

| Column | Meaning |
|---|---|
| **Item** | The item, with its icon. Hover over a row for its full id and how many are still to fetch |
| **Missing** | How many are still to be placed. Green at 0 |
| **Total** | How many the whole placement needs |
| **Carried** | How many you have with you. Green once you have enough, orange until then |

The line at the top shows the number of kinds of item and how many items are still to fetch.
