// The selection tool: with it in hand, the mouse buttons set the area's corners (the game's own
// break, use and pick are skipped for those clicks). Without it, a middle click on a block the
// schematic wants picks that block.
fn promatica_mouse(game: &mut crate::game::Game<'_>, button: winit::event::MouseButton, pressed: bool) -> std::ops::ControlFlow<Result<()>> {
    if crate::game::promatica::mouse(game, button, pressed) {
        std::ops::ControlFlow::Break(Ok(()))
    } else {
        std::ops::ControlFlow::Continue(())
    }
}
