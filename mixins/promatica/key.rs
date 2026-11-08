// The mod's keys (Options > Mods lists and rebinds them).
fn promatica_key(game: &mut crate::game::Game<'_>, key: &str, pressed: bool) {
    crate::game::promatica::key(game, key, pressed);
}
