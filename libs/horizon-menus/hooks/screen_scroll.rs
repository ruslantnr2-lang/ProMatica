// horizon-menus: the wheel on MODID's menu screen.
fn MODID_screen_scroll(game: &mut crate::game::Game<'_>, delta: f32) {
    crate::game::MODID_menus::hook_scroll(game, delta);
}
