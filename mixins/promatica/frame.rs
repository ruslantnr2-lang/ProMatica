// Each frame: keeps the renderer's options in step and clears it outside a world.
fn promatica_frame(game: &mut crate::game::Game<'_>) {
    crate::game::promatica::frame(game);
}
