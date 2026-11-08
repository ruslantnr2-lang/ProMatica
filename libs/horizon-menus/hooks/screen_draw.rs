// horizon-menus: draws MODID's menu screen, in the Debug Options screen's place.
fn MODID_screen_draw(game: &mut crate::game::Game<'_>, batch: &mut horizon_client::hud::HudBatch) {
    crate::game::MODID_menus::draw(game, batch, crate::game::MODID::screen);
}
