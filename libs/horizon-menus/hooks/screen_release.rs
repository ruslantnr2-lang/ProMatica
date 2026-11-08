// horizon-menus: the left button let go on MODID's menu screen.
fn MODID_screen_release(game: &mut crate::game::Game<'_>) {
    crate::game::MODID_menus::hook_release(game);
}
