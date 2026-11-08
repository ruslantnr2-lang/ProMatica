# Commands

[Back to the overview](Home.md)

All commands start with `/promatica` or `/hm`. `/hm help` lists them in game. A switch without `on` or `off`
toggles.

The chat completes the commands: type `/hm ` and it offers the subcommands, narrowing them down as you type. After
`load` it offers your schematic files, and after `select` and `unload` your placements. **Tab** takes a suggestion.

## Files

| Command | Effect | In the menu |
|---|---|---|
| `/hm list` | Lists the schematics in the folder | [Load](Load.md): file list |
| `/hm load <name>` | Loads one as another placement at your position and selects it | Load: **Load here** |
| `/hm reload` | Reads the last schematic again from its file, where it was | Load: **Reload last** |
| `/hm folder` | Prints the schematics folder's path | Load: **Open folder** opens it |

## Placements

| Command | Effect | In the menu |
|---|---|---|
| `/hm placements` | Lists the placements; `>` marks the selected one | [Placement](Placement.md): list |
| `/hm select <n>\|<name>\|next\|prev` | Selects a placement | Placement: list row |
| `/hm show [on\|off]`, `/hm hide` | Shows or hides the selected placement | Placement: **Show** / **Hide** |
| `/hm lock [on\|off]`, `/hm unlock` | Locks it where it is | Placement: **Lock** / **Unlock** |
| `/hm unload [<n>\|all]` | Unloads the selected one, another one, or all | Placement: **Unload**, **Unload all** |
| `/hm origin here` | Moves it to your position | Placement: **Move to me** |
| `/hm origin <x> <y> <z>` | Moves it to a position | Placement: **Origin** and **Set** |
| `/hm move <dx> <dy> <dz>` | Moves it by a distance | Placement: **-X +X -Y +Y -Z +Z** |
| `/hm rotate cw\|ccw\|180\|none` | Turns it | Placement: **Rotate left**, **Rotate right** |
| `/hm mirror none\|x\|z` | Mirrors it | Placement: **Mirror** |
| `/hm info` | Prints position, progress and errors | Placement: top lines |

## Layers

| Command | Effect | In the menu |
|---|---|---|
| `/hm layer all` | Shows all layers | Placement: **All layers** |
| `/hm layer up`, `/hm layer down` | Moves the shown layer | Placement: **Layer up**, **Layer down** |
| `/hm layer <y>`, `/hm layer here` | One layer: a height, or where you stand | — |
| `/hm layer above <y>`, `/hm layer below <y>` | Everything above or below a height | — |
| `/hm layer range <a> <b>` | A range of heights | — |

## Area selection and saving

| Command | Effect | In the menu |
|---|---|---|
| `/hm pos1`, `/hm pos2` | Sets a corner on the block you look at | [Save](Save.md): **Look** |
| `/hm pos1 here`, `/hm pos2 here` | Sets a corner where you stand | Save: **Here** |
| `/hm pos1 <x> <y> <z>`, `/hm pos2 <x> <y> <z>` | Sets a corner at a position | Save: fields and **Set** |
| `/hm area` | Prints the selection's corners and size | Save: shown in the tab |
| `/hm area clear` | Clears the selection | Save: **Clear selection** |
| `/hm save <name> [overwrite]` | Saves the area as `<name>.litematic` | Save: **Save**, **Overwrite** |
| `/hm save status` | Prints how far the save has got | Save: progress bar |
| `/hm save cancel` | Stops the save | Save: **Cancel** |

## Selection tool

| Command | Effect | In the menu |
|---|---|---|
| `/hm tool [on\|off]` | Turns the tool on or off. Without a word it only tells its state | [Tool](Tool.md): **Selection tool** |
| `/hm tool <item>` | Makes an item the tool and turns it on | Tool: item icons, **item id** |
| `/hm tool mode [area\|placement]` | What it works on. Without a word it switches, and it also turns the tool on | Tool: **Tool works on** |

## Display

| Command | Effect | In the menu |
|---|---|---|
| `/hm render [on\|off]` | Draws the overlay or not | [Display](Display.md): **Rendering** |
| `/hm through [on\|off]` | Draws it through blocks | Display: **See through blocks** |
| `/hm highlight missing\|wrong [on\|off]` | Marks missing or wrong blocks through everything | Display or [Materials](Materials.md#highlighting-blocks) |
| `/hm highlight item <item>\|all` | Marks only that item, or all again | Materials: click a row |
| `/hm highlight off` | Switches the highlights off | — |
| `/hm mode [ghost\|boxes]` | Ghost blocks or boxes. Without a word it switches | Display: **Mode** |
| `/hm radius <blocks>\|view` | How far the overlay reaches | Display: **Render radius**, **Radius = view distance** |
| `/hm faces <n>\|none` | The most faces it keeps, such as `300k` or `2m` | Display: **Face limit** |
| `/hm fluids [on\|off]` | Shows or hides water and lava | Display: **Hide water and lava** |

## Printer

| Command | Effect | In the menu |
|---|---|---|
| `/hm printer [on\|off]` | Starts or stops the printer | [Printer](Printer.md): **Printer** |
| `/hm printer speed <ticks>` | One block every 1 to 20 ticks | Printer: **One block every** |
| `/hm printer reach <blocks>` | Reach, from 1 to 6 blocks | Printer: **Reach** |
| `/hm printer air [on\|off]` | Placing without a neighbouring block | Printer: **Air place** |

## Easy place

| Command | Effect | In the menu |
|---|---|---|
| `/hm easyplace [on\|off]` | Easy place on or off | [Placement](Placement.md#easy-place): **Easy place** |
| `/hm easyplace every <ticks>` | One block every 1 to 10 ticks while the button is held | Placement: **Easy place every** |
| `/hm easyplace air [on\|off]` | Ghost blocks without a neighbouring block | Placement: **Easy air place** |

## Other

| Command | Effect | In the menu |
|---|---|---|
| `/hm materials [on\|off]` | The material list in the HUD | [Materials](Materials.md): **HUD list** |
| `/hm materials copy` | Copies the material list to the clipboard as a table | Materials: **Copy** |
| `/hm credits` | Prints who made the originals | **Credits** button: **Show the credits in chat** |
| `/hm help` | Lists the commands | — |
