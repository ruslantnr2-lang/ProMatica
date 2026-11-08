// horizon-menus: the Debug Options screen closed (Esc, F3+F6): so did promatica's menu.
fn promatica_screen_close(game: &mut crate::game::Game<'_>) {
    crate::game::promatica_menus::hook_close(game);
}
