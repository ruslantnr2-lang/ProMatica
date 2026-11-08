# horizon-menus

Menus and settings for [Horizon](https://github.com/horizon-rust/horizon) native mods.

Horizon gives every native mod a page in **Options > Mods**: a list of buttons, asked for again after each click.
horizon-menus turns that page into a real menu:

- **Settings with a schema**: toggles, choices and numbers (plain or percent) with defaults and ranges. Values are
  stored in Horizon's per-mod settings (`native_setting` / `native_set_setting`), read back with their defaults,
  bad values clamped or ignored.
- **Pages**: links to sub-pages, a `< Back` row and a title row on each sub-page.
- **Actions** your mod runs, **info** lines, and **lists** your mod fills when the page is drawn (files, status
  lines), paged when long.
- **A key opens the menu** straight from the game (like Horizon's own tweaks key): Esc goes back to playing.
- **Shift-click** steps numbers and choices backwards.

It is used by [Horizonmatica](https://github.com/keksi2k/horizonmatica), the first mod built with it.

## Why a copy per mod

Horizon has no dependencies between native mods yet: a mod's crate may only use crates from Horizon's
`Cargo.lock`, and the launcher has no "requires". A shared library mod would break every player's native build the
moment it is missing. So each mod carries **its own copy**, under its own name (`<mod>_menus`): no conflicts, no
dependencies, every mod works alone.

## Adding it to a mod

1. Copy it into your bundle (rerun after updating this library):

   ```
   git submodule add https://github.com/keksi2k/horizon-menus libs/horizon-menus
   python libs/horizon-menus/tools/vendor_menus.py mixins/<your mod folder> <your mod id>
   ```

   (Or download this repository anywhere and point the script at your bundle.)

   This writes `menus/` (the crate, named `<mod>-menus`), `menus_game.rs` (the game-side module) and the screen's
   hook helpers `menus_screen_*.rs`. `--toml` prints their `[[injection]]`s for your `horizon-mixin.toml`.

2. Add both to your `horizon-mixin.toml`:

   ```toml
   [[module]]
   id = "menus_game"
   file = "crates/horizon-client/src/game.rs"
   name = "<mod>_menus"
   source = "menus_game.rs"

   [[crate]]
   id = "menus"
   path = "menus"
   ```

3. Describe the menu in your game-side module (a `[[module]]` in `game.rs`, like the one above):

   ```rust
   use super::<mod>_menus as menus;

   fn menu() -> &'static menus::Menu {
       static MENU: std::sync::OnceLock<menus::Menu> = std::sync::OnceLock::new();
       MENU.get_or_init(|| {
           menus::Menu::new("My mod")
               .page("main", "My mod", |p| {
                   p.list("status", 3);                                   // rows you give at draw time
                   p.toggle("enabled", "Enabled", true);
                   p.choice("mode", "Mode", &[("fast", "Fast"), ("pretty", "Pretty")], "fast");
                   p.percent("opacity", "Opacity", 0.5, 0.1, 1.0, 0.05);
                   p.number("radius", "Radius", 32.0, (16.0, 128.0, 16.0), 0, " blocks");
                   p.link("about", "About");
               })
               .page("about", "About", |p| {
                   p.info("Made by me");
                   p.action("hello", "Say hello in chat");
               })
       })
   }
   ```

4. Hook the page (`settings` and `settings_click`, hook version 1) and keep a `menus::Nav` in your state:

   ```rust
   pub(super) fn settings_buttons(game: &Game<'_>, buttons: &mut Vec<String>) {
       let store = menus::GameStore { game, id: ID };
       let rows = with_nav(|nav| menu().render(nav, &store, &mut |list| my_list_rows(list)));
       buttons.extend(rows);
   }

   pub(super) fn settings_click(game: &mut Game<'_>, button: usize) {
       let reverse = menus::reverse(game);
       let event = with_nav(|nav| {
           let mut store = menus::GameStore { game: &*game, id: ID };
           menu().click(nav, &mut store, button, reverse)
       });
       match event {
           menus::Event::Changed(key) => { /* a setting changed: read it again */ }
           menus::Event::Action(id) => { /* run the action */ }
           menus::Event::None => {}
       }
   }
   ```

   Read values anywhere with `menu().bool(&store, "enabled")`, `menu().number(&store, "radius")` and
   `menu().choice(&store, "mode")`: the store's value, or the schema's default.

5. Optionally open it from a key (in your `key` hook): `menus::open_page(game, ID)`. Call `nav.reset()` first to
   always start on the main page.

`with_nav` stands for however your mod keeps state (Horizonmatica keeps the `Nav` in its `thread_local` state and takes
it out while rendering, so that the list callback can read the state too).

## A screen of its own

Besides the mod page, a mod can open **its own screen**, drawn with the game's widgets:

```rust
// in the mod's key hook:
if !menus::open_screen(game) { menus::open_page(game, ID); }

// called every frame while it is open (hooks/screen_draw.rs calls crate::game::<mod>::screen):
pub(super) fn screen(game: &mut Game<'_>, ui: &mut menus::Ui<'_>) {
    ui.text_centered(game, ui.width / 2.0, 8.0, "My mod", 0xFFFFFF);
    if let Some(tab) = ui.tabs(game, [10.0, 20.0, ui.width - 20.0, 20.0], &["One", "Two"], current) { /* ... */ }
    let name = ui.edit(game, "name", [10.0, 50.0, 150.0, 18.0], "Name...", 32);
    if ui.button_tip(game, [10.0, 74.0, 150.0, 20.0], "Save", true, &["Saves it"]) { /* &mut game is yours */ }
    if let Some(v) = ui.slider(game, "volume", [10.0, 98.0, 150.0, 20.0], "Volume", value) { /* 0..1 */ }
    let mut store = menus::GameStore { game: &*game, id: ID };
    let (event, height) = ui.settings(game, menu(), "main", &mut store, [170.0, 50.0, 300.0, 0.0], 2);
    // scroll lists: let (top, mark) = ui.scroll_begin("list", rect, content); ...rows...; ui.scroll_end(...)
}
```

Widgets: `button`, `button_tip`, `slider`, `edit` (the game's EditBox: caret, selection, clipboard), `tabs`,
`scroll_begin`/`scroll_end` (wheel, clipped rows, scroll bar), `item` (icons), `text`, `text_fit`, `text_wrapped`,
`text_right`, `text_centered`, `fill`, `frame`, `tooltip`, `clicked`, `hovered`, `key`, `close`. `settings` lays a
menu page out as toggles, choice buttons and sliders. The screen is immediate-mode: draw it top to bottom each frame,
each widget answers that frame's input. The game keeps running behind it.

**How:** Horizon has no hook for a mod's own screen yet, so the screen takes the place of the Debug Options screen
(F3+F6). Its draw and input functions are hooked (`hooks/*.rs`, `hooks/injections.toml`; `vendor_menus.py --toml`
prints them for your mod) and do nothing of their own while the game hasn't opened that screen; `pauses_game` is
told not to pause for it. These are internal functions, not hook points: the bundle is pinned to a Horizon commit
anyway, but check them when moving to another Nightly.

## Tests

`cargo test --manifest-path core/Cargo.toml` (the crate needs nothing from the game).

License: MIT.
