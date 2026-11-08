# Load tab

[Back to the overview](Home.md)

Schematics live in the `schematics/` folder of your Horizon instance, next to `options.txt`. `.litematic`, `.schem`,
`.schematic` and `.nbt` files are read.

| Control | What it does | Command |
|---|---|---|
| **Search...** | Filters the file list by name | — |
| File list | One click picks a file and shows its details on the right. A second click on the picked file loads it | `/hm list` lists the files in chat |
| **Load here** (only with a file picked) | Loads the file as one more placement at your feet, selects it, and switches to the Placement tab | `/hm load <name>` |
| **Reload last** | Reads the last schematic again from its file, in the place it was. Use it after changing the file | `/hm reload` |
| **Open folder** | Opens the schematics folder in your file manager | `/hm folder` only prints the path in chat |
| **Refresh** | Lists the folder and reads the file details again. The list also refreshes by itself every few seconds | — |

## The details of a picked file

Name, size (x × y × z), number of blocks, number of regions, format and author.

## Good to know

- With `/hm load`, the name without its extension, or just its start, is enough. When two files share a name, the
  `.litematic` wins.
- Large files are read in the background, and the HUD shows "Loading ...". The game keeps running meanwhile.
- Older files are upgraded to the current block names when they are loaded.
