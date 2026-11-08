// horizon-menus: promatica's menu screen doesn't pause the game.
fn promatica_screen_pauses(game: &crate::game::Game<'_>) -> std::ops::ControlFlow<bool> {
    crate::game::promatica_menus::hook_pauses(game)
}
