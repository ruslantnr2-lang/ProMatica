// horizon-menus: a left press on promatica's menu screen.
fn promatica_screen_click(game: &mut crate::game::Game<'_>) {
    crate::game::promatica_menus::hook_click(game);
}
