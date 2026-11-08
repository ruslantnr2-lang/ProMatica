// A click on one of the settings page's buttons.
fn promatica_click(game: &mut crate::game::Game<'_>, mod_id: &str, button: usize) {
    if mod_id == crate::game::promatica::ID {
        crate::game::promatica::settings_click(game, button);
    }
}
