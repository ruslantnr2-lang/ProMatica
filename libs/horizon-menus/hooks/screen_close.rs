// horizon-menus: the Debug Options screen closed (Esc, F3+F6): so did MODID's menu.
fn MODID_screen_close(game: &mut crate::game::Game<'_>) {
    crate::game::MODID_menus::hook_close(game);
}
