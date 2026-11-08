// horizon-menus: the left button let go on promatica's menu screen.
fn promatica_screen_release(game: &mut crate::game::Game<'_>) {
    crate::game::promatica_menus::hook_release(game);
}
