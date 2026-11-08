// horizon-menus: the wheel on promatica's menu screen.
fn promatica_screen_scroll(game: &mut crate::game::Game<'_>, delta: f32) {
    crate::game::promatica_menus::hook_scroll(game, delta);
}
