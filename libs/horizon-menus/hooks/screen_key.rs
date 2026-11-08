// horizon-menus: a key on MODID's menu screen (the game's handling skipped while it is open).
fn MODID_screen_key(game: &mut crate::game::Game<'_>, event: &winit::event::KeyEvent) -> std::ops::ControlFlow<()> {
    crate::game::MODID_menus::hook_key(game, event)
}
