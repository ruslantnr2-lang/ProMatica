// horizon-menus: text from the input method on promatica's menu screen.
fn promatica_screen_type(game: &mut crate::game::Game<'_>, typed: &str) {
    crate::game::promatica_menus::hook_type(game, typed);
}
