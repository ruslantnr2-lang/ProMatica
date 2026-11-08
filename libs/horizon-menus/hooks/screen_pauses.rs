// horizon-menus: MODID's menu screen doesn't pause the game.
fn MODID_screen_pauses(game: &crate::game::Game<'_>) -> std::ops::ControlFlow<bool> {
    crate::game::MODID_menus::hook_pauses(game)
}
