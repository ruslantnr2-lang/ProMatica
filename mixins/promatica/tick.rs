// Each game tick: verifier scan, overlay rebuild, printer.
fn promatica_tick(game: &mut crate::game::Game<'_>) {
    crate::game::promatica::tick(game);
}
