// horizon-menus: a key on promatica's menu screen (the game's handling skipped while it is open).
fn promatica_screen_key(game: &mut crate::game::Game<'_>, event: &winit::event::KeyEvent) -> std::ops::ControlFlow<()> {
    crate::game::promatica_menus::hook_key(game, event)
}
