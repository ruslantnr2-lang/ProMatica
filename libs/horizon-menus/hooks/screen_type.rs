// horizon-menus: text from the input method on MODID's menu screen.
fn MODID_screen_type(game: &mut crate::game::Game<'_>, typed: &str) {
    crate::game::MODID_menus::hook_type(game, typed);
}
