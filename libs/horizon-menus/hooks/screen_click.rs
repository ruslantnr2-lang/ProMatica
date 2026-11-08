// horizon-menus: a left press on MODID's menu screen.
fn MODID_screen_click(game: &mut crate::game::Game<'_>) {
    crate::game::MODID_menus::hook_click(game);
}
