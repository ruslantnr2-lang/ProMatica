// The mod's settings page.
fn promatica_settings(game: &mut crate::game::Game<'_>, mod_id: &str, buttons: &mut Vec<String>) {
    if mod_id == crate::game::promatica::ID {
        crate::game::promatica::settings_buttons(game, buttons);
    }
}
